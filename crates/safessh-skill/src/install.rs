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
    crate::sections::install_md_section(path, body)
}

/// Remove the safessh skill from `dest`.
///
/// For `Target::ClaudeCode`, deletes the file if present.
/// For `Target::AgentsMd`, strips only the `## safessh` section, preserving
/// the rest of the file.
pub fn uninstall_at(target: Target, dest: &Path) -> Result<()> {
    if matches!(target, Target::AgentsMd) {
        return crate::sections::uninstall_md_section(dest);
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
        (Target::Cursor, Scope::Project) => cwd.join(".cursor/rules/safessh.md"),
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
