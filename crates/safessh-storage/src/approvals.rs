//! Approval stores: pending, timed, always, and blocked rule lists.
//!
//! All writes go through [`atomic::write_string`] under an exclusive
//! [`LockedFile`] so concurrent CLI/TUI invocations cannot corrupt each
//! other (SAFETY-INVARIANT-12). Timed-rule expiry is wall-clock-based
//! via `Utc::now()` (SAFETY-INVARIANT-7).

use crate::atomic;
use crate::locking::LockedFile;
use crate::paths::Paths;
use chrono::{DateTime, Duration, Utc};
use safessh_core::error::{Error, Result};
use safessh_core::types::{ApprovalToken, ParsedCommand};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A pending approval request, persisted as `approvals/pending/<token>.toml`
/// until the user grants or rejects it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequest {
    pub token: String,
    pub project: String,
    pub categories: Vec<String>,
    pub parsed: ParsedCommand,
    pub raw: String,
    pub created_at: DateTime<Utc>,
    /// Remote path for file-op approvals (`file:read` / `file:write`).
    /// `None` for exec approvals. Existing pending files without this field
    /// parse fine via `#[serde(default)]` (backward compat).
    #[serde(default)]
    pub path: Option<String>,
    /// Tunnel spec string for `network:tunnel` approvals (`local:host:port`).
    /// `None` for exec / file approvals. Backward-compat via `serde(default)`.
    #[serde(default)]
    pub tunnel: Option<String>,
}

/// A pattern-based rule shared by `always` and `blocked` stores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternRule {
    pub rule_id: String,
    pub binary: String,
    pub flags: Vec<String>,
    pub args_pattern: Option<String>,
    pub categories: Vec<String>,
    /// `Some("network:tunnel")` for category-level rules used by tunnel
    /// approval; `None` for exec rules that match by `binary` + flags.
    /// Backward-compat via `serde(default)`.
    #[serde(default)]
    pub category: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A timed rule: a [`PatternRule`] flattened together with a wall-clock
/// expiry timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedRule {
    #[serde(flatten)]
    pub pattern: PatternRule,
    pub expires_at: DateTime<Utc>,
}

/// Generic newtype around a list of rules; serialized as `{ rules = [...] }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleList<T> {
    pub rules: Vec<T>,
}

impl<T> Default for RuleList<T> {
    fn default() -> Self {
        Self { rules: Vec::new() }
    }
}

pub struct PendingStore {
    dir: PathBuf,
}
pub struct TimedStore {
    dir: PathBuf,
}
pub struct AlwaysStore {
    dir: PathBuf,
}
pub struct BlockedStore {
    dir: PathBuf,
}

impl PendingStore {
    pub fn new(paths: &Paths) -> Self {
        Self {
            dir: paths.approvals_dir().join("pending"),
        }
    }

    /// Persist a new pending request to `approvals/pending/<token>.toml`.
    pub fn add(&self, request: &PendingRequest) -> Result<()> {
        let path = self.dir.join(format!("{}.toml", request.token));
        let toml = toml::to_string_pretty(request).map_err(|e| Error::Serde(e.to_string()))?;
        atomic::write_string(&path, &toml)?;
        Ok(())
    }

    /// Read and remove a pending request by token. Returns `Error::Usage`
    /// if the token is unknown.
    pub fn take(&self, token: &ApprovalToken) -> Result<PendingRequest> {
        let path = self.dir.join(format!("{}.toml", token.as_str()));
        let raw = std::fs::read_to_string(&path)
            .map_err(|_| Error::Usage(format!("no pending approval: {}", token.as_str())))?;
        let req: PendingRequest =
            toml::from_str(&raw).map_err(|e| Error::Storage(e.to_string()))?;
        std::fs::remove_file(&path).ok();
        Ok(req)
    }

    /// Remove pending requests older than `max_age_hours`. Returns the
    /// number of files removed.
    pub fn cleanup_expired(&self, max_age_hours: i64) -> Result<usize> {
        if !self.dir.exists() {
            return Ok(0);
        }
        let cutoff = Utc::now() - Duration::hours(max_age_hours);
        let mut removed = 0;
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            if let Ok(raw) = std::fs::read_to_string(entry.path()) {
                if let Ok(req) = toml::from_str::<PendingRequest>(&raw) {
                    if req.created_at < cutoff {
                        let _ = std::fs::remove_file(entry.path());
                        removed += 1;
                    }
                }
            }
        }
        Ok(removed)
    }
}

/// Serialize `list` to TOML and atomically write to `path` while holding an
/// exclusive advisory lock (SAFETY-INVARIANT-12).
fn save_locked<T: Serialize>(path: &Path, list: &RuleList<T>) -> Result<()> {
    let _lock = LockedFile::open_exclusive(path)?;
    let toml = toml::to_string_pretty(list).map_err(|e| Error::Serde(e.to_string()))?;
    atomic::write_string(path, &toml)?;
    Ok(())
}

/// Load a `RuleList<T>` from `path`, returning the default empty list if the
/// file does not exist.
fn load_or_default<T>(path: &Path) -> Result<RuleList<T>>
where
    T: for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(RuleList::default());
    }
    let raw = std::fs::read_to_string(path)?;
    toml::from_str(&raw).map_err(|e| Error::Storage(e.to_string()))
}

impl TimedStore {
    pub fn new(paths: &Paths) -> Self {
        Self {
            dir: paths.approvals_dir().join("timed"),
        }
    }

    /// Append `rule` to `approvals/timed/<project>.toml` under exclusive lock.
    pub fn add(&self, project: &str, rule: TimedRule) -> Result<()> {
        let path = self.dir.join(format!("{project}.toml"));
        let mut list: RuleList<TimedRule> = load_or_default(&path)?;
        list.rules.push(rule);
        save_locked(&path, &list)
    }

