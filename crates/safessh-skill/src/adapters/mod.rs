//! Format adapters that wrap the canonical skill body for each supported
//! agent framework.

pub mod agents_md;
pub mod claude_code;
pub mod cursor;

/// Target agent framework / surface for the formatted skill content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Claude Code skill file (`safessh.md` with YAML frontmatter).
    ClaudeCode,
    /// `AGENTS.md` section.
    AgentsMd,
    /// Cursor rules file (`safessh.md` with cursor-style frontmatter).
    Cursor,
}

/// Format the given canonical body for the requested target.
pub fn format(target: Target, body: &str) -> String {
    match target {
        Target::ClaudeCode => claude_code::format(body),
        Target::AgentsMd => agents_md::format(body),
        Target::Cursor => cursor::format(body),
    }
}

/// Default filename for the formatted skill content for the given target.
pub fn filename(target: Target) -> &'static str {
    match target {
        Target::ClaudeCode => "safessh.md",
        Target::AgentsMd => "AGENTS.md",
        Target::Cursor => "safessh.md",
    }
}
