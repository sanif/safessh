//! Claude Code skill format adapter — wraps the body in YAML frontmatter.

/// Skill description that drives Claude Code's skill activation heuristics.
pub const DESCRIPTION: &str = "SSH proxy for running gated commands on user-configured servers without seeing credentials. Use when the user asks to run commands on a remote server they've configured in safessh.";

/// Wrap the canonical skill body in Claude Code YAML frontmatter.
pub fn format(body: &str) -> String {
    format!("---\nname: safessh\ndescription: {DESCRIPTION}\n---\n\n{body}")
}
