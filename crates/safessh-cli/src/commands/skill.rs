//! `safessh skill {install,uninstall,show,check}` — manage skill files in
//! the user's agent frameworks.
//!
//! v0.1 design notes:
//! * `install` / `uninstall` require `--target` to be specified explicitly.
//!   `--target all` walks the supported (target, scope) matrix from spec
//!   §4.3 honoring the user-supplied `--scope`, not detection-based fan-out.
//!   Interactive multi-select is deferred; the `SAFESSH_PROMPT_RESPONSE` env
//!   var path is honored by the prompt module elsewhere but not used here.
//! * `show` formats and prints the canonical content for the requested
//!   target (defaults to `claude-code`).
//! * `check` walks `detection::detect` and reports installed-vs-missing plus
//!   hash drift per framework.

use crate::cli::{SkillCmd, SkillScope, UpdateScope};
use safessh_core::error::{Error, Result};
use safessh_skill::adapters::{format, Target};
use safessh_skill::detection;
use safessh_skill::install::{current_hash, default_path, install_to, uninstall_at, Scope};
use safessh_skill::CONTENT;
use std::path::{Path, PathBuf};

const ALL_TARGETS_HINT: &str = "claude-code, agents-md, cursor, gemini-cli, codex, plain, all";

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
        SkillCmd::Update {
            dry_run,
            target,
            scope,
        } => update(dry_run, target, scope),
    }
}

/// Parse the `--target` string into a `Target`. Returns `Err` for unknown
/// values; the special string `"all"` is handled by callers before reaching
/// this function.
fn parse_target(s: &str) -> Result<Target> {
    match s {
        "claude-code" => Ok(Target::ClaudeCode),
        "agents-md" | "agents.md" | "AGENTS.md" => Ok(Target::AgentsMd),
        "cursor" => Ok(Target::Cursor),
        "gemini-cli" | "gemini" => Ok(Target::GeminiCli),
        "codex" => Ok(Target::Codex),
        "plain" => Ok(Target::Plain),
        other => Err(Error::Usage(format!(
            "unknown skill target: {other} (expected: {ALL_TARGETS_HINT})"
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
        Error::Usage(format!(
            "specify --target <{ALL_TARGETS_HINT}> (interactive prompt is v0.2)"
        ))
    })?;

    let cwd = cwd()?;

    if target_str == "all" {
        return install_all(scope, &cwd);
    }

    let target = parse_target(&target_str)?;
    if matches!(target, Target::Plain) && !matches!(scope, SkillScope::Path) {
        return Err(Error::Usage(
            "--target plain requires --scope path --path <FILE>".into(),
        ));
    }
    let dest = resolve_dest(target, scope, path, &cwd)?;
    ensure_parent(&dest)?;
    install_to(target, &dest)?;
    println!("Installed {target_str}: {}", dest.display());
    Ok(())
}

/// Walk the supported (target, scope) matrix from spec §4.3, honoring the
/// caller's `--scope`. `Plain` is intentionally absent — it is path-only and
/// must be installed explicitly.
fn install_all(scope: SkillScope, cwd: &Path) -> Result<()> {
    if matches!(scope, SkillScope::Path) {
        return Err(Error::Usage(
            "--target all is incompatible with --scope path".into(),
        ));
    }

    let combos: &[(Target, SkillScope, &'static str, &'static str)] = &[
        (Target::ClaudeCode, SkillScope::User, "claude-code", "user"),
        (
            Target::ClaudeCode,
            SkillScope::Project,
            "claude-code",
            "project",
        ),
        (
            Target::AgentsMd,
            SkillScope::Project,
            "agents-md",
            "project",
        ),
        (Target::Cursor, SkillScope::Project, "cursor", "project"),
        (Target::GeminiCli, SkillScope::User, "gemini-cli", "user"),
        (
            Target::GeminiCli,
            SkillScope::Project,
            "gemini-cli",
            "project",
        ),
        (Target::Codex, SkillScope::User, "codex", "user"),
    ];

    let want_user = matches!(scope, SkillScope::User);
    let want_project = matches!(scope, SkillScope::Project);

    let mut installed: Vec<String> = vec![];
    let mut skipped: Vec<String> = vec![];
    for (t, s, label, slabel) in combos {
        let want = match s {
            SkillScope::User => want_user,
            SkillScope::Project => want_project,
            SkillScope::Path => false,
        };
        if !want {
            continue;
        }
        let Some(path) = default_path(*t, map_scope(s.clone()), cwd) else {
            skipped.push(format!("{label} ({slabel}): no default path"));
            continue;
        };
        if let Err(e) = ensure_parent(&path) {
            skipped.push(format!("{label} ({slabel}): {e}"));
            continue;
        }
        if let Err(e) = install_to(*t, &path) {
            skipped.push(format!("{label} ({slabel}): install error: {e}"));
            continue;
        }
        installed.push(format!("{label} ({slabel}): {}", path.display()));
    }

    // Note (stderr) which user/project-only targets we elided so the user
    // sees the trade-off of their --scope choice.
    let scope_label = if want_user { "user" } else { "project" };
    let elided: &[&str] = if want_user {
        // user run: project-only adapters that do not appear in `combos`
        // for User scope.
        &["agents-md", "cursor"]
    } else {
        &["codex"]
    };
    for label in elided {
        eprintln!("safessh: skill: skipping {label}: no {scope_label} install path");
    }

    for line in &installed {
        println!("Installed {line}");
    }
    for line in &skipped {
        eprintln!("safessh: skill: skipping {line}");
    }
    if installed.is_empty() && skipped.is_empty() {
        println!("No agent frameworks detected.");
    }
    Ok(())
}

fn uninstall(target: Option<String>, scope: SkillScope, path: Option<PathBuf>) -> Result<()> {
    let target_str =
        target.ok_or_else(|| Error::Usage(format!("specify --target <{ALL_TARGETS_HINT}>")))?;
    let cwd = cwd()?;
    let target = parse_target(&target_str)?;
    if matches!(target, Target::Plain) && !matches!(scope, SkillScope::Path) {
        return Err(Error::Usage(
            "--target plain requires --scope path --path <FILE>".into(),
        ));
    }
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
            Target::GeminiCli => "gemini-cli",
            Target::Codex => "codex",
            Target::Plain => "plain",
        };
        report_path(label, "user", det.user_path.as_deref(), det.target);
        report_path(label, "project", det.project_path.as_deref(), det.target);
    }
    Ok(())
}

