//! OpenAI Codex CLI adapter — section in `~/.codex/AGENTS.md` (user scope).

pub fn format(body: &str) -> String {
    crate::adapters::agents_md::format(body)
}
