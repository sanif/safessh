//! Gemini CLI adapter — section-style format under `GEMINI.md`.
//!
//! Wrapping is identical to AGENTS.md; we keep a separate adapter so the
//! Target enum can express different default install paths and detection.

pub fn format(body: &str) -> String {
    crate::adapters::agents_md::format(body)
}