fn update(dry_run: bool, targets: Vec<String>, scope: UpdateScope) -> Result<()> {
    let cwd = cwd()?;
    let want_user = matches!(scope, UpdateScope::User | UpdateScope::Both);
    let want_project = matches!(scope, UpdateScope::Project | UpdateScope::Both);

    let want_targets: Option<Vec<Target>> = if targets.is_empty() {
        None
    } else {
        Some(
            targets
                .iter()
                .map(|s| parse_target(s))
                .collect::<Result<Vec<_>>>()?,
        )
    };

    let combos: &[(Target, SkillScope, &'static str, &'static str)] = &[
        (Target::ClaudeCode, SkillScope::User, "claude-code", "user"),
        (
            Target::ClaudeCode,
            SkillScope::Project,
            "claude-code",
            "project",
        ),
        (
            Target::AgentsMd,
            SkillScope::Project,
            "agents-md",
            "project",
        ),
        (Target::Cursor, SkillScope::Project, "cursor", "project"),
        (Target::GeminiCli, SkillScope::User, "gemini-cli", "user"),
        (
            Target::GeminiCli,
            SkillScope::Project,
            "gemini-cli",
            "project",
        ),
        (Target::Codex, SkillScope::User, "codex", "user"),
    ];

    let mut updated = 0usize;
    let mut considered = 0usize;

    for (target, scope_combo, label, slabel) in combos {
        if let Some(filt) = &want_targets {
            if !filt.contains(target) {
                continue;
            }
        }
        let want = match scope_combo {
            SkillScope::User => want_user,
            SkillScope::Project => want_project,
            SkillScope::Path => false,
        };
        if !want {
            continue;
        }

        let Some(path) = default_path(*target, map_scope(scope_combo.clone()), &cwd) else {
            continue;
        };

        let new_body = format(*target, CONTENT);
        let existing = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue, // not installed
        };

        let section_style = matches!(target, Target::AgentsMd | Target::GeminiCli | Target::Codex);
        let already_installed = if section_style {
            existing.contains("## safessh")
        } else {
            !existing.is_empty()
        };
        if !already_installed {
            continue;
        }
        considered += 1;

        // For section-style files, use a substring check against the
        // canonical body — the same contract `safessh skill check` uses.
        // For file-style targets, we want exact-match.
        let already_current = if section_style {
            existing.contains(new_body.trim_end())
        } else {
            existing == new_body
        };

        // Compute what the file would look like after re-installing.
        // For section-style, replace `## safessh` cleanly via the helper
        // (we can't easily preview without writing); fall back to
        // synthesizing from `strip_section` for diff display.
        let target_text = if section_style {
            let stripped = safessh_skill::sections::strip_section(&existing);
            if stripped.trim().is_empty() {
                new_body.clone()
            } else {
                format!("{}\n\n{}", stripped.trim_end(), new_body)
            }
        } else {
            new_body.clone()
        };

        if dry_run {
            if already_current {
                println!("--- {} (unchanged)", path.display());
            } else {
                print_diff(&path, &existing, &target_text);
            }
            continue;
        }

        if already_current {
            continue;
        }

        if section_style {
            crate::commands::skill_helper::write_section(&path, &new_body)?;
        } else {
            crate::commands::skill_helper::write_file(&path, &new_body)?;
        }
        updated += 1;
        println!("Updated {label} ({slabel}): {}", path.display());
    }

    if !dry_run && considered == 0 {
        eprintln!("safessh: skill: no installed copies found");
    } else if !dry_run && updated == 0 {
        // All copies were already current.
        println!("All installed copies are up to date.");
    }

    Ok(())
}

fn print_diff(path: &Path, current: &str, target: &str) {
    use similar::TextDiff;
    if current == target {
        println!("--- {} (unchanged)", path.display());
        return;
    }
    let diff = TextDiff::from_lines(current, target);
    println!("--- {}", path.display());
    println!("+++ {} (proposed)", path.display());
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            similar::ChangeTag::Delete => "-",
            similar::ChangeTag::Insert => "+",
            similar::ChangeTag::Equal => " ",
        };
        print!("{sign}{change}");
    }
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
                Target::GeminiCli => installed.contains(expected.trim_end()),
                Target::Codex => installed.contains(expected.trim_end()),
                Target::Plain => installed == expected,
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
