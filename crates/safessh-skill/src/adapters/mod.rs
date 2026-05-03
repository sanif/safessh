//! Format adapters that wrap the canonical skill body for each supported
//! agent framework.

pub mod agents_md;
pub mod claude_code;
pub mod codex;
pub mod cursor;
pub mod gemini_cli;
pub mod plain;

/// Target agent framework / surface for the formatted skill content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Claude Code skill file (`safessh.md` with YAML frontmatter).
    ClaudeCode,
    /// `AGENTS.md` section.
    AgentsMd,
    /// Cursor rules file (`safessh.md` with cursor-style frontmatter).
    Cursor,
    /// Gemini CLI section in `GEMINI.md`.
    GeminiCli,
    /// OpenAI Codex CLI section in `~/.codex/AGENTS.md`.
    Codex,
    /// Plain markdown body — no wrapping, no section header. Requires --path.
    Plain,
}

/// Format the given canonical body for the requested target.
pub fn format(target: Target, body: &str) -> String {
    match target {
        Target::ClaudeCode => claude_code::format(body),
        Target::AgentsMd => agents_md::format(body),
        Target::Cursor => cursor::format(body),
        Target::GeminiCli => gemini_cli::format(body),
        Target::Codex => codex::format(body),
        Target::Plain => plain::format(body),
    }
}

/// Default filename for the formatted skill content for the given target.
pub fn filename(target: Target) -> &'static str {
    match target {
        Target::ClaudeCode => "safessh.md",
        Target::AgentsMd => "AGENTS.md",
        Target::Cursor => "safessh.md",
        Target::GeminiCli => "GEMINI.md",
        Target::Codex => "AGENTS.md",
        Target::Plain => "safessh.md",
    }
}