    // SAFETY-INVARIANT-7: Expiry is wall-clock-based via `Utc::now()`
    // comparison against stored `expires_at`. Never derived from process
    // lifetime, monotonic clocks, or cached "now" values.
    pub fn list_active(&self, project: &str) -> Result<Vec<TimedRule>> {
        let path = self.dir.join(format!("{project}.toml"));
        let list: RuleList<TimedRule> = load_or_default(&path)?;
        let now = Utc::now();
        Ok(list
            .rules
            .into_iter()
            .filter(|r| r.expires_at > now)
            .collect())
    }

    /// Remove a timed rule by id. Mirrors [`AlwaysStore::remove`] /
    /// [`BlockedStore::remove`] so the TUI rules screen can delete from
    /// any of the three persistent stores uniformly.
    pub fn remove(&self, project: &str, rule_id: &str) -> Result<()> {
        let path = self.dir.join(format!("{project}.toml"));
        let list: RuleList<TimedRule> = load_or_default(&path)?;
        let kept: Vec<_> = list
            .rules
            .into_iter()
            .filter(|r| r.pattern.rule_id != rule_id)
            .collect();
        save_locked(&path, &RuleList { rules: kept })
    }

    /// Rewrite the project's timed-rule file with expired entries removed.
    /// Returns the number of rules removed.
    pub fn purge_expired(&self, project: &str) -> Result<usize> {
        let path = self.dir.join(format!("{project}.toml"));
        let list: RuleList<TimedRule> = load_or_default(&path)?;
        let now = Utc::now();
        let original = list.rules.len();
        let kept: Vec<_> = list
            .rules
            .into_iter()
            .filter(|r| r.expires_at > now)
            .collect();
        let removed = original - kept.len();
        if removed > 0 {
            save_locked(&path, &RuleList { rules: kept })?;
        }
        Ok(removed)
    }
}

impl AlwaysStore {
    pub fn new(paths: &Paths) -> Self {
        Self {
            dir: paths.approvals_dir().join("always"),
        }
    }

    pub fn add(&self, project: &str, rule: PatternRule) -> Result<()> {
        let path = self.dir.join(format!("{project}.toml"));
        let mut list: RuleList<PatternRule> = load_or_default(&path)?;
        list.rules.push(rule);
        save_locked(&path, &list)
    }

    pub fn list(&self, project: &str) -> Result<Vec<PatternRule>> {
        let path = self.dir.join(format!("{project}.toml"));
        let list: RuleList<PatternRule> = load_or_default(&path)?;
        Ok(list.rules)
    }

    pub fn remove(&self, project: &str, rule_id: &str) -> Result<()> {
        let path = self.dir.join(format!("{project}.toml"));
        let list: RuleList<PatternRule> = load_or_default(&path)?;
        let kept: Vec<_> = list
            .rules
            .into_iter()
            .filter(|r| r.rule_id != rule_id)
            .collect();
        save_locked(&path, &RuleList { rules: kept })
    }
}

impl BlockedStore {
    pub fn new(paths: &Paths) -> Self {
        Self {
            dir: paths.approvals_dir().join("blocked"),
        }
    }

    pub fn add(&self, project: &str, rule: PatternRule) -> Result<()> {
        let path = self.dir.join(format!("{project}.toml"));
        let mut list: RuleList<PatternRule> = load_or_default(&path)?;
        list.rules.push(rule);
        save_locked(&path, &list)
    }

    pub fn list(&self, project: &str) -> Result<Vec<PatternRule>> {
        let path = self.dir.join(format!("{project}.toml"));
        let list: RuleList<PatternRule> = load_or_default(&path)?;
        Ok(list.rules)
    }

    pub fn remove(&self, project: &str, rule_id: &str) -> Result<()> {
        let path = self.dir.join(format!("{project}.toml"));
        let list: RuleList<PatternRule> = load_or_default(&path)?;
        let kept: Vec<_> = list
            .rules
            .into_iter()
            .filter(|r| r.rule_id != rule_id)
            .collect();
        save_locked(&path, &RuleList { rules: kept })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Existing pending files written without the `path` field must still
    /// deserialize successfully with `path = None` (backward compat).
    #[test]
    fn pending_request_without_path_parses_as_none() {
        let toml_without_path = r#"
token = "ABC123"
project = "prod"
categories = ["destructive:filesystem"]
raw = "rm -rf /tmp/x"
created_at = "2024-01-01T00:00:00Z"

[parsed]
binary = "rm"
flags = ["-rf"]
args = ["/tmp/x"]
redirects = []
pipes = []
env_mutations = []
raw = "rm -rf /tmp/x"
"#;
        let req: PendingRequest = toml::from_str(toml_without_path).expect("should parse");
        assert_eq!(req.path, None, "path should default to None");
        assert_eq!(req.token, "ABC123");
    }

    /// Pending files that include `path` parse correctly.
    #[test]
    fn pending_request_with_path_parses_correctly() {
        let toml_with_path = r#"
token = "XYZ789"
project = "prod"
categories = ["file:read"]
raw = "read /etc/hosts"
created_at = "2024-01-01T00:00:00Z"
path = "/etc/hosts"

[parsed]
binary = ""
flags = []
args = []
redirects = []
pipes = []
env_mutations = []
raw = "read /etc/hosts"
"#;
        let req: PendingRequest = toml::from_str(toml_with_path).expect("should parse");
        assert_eq!(req.path, Some("/etc/hosts".to_string()));
        assert_eq!(req.categories, vec!["file:read"]);
    }
}
