//! safessh-skill — Skill content generator and host-format adapters.

pub mod adapters;
pub mod detection;
pub mod install;

/// The canonical, format-agnostic skill body shipped to agent frameworks.
pub const CONTENT: &str = include_str!("content/safessh.md");
