//! Parse `~/.ssh/config` host aliases for project import.
//!
//! Cached to `~/.cache/safessh/ssh-config-snapshot.toml` so the TUI projects
//! screen doesn't re-parse on every key press. mtime-invalidated.

use crate::atomic;
use crate::paths::Paths;
use safessh_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::SystemTime;

/// SSH-config alias snapshot.
///
/// Note: `ProxyJump` is intentionally omitted. The `ssh2-config` 0.3 crate
/// routes ProxyJump into its `unsupported_fields` sink rather than exposing
/// a typed field. Users who need ProxyJump should reference the alias via
/// `Target::SshConfigAlias` (`safessh project add --alias <name>`) which
/// delegates to `ssh` at exec time and respects the full ssh-config spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshAlias {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SshConfigSnapshot {
    pub source_mtime_secs: i64,
    pub aliases: Vec<SshAlias>,
}

fn source_path() -> PathBuf {
    if let Ok(p) = std::env::var("SSH_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".ssh/config")
}

impl SshConfigSnapshot {
    pub fn load(paths: &Paths) -> Result<Self> {
        let src = source_path();
        let src_mtime = match std::fs::metadata(&src) {
            Ok(m) => m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            Err(_) => return Ok(Self::default()),
        };

        let cache_path = paths.ssh_config_snapshot();
        if let Ok(raw) = std::fs::read_to_string(&cache_path) {
            if let Ok(cached) = toml::from_str::<Self>(&raw) {
                if cached.source_mtime_secs == src_mtime {
                    return Ok(cached);
                }
            }
        }

        let aliases = parse_file(&src)?;
        let snap = Self {
            source_mtime_secs: src_mtime,
            aliases,
        };
        // SAFETY-INVARIANT-5: tempfile + rename via atomic::write_string ensures
        // no half-written snapshot file is ever observable by concurrent readers.
        let toml = toml::to_string_pretty(&snap).map_err(|e| Error::Serde(e.to_string()))?;
        atomic::write_string(&cache_path, &toml)?;
        Ok(snap)
    }
}

fn parse_file(path: &std::path::Path) -> Result<Vec<SshAlias>> {
    use ssh2_config::{ParseRule, SshConfig};
    use std::io::BufReader;

    let file = std::fs::File::open(path).map_err(Error::Io)?;
    let mut reader = BufReader::new(file);
    let cfg = SshConfig::default()
        .parse(&mut reader, ParseRule::ALLOW_UNKNOWN_FIELDS)
        .map_err(|e| Error::Storage(format!("ssh-config parse: {e}")))?;

    let mut out = Vec::new();
    for host in cfg.get_hosts() {
        // A Host entry may have multiple clauses (e.g. `Host foo bar`).
        // We expand each non-wildcard clause into its own alias.
        for clause in &host.pattern {
            let alias = clause.pattern.as_str();
            // Skip wildcard / glob entries — useless for project import.
            if alias.contains('*') || alias.contains('?') || alias.is_empty() {
                continue;
            }
            let params = &host.params;
            out.push(SshAlias {
                alias: alias.to_string(),
                hostname: params.host_name.clone(),
                user: params.user.clone(),
                port: params.port,
                identity_file: params
                    .identity_file
                    .as_deref()
                    .and_then(|v| v.first().cloned()),
            });
        }
    }
    out.sort_by(|a, b| a.alias.cmp(&b.alias));
    out.dedup_by_key(|a| a.alias.clone());
    Ok(out)
}
