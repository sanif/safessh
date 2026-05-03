//! Shared markdown section installer used by AGENTS.md-style targets.
//!
//! "Section style" means: a single `## safessh` H2 inside a larger markdown
//! file, where the rest of the file must be preserved across install /
//! update / uninstall.

use safessh_core::error::{Error, Result};
use safessh_storage::atomic;
use std::path::Path;

const HEADER: &str = "## safessh";

/// Install or replace the `## safessh` section in `path`. The body is
/// written verbatim — caller is responsible for ensuring it begins with
/// the `## safessh` header line (i.e., wrap with `agents_md::format`).
pub fn install_md_section(path: &Path, body_with_header: &str) -> Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let cleaned = strip_section(&existing);
    let combined = if cleaned.trim().is_empty() {
        body_with_header.to_string()
    } else {
        format!("{}\n\n{}", cleaned.trim_end(), body_with_header)
    };
    atomic::write_string(path, &combined).map_err(Error::Io)
}

/// Remove the `## safessh` section from `path`, preserving the rest of the
/// file. The cleaned content is trimmed of trailing whitespace.
pub fn uninstall_md_section(path: &Path) -> Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let cleaned = strip_section(&existing);
    atomic::write_string(path, cleaned.trim_end()).map_err(Error::Io)
}

/// Strip the `## safessh` section from `content`. Used internally by
/// install/uninstall and exposed for callers that need a pure preview
/// (e.g., `safessh skill update --dry-run` in Task 16).
pub fn strip_section(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = vec![];
    let mut skipping = false;
    for line in &lines {
        if line.trim_start().starts_with(HEADER) {
            skipping = true;
            continue;
        }
        if skipping {
            if line.starts_with("## ") || line.starts_with("# ") {
                skipping = false;
                out.push(*line);
            }
            continue;
        }
        out.push(*line);
    }
    out.join("\n")
}
