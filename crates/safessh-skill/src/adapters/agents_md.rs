//! AGENTS.md skill format adapter — wraps the body in a `## safessh` section.

/// Wrap the canonical skill body as an `## safessh` section suitable for
/// inclusion in an `AGENTS.md` file.
pub fn format(body: &str) -> String {
    format!("## safessh\n\n{body}\n")
}
