//! Detection of installed agent frameworks and their default skill paths.

use crate::adapters::Target;
use std::path::{Path, PathBuf};

/// A detected agent framework with its candidate user/project install paths.
///
/// `user_path` and `project_path` are `Some` only when the framework is
/// considered "installed" at that scope (e.g., `~/.claude` exists). For
/// `AgentsMd`, only `project_path` is populated (the file may not yet exist
/// — install will create it).
#[derive(Clone, Debug)]
pub struct Detected {
    pub target: Target,
    pub user_path: Option<PathBuf>,
    pub project_path: Option<PathBuf>,
}

/// Detect installed agent frameworks at user scope (`$HOME`) and project
/// scope (`cwd`), returning a `Detected` entry per supported target.
pub fn detect(cwd: &Path) -> Vec<Detected> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let mut out = vec![];

    // Claude Code: detected if ~/.claude (user) or <cwd>/.claude (project) exists.
    let claude_user = home.join(".claude/skills/safessh.md");
    let claude_proj = cwd.join(".claude/skills/safessh.md");
    out.push(Detected {
        target: Target::ClaudeCode,
        user_path: home.join(".claude").exists().then_some(claude_user),
        project_path: cwd.join(".claude").exists().then_some(claude_proj),
    });

    // AGENTS.md: project-scope only; file may or may not yet exist.
    out.push(Detected {
        target: Target::AgentsMd,
        user_path: None,
        project_path: Some(cwd.join("AGENTS.md")),
    });

    let cursor_proj = cwd.join(".cursor/rules/safessh.md");
    out.push(Detected {
        target: Target::Cursor,
        user_path: None,
        project_path: cwd.join(".cursor").exists().then_some(cursor_proj),
    });

    let gemini_user = home.join(".gemini/GEMINI.md");
    let gemini_proj = cwd.join("GEMINI.md");
    out.push(Detected {
        target: Target::GeminiCli,
        user_path: home.join(".gemini").exists().then_some(gemini_user),
        project_path: if gemini_proj.exists() {
            Some(gemini_proj)
        } else {
            None
        },
    });

    let codex_user = home.join(".codex/AGENTS.md");
    out.push(Detected {
        target: Target::Codex,
        user_path: home.join(".codex").exists().then_some(codex_user),
        project_path: None,
    });

    out
}
