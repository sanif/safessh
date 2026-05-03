//! Cursor rules format adapter — wraps the body in cursor-style frontmatter.

pub const DESCRIPTION: &str = "SSH proxy for running gated commands on user-configured servers without seeing credentials. Use when the user asks to run commands on a remote server they've configured in safessh.";

pub fn format(body: &str) -> String {
    format!("---\ndescription: {DESCRIPTION}\nglobs:\nalwaysApply: false\n---\n\n{body}")
}
