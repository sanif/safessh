//! Install / uninstall the safessh skill into a target framework's location.
//!
//! For `Target::ClaudeCode`, we write the formatted skill file directly.
//! For `Target::AgentsMd`, we append (or replace) a `## safessh` section in
//! the existing `AGENTS.md` without disturbing other content.

use crate::adapters::{format, Target};
use crate::CONTENT;
use safessh_core::error::{Error, Result};
use safessh_storage::atomic;
use std::path::{Path, PathBuf};

/// Where to install the skill.
#[derive(Clone, Copy, Debug)]
pub enum Scope {
    /// User-level config (e.g., `~/.claude/skills/...`).
    User,
    /// Project-level config rooted at the supplied `cwd`.
    Project,
    /// Caller-provided custom path.
    Path,
}

/// Write the skill content for `target` to `dest`.
///
/// For `Target::AgentsMd`, this appends or replaces the `## safessh` section
/// rather than overwriting the entire file.
pub fn install_to(target: Target, dest: &Path) -> Result<()> {
    let body = format(target, CONTENT);
    if matches!(target, Target::AgentsMd) {
        return install_agents_md_section(dest, &body);
    }
    atomic::write_string(dest, &body).map_err(Error::Io)?;
    Ok(())
}

fn install_agents_md_section(path: &Path, body: &str) -> Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let cleaned = strip_safessh_section(&existing);
    let combined = if cleaned.trim().is_empty() {
        body.to_string()
    } else {
        format!("{}\n\n{}", cleaned.trim_end(), body)
    };
    atomic::write_string(path, &combined).map_err(Error::Io)?;
    Ok(())
}

/// Remove the existing `## safessh` section from an AGENTS.md-style document.
///
/// Skipping starts at a line whose `trim_start()` begins with `## safessh`
/// and stops as soon as we hit any later line starting with `## ` or `# `
/// (which is preserved). If the file ends inside the safessh section, we
/// simply finish with `skipping=true` and emit nothing further.
fn strip_safessh_section(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = vec![];
    let mut skipping = false;
    for line in &lines {
        if line.trim_start().starts_with("## safessh") {
            skipping = true;
            continue;
        }
        if skipping {
            // Resume copying once we hit the next top-level / section header.
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

/// Remove the safessh skill from `dest`.
///
/// For `Target::ClaudeCode`, deletes the file if present.
/// For `Target::AgentsMd`, strips only the `## safessh` section, preserving
/// the rest of the file.
pub fn uninstall_at(target: Target, dest: &Path) -> Result<()> {
    if matches!(target, Target::AgentsMd) {
        let existing = std::fs::read_to_string(dest).unwrap_or_default();
        let cleaned = strip_safessh_section(&existing);
        atomic::write_string(dest, cleaned.trim_end()).map_err(Error::Io)?;
        return Ok(());
    }
    if dest.exists() {
        std::fs::remove_file(dest).map_err(Error::Io)?;
    }
    Ok(())
}

/// Resolve the default install path for the given (`target`, `scope`) pair.
///
/// Returns `None` for unsupported combinations (e.g., AgentsMd at User scope
/// or `Scope::Path`, where the caller must supply an explicit path).
pub fn default_path(target: Target, scope: Scope, cwd: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(match (target, scope) {
        (Target::ClaudeCode, Scope::User) => home.join(".claude/skills/safessh.md"),
        (Target::ClaudeCode, Scope::Project) => cwd.join(".claude/skills/safessh.md"),
        (Target::AgentsMd, Scope::Project) => cwd.join("AGENTS.md"),
        _ => return None,
    })
}

/// Stable hash of the embedded skill body, used by `safessh skill check` to
/// detect drift between installed copies and the current binary.
pub fn current_hash() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    CONTENT.hash(&mut h);
    format!("{:x}", h.finish())
}
