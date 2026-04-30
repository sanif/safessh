//! `safessh audit query` — read the JSONL audit log line-by-line and apply
//! optional filters (`--project`, `--type`, `--grep`).
//!
//! Output is raw JSONL; matching lines are printed verbatim. A missing audit
//! log is treated as an empty result (not an error) so the command is safe to
//! run on a fresh install.

use safessh_core::error::{Error, Result};
use safessh_storage::paths::Paths;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};

pub fn run(
    project: Option<String>,
    event_type: Option<String>,
    grep: Option<String>,
) -> Result<()> {
    let paths = Paths::user().map_err(Error::Io)?;
    let log_path = paths.audit_log();

    if !log_path.exists() {
        return Ok(());
    }

    let file = File::open(&log_path).map_err(Error::Io)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            // Skip unreadable lines silently — keep going to surface as much
            // log content as possible.
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        // Parse for `--project` / `--type` filters. If parsing fails, the
        // line cannot possibly match a structured filter, but it should
        // still be considered for `--grep` (free-text search over JSONL).
        let parsed: Option<Value> = serde_json::from_str(&line).ok();

        if let Some(want) = project.as_deref() {
            let matches = parsed
                .as_ref()
                .and_then(|v| v.get("project"))
                .and_then(|v| v.as_str())
                .map(|s| s == want)
                .unwrap_or(false);
            if !matches {
                continue;
            }
        }

        if let Some(want) = event_type.as_deref() {
            let matches = parsed
                .as_ref()
                .and_then(|v| v.get("event_type"))
                .and_then(|v| v.as_str())
                .map(|s| s == want)
                .unwrap_or(false);
            if !matches {
                continue;
            }
        }

        if let Some(pat) = grep.as_deref() {
            if !line.contains(pat) {
                continue;
            }
        }

        println!("{line}");
    }

    Ok(())
}
