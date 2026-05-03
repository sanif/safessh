//! `safessh skill {install,uninstall,show,check}` — manage skill files in
//! the user's agent frameworks.
//!
//! v0.1 design notes:
//! * `install` / `uninstall` require `--target` to be specified explicitly
//!   (or `--target all` to fan out across detected frameworks). Interactive
//!   multi-select is deferred; the `SAFESSH_PROMPT_RESPONSE` env var path is
//!   honored by the prompt module elsewhere but not used here.
//! * `show` formats and prints the canonical content for the requested
//!   target (defaults to `claude-code`).
//! * `check` walks `detection::detect` and reports installed-vs-missing plus
//!   hash drift per framework.

use crate::cli::{SkillCmd, SkillScope};
use safessh_core::error::{Error, Result};
use safessh_skill::adapters::{format, Target};
use safessh_skill::detection;
use safessh_skill::install::{current_hash, default_path, install_to, uninstall_at, Scope};
use safessh_skill::CONTENT;
use std::path::{Path, PathBuf};

pub fn run(cmd: SkillCmd) -> Result<()> {
    match cmd {
        SkillCmd::Install {
            target,
            scope,
            path,
        } => install(target, scope, path),
        SkillCmd::Uninstall {
            target,
            scope,
            path,
        } => uninstall(target, scope, path),
        SkillCmd::Show { target } => show(target),
        SkillCmd::Check => check(),
    }
}

/// Parse the `--target` string into a `Target`. Returns `Err` for unknown
/// values; the special string `"all"` is handled by callers before reaching
/// this function.
fn parse_target(s: &str) -> Result<Target> {
    match s {
        "claude-code" => Ok(Target::ClaudeCode),
        "agents-md" | "agents.md" | "AGENTS.md" => Ok(Target::AgentsMd),
        other => Err(Error::Usage(format!(
            "unknown skill target: {other} (expected: claude-code, agents-md, all)"
        ))),
    }
}

fn map_scope(scope: SkillScope) -> Scope {
    match scope {
        SkillScope::User => Scope::User,
        SkillScope::Project => Scope::Project,
        SkillScope::Path => Scope::Path,
    }
}

fn cwd() -> Result<PathBuf> {
    std::env::current_dir().map_err(Error::Io)
}

fn install(target: Option<String>, scope: SkillScope, path: Option<PathBuf>) -> Result<()> {
    let target_str = target.ok_or_else(|| {
        Error::Usage(
            "specify --target <claude-code|agents-md|all> (interactive prompt is v0.2)".into(),
        )
    })?;

    let cwd = cwd()?;

    if target_str == "all" {
        // Fan out: install at user-level for ClaudeCode if detected, and at
        // project-level for AgentsMd (its only supported scope).
        let mut installed: Vec<String> = vec![];
        for det in detection::detect(&cwd) {
            match det.target {
                Target::ClaudeCode => {
                    if let Some(p) = det.user_path.clone() {
                        ensure_parent(&p)?;
                        install_to(Target::ClaudeCode, &p)?;
                        installed.push(format!("claude-code (user): {}", p.display()));
                    }
                }
                Target::AgentsMd => {
                    if let Some(p) = det.project_path.clone() {
                        ensure_parent(&p)?;
                        install_to(Target::AgentsMd, &p)?;
                        installed.push(format!("agents-md (project): {}", p.display()));
                    }
                }
                Target::Cursor => {
                    if let Some(p) = det.project_path.clone() {
                        ensure_parent(&p)?;
                        install_to(Target::Cursor, &p)?;
                        installed.push(format!("cursor (project): {}", p.display()));
                    }
                }
            }
        }
        if installed.is_empty() {
            println!("No agent frameworks detected.");
        } else {
            for line in installed {
                println!("Installed {line}");
            }
        }
        return Ok(());
    }

    let target = parse_target(&target_str)?;
    let dest = resolve_dest(target, scope, path, &cwd)?;
    ensure_parent(&dest)?;
    install_to(target, &dest)?;
    println!("Installed {target_str}: {}", dest.display());
    Ok(())
}

fn uninstall(target: Option<String>, scope: SkillScope, path: Option<PathBuf>) -> Result<()> {
    let target_str =
        target.ok_or_else(|| Error::Usage("specify --target <claude-code|agents-md>".into()))?;
    let cwd = cwd()?;
    let target = parse_target(&target_str)?;
    let dest = resolve_dest(target, scope, path, &cwd)?;
    uninstall_at(target, &dest)?;
    println!("Uninstalled {target_str}: {}", dest.display());
    Ok(())
}

fn resolve_dest(
    target: Target,
    scope: SkillScope,
    path: Option<PathBuf>,
    cwd: &Path,
) -> Result<PathBuf> {
    if matches!(scope, SkillScope::Path) {
        let dir = path.ok_or_else(|| Error::Usage("--scope path requires --path <dir>".into()))?;
        return Ok(dir.join(safessh_skill::adapters::filename(target)));
    }
    default_path(target, map_scope(scope), cwd).ok_or_else(|| {
        Error::Usage(format!(
            "unsupported (target, scope) combination for {target:?}"
        ))
    })
}

fn ensure_parent(p: &Path) -> Result<()> {
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }
    }
    Ok(())
}

fn show(target: Option<String>) -> Result<()> {
    let target = match target.as_deref() {
        Some(s) => parse_target(s)?,
        None => Target::ClaudeCode,
    };
    print!("{}", format(target, CONTENT));
    Ok(())
}

fn check() -> Result<()> {
    let cwd = cwd()?;
    let hash = current_hash();
    println!("Embedded skill hash: {hash}");
    for det in detection::detect(&cwd) {
        let label = match det.target {
            Target::ClaudeCode => "claude-code",
            Target::AgentsMd => "agents-md",
            Target::Cursor => "cursor",
        };
        report_path(label, "user", det.user_path.as_deref(), det.target);
        report_path(label, "project", det.project_path.as_deref(), det.target);
    }
    Ok(())
}

fn report_path(label: &str, scope: &str, path: Option<&Path>, target: Target) {
    let Some(path) = path else {
        return;
    };
    if !path.exists() {
        println!("[{label} {scope}] not installed: {}", path.display());
        return;
    }
    match std::fs::read_to_string(path) {
        Ok(installed) => {
            // Compare against the formatted body for this target — that's
            // what `install_to` writes for ClaudeCode. AgentsMd is appended
            // as a section, so a substring check is the right contract.
            let expected = format(target, CONTENT);
            let same = match target {
                Target::ClaudeCode => installed == expected,
                Target::AgentsMd => installed.contains(expected.trim_end()),
                Target::Cursor => installed == expected,
            };
            if same {
                println!("[{label} {scope}] installed (current): {}", path.display());
            } else {
                println!(
                    "[{label} {scope}] installed (DRIFT — re-run install): {}",
                    path.display()
                );
            }
        }
        Err(e) => {
            println!("[{label} {scope}] error reading {}: {e}", path.display());
        }
    }
}
