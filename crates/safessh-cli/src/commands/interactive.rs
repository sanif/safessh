//! Interactive `project add` / `project edit` flows.
//!
//! Driven by `dialoguer` 0.11 with the `ColorfulTheme` so prompts pick up
//! terminal colors while remaining accessible in low-color sessions.
//!
//! These helpers are entered only when both:
//!   * the corresponding `safessh project ...` subcommand was invoked
//!     with no positional / flag arguments that would otherwise drive the
//!     non-interactive flow, AND
//!   * `atty::is(atty::Stream::Stdin)` reports a TTY.
//!
//! When stdin is not a TTY (CI, scripted invocations, piped input), the
//! caller in `project::run` falls back to `Error::Usage(...)` rather than
//! prompting — `dialoguer` itself would block on `read_line` against an
//! EOF-yielding pipe, which is a worse failure mode.

use dialoguer::{theme::ColorfulTheme, Confirm, FuzzySelect, Input, Select};
use safessh_core::error::{Error, Result};
use safessh_storage::project::{Approvals, OutputCaps, Policy, Project, ProjectStore, Target};
use safessh_storage::ssh_config::SshConfigSnapshot;
use std::path::{Path, PathBuf};

/// Run the full interactive `project add` flow and persist the result.
pub fn add(store: &ProjectStore) -> Result<()> {
    let theme = ColorfulTheme::default();
    let existing = store.list().unwrap_or_default();

    let name: String = Input::with_theme(&theme)
        .with_prompt("What's the project name?")
        .validate_with(move |s: &String| -> std::result::Result<(), &str> {
            if s.trim().is_empty() {
                return Err("looks empty — try a name");
            }
            if !s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err("use letters, digits, '-' or '_'");
            }
            if existing.iter().any(|n| n == s) {
                return Err("you already have a project with that name");
            }
            Ok(())
        })
        .interact_text()
        .map_err(io_to_err)?;

    let has_alias = Confirm::with_theme(&theme)
        .with_prompt("Do you already have a ~/.ssh/config alias for this host?")
        .default(false)
        .interact()
        .map_err(io_to_err)?;

    let target = if has_alias {
        prompt_ssh_config_target(store, &theme, "default")?
    } else {
        prompt_inline_target(&theme, "default")?
    };

    let project = Project {
        name: name.clone(),
        default_target: "default".into(),
        targets: vec![target],
        policy: Policy {
            allow: vec!["read:safe".into(), "file:read".into()],
            require_approval: vec![],
            deny: vec![],
            file_rules: vec![],
        },
        approvals: Approvals::default(),
        output: OutputCaps::default(),
    };

    println!("\nHere's what we'll save:");
    println!(
        "{}",
        toml::to_string_pretty(&project)
            .unwrap_or_else(|_| "(could not format preview)".to_string())
    );

    let confirm = Confirm::with_theme(&theme)
        .with_prompt("Save this project?")
        .default(true)
        .interact()
        .map_err(io_to_err)?;
    if !confirm {
        println!("Cancelled — nothing saved.");
        return Ok(());
    }

    store.save(&project)?;
    println!("Created project '{name}'.");
    Ok(())
}

/// Run the full interactive `project edit` flow.
pub fn edit(store: &ProjectStore, name_hint: Option<String>) -> Result<()> {
    let theme = ColorfulTheme::default();
    let name = match name_hint {
        Some(n) => n,
        None => pick_existing_project(store, &theme)?,
    };
    let mut project = store.load(&name)?;

    println!("\nHere's what '{}' looks like right now:\n", project.name);
    println!(
        "{}",
        toml::to_string_pretty(&project).unwrap_or_else(|_| String::new())
    );

    loop {
        let actions = [
            "Add a target",
            "Remove a target",
            "Change the default target",
            "Edit policy categories",
            "Save and exit",
            "Discard and exit",
        ];
        let choice = Select::with_theme(&theme)
            .with_prompt("What would you like to do?")
            .items(&actions)
            .default(0)
            .interact()
            .map_err(io_to_err)?;
        match choice {
            0 => add_target_to(&mut project, &theme, store)?,
            1 => remove_target_from(&mut project, &theme)?,
            2 => change_default_target(&mut project, &theme)?,
            3 => edit_policy(&mut project, &theme)?,
            4 => {
                store.save(&project)?;
                println!("Saved.");
                return Ok(());
            }
            _ => {
                println!("Discarded — no changes saved.");
                return Ok(());
            }
        }
    }
}

fn pick_existing_project(store: &ProjectStore, theme: &ColorfulTheme) -> Result<String> {
    let names = store.list().unwrap_or_default();
    if names.is_empty() {
        return Err(Error::Usage(
            "you don't have any projects yet — run `safessh project add` to create one".into(),
        ));
    }
    let idx = FuzzySelect::with_theme(theme)
        .with_prompt("Which project would you like to edit?")
        .items(&names)
        .default(0)
        .interact()
        .map_err(io_to_err)?;
    Ok(names[idx].clone())
}

fn prompt_ssh_config_target(
    store: &ProjectStore,
    theme: &ColorfulTheme,
    target_name: &str,
) -> Result<Target> {
    let snap = SshConfigSnapshot::load(store.paths_ref())?;
    if snap.aliases.is_empty() {
        return Err(Error::Config(
            "couldn't find any Host blocks in ~/.ssh/config".into(),
        ));
    }
    let alias_labels: Vec<String> = snap
        .aliases
        .iter()
        .map(|a| {
            let h = a.hostname.clone().unwrap_or_else(|| a.alias.clone());
            let u = a.user.clone().unwrap_or_default();
            let p = a.port.unwrap_or(22);
            if u.is_empty() {
                format!("{}  →  {}:{p}", a.alias, h)
            } else {
                format!("{}  →  {u}@{h}:{p}", a.alias)
            }
        })
        .collect();

    let mode_labels = [
        "Live link — re-reads ~/.ssh/config on every exec",
        "Snapshot — copies host/user/port now, ignores later edits",
    ];
    let mode = Select::with_theme(theme)
        .with_prompt("How should safessh use this alias?")
        .items(&mode_labels)
        .default(0)
        .interact()
        .map_err(io_to_err)?;

    let alias_idx = FuzzySelect::with_theme(theme)
        .with_prompt("Which alias? (type to filter)")
        .items(&alias_labels)
        .default(0)
        .interact()
        .map_err(io_to_err)?;
    let entry = &snap.aliases[alias_idx];

    if mode == 0 {
        Ok(Target::SshConfigAlias {
            name: target_name.to_string(),
            ssh_config_alias: entry.alias.clone(),
        })
    } else {
        Ok(Target::Inline {
            name: target_name.to_string(),
            host: entry
                .hostname
                .clone()
                .unwrap_or_else(|| entry.alias.clone()),
            port: entry.port.unwrap_or(22),
            user: entry
                .user
                .clone()
                .unwrap_or_else(|| std::env::var("USER").unwrap_or_default()),
            identity_file: entry.identity_file.clone(),
            proxy_jump: None,
            keychain_secret: None,
        })
    }
}

fn prompt_inline_target(theme: &ColorfulTheme, target_name: &str) -> Result<Target> {
    let host: String = Input::with_theme(theme)
        .with_prompt("Hostname (e.g. 10.0.0.x, db.internal, prod-web.example.com)")
        .validate_with(|s: &String| -> std::result::Result<(), &str> {
            if s.trim().is_empty() {
                Err("looks empty — try a hostname")
            } else {
                Ok(())
            }
        })
        .interact_text()
        .map_err(io_to_err)?;

    let user_default = std::env::var("USER").unwrap_or_default();
    let user_prompt = Input::with_theme(theme).with_prompt("Username on the remote");
    let user: String = if user_default.is_empty() {
        user_prompt.interact_text().map_err(io_to_err)?
    } else {
        user_prompt
            .default(user_default)
            .interact_text()
            .map_err(io_to_err)?
    };

    let port: u16 = Input::with_theme(theme)
        .with_prompt("SSH port")
        .default(22u16)
        .interact_text()
        .map_err(io_to_err)?;

    let identity_file = if Confirm::with_theme(theme)
        .with_prompt("Use a private key for this host?")
        .default(false)
        .interact()
        .map_err(io_to_err)?
    {
        Some(prompt_ssh_key_location(theme)?)
    } else {
        None
    };

    let proxy_jump = if Confirm::with_theme(theme)
        .with_prompt("Connect through a bastion (ProxyJump)?")
        .default(false)
        .interact()
        .map_err(io_to_err)?
    {
        let s: String = Input::with_theme(theme)
            .with_prompt("Bastion (e.g. user@bastion or user@bastion:2222)")
            .interact_text()
            .map_err(io_to_err)?;
        Some(s)
    } else {
        None
    };

    Ok(Target::Inline {
        name: target_name.to_string(),
        host,
        port,
        user,
        identity_file,
        proxy_jump,
        keychain_secret: None,
    })
}

/// Sub-menu for the "Use a private key?" yes-branch. Three escape hatches
/// covering the common locations: the conventional `~/.ssh/` directory,
/// arbitrary folders (with a hand-rolled directory navigator), and a typed
/// path. The returned `PathBuf` is always made absolute via `canonicalize`
/// (or, if that fails, by joining `current_dir` with relative input) so the
/// project file keeps working when the user `cd`s elsewhere later.
fn prompt_ssh_key_location(theme: &ColorfulTheme) -> Result<PathBuf> {
    let labels = ["Pick from ~/.ssh/", "Browse another folder", "Paste a path"];
    let choice = Select::with_theme(theme)
        .with_prompt("Where's the key?")
        .items(&labels)
        .default(0)
        .interact()
        .map_err(io_to_err)?;
    let raw = match choice {
        0 => pick_from_ssh_dir(theme)?,
        1 => browse_for_file(theme)?,
        _ => paste_a_path(theme)?,
    };
    Ok(canonicalize_path(&raw))
}

/// Fuzzy-pick a private key from `~/.ssh/`, skipping `.pub`, `known_hosts`,
/// `config`, `authorized_keys`, and dotfiles. If the directory has no
/// candidates the caller gets an `Error::Config` with a hint to try a
/// different sub-menu choice.
fn pick_from_ssh_dir(theme: &ColorfulTheme) -> Result<PathBuf> {
    let home = home_dir();
    let ssh_dir = home.join(".ssh");
    let mut candidates: Vec<PathBuf> = vec![];
    if let Ok(entries) = std::fs::read_dir(&ssh_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.ends_with(".pub")
                || name.starts_with("known_hosts")
                || name == "config"
                || name == "authorized_keys"
                || name.starts_with('.')
            {
                continue;
            }
            candidates.push(p);
        }
    }
    candidates.sort();
    if candidates.is_empty() {
        return Err(Error::Config(
            "no key files in ~/.ssh/ — try \"Browse another folder\" or \"Paste a path\" instead"
                .into(),
        ));
    }
    let labels: Vec<String> = candidates.iter().map(|p| display_path(p)).collect();
    let idx = FuzzySelect::with_theme(theme)
        .with_prompt("Pick a key from ~/.ssh/ (type to filter)")
        .items(&labels)
        .default(0)
        .interact()
        .map_err(io_to_err)?;
    Ok(candidates[idx].clone())
}

/// Hand-rolled directory navigator over `dialoguer::FuzzySelect`. Starts at
/// the current working directory (so a key sitting next to where the user
/// invoked `safessh` is the very first thing on the screen) and lets them
/// step in/out of folders until they pick a regular file. Dotfiles and
/// dot-directories are hidden — paste-a-path is the escape hatch for those.
/// Directories sort before files; entries within each group sort
/// alphabetically.
fn browse_for_file(theme: &ColorfulTheme) -> Result<PathBuf> {
    let mut current = std::env::current_dir().unwrap_or_else(|_| home_dir());
    loop {
        // (label, target_path, is_dir)
        let mut items: Vec<(String, PathBuf, bool)> = Vec::new();
        if let Some(parent) = current.parent() {
            items.push(("../".to_string(), parent.to_path_buf(), true));
        }
        let mut entries: Vec<(String, PathBuf, bool)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&current) {
            for e in rd.flatten() {
                let p = e.path();
                let name = match p.file_name().and_then(|s| s.to_str()) {
                    Some(n) if !n.starts_with('.') => n.to_string(),
                    _ => continue,
                };
                let is_dir = p.is_dir();
                let label = if is_dir { format!("{name}/") } else { name };
                entries.push((label, p, is_dir));
            }
        }
        entries.sort_by(|a, b| match (a.2, b.2) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.cmp(&b.0),
        });
        items.extend(entries);
        if items.is_empty() {
            return Err(Error::Config(format!(
                "couldn't read {} — try \"Paste a path\" instead",
                display_path(&current)
            )));
        }
        let labels: Vec<String> = items.iter().map(|(l, _, _)| l.clone()).collect();
        let prompt = format!("Browse: {}", display_path(&current));
        let idx = FuzzySelect::with_theme(theme)
            .with_prompt(prompt)
            .items(&labels)
            .default(0)
            .interact()
            .map_err(io_to_err)?;
        let (_, picked, is_dir) = items.remove(idx);
        if is_dir {
            current = picked;
            continue;
        }
        return Ok(picked);
    }
}

/// Free-text path with tilde expansion and existence check. Relative paths
/// are resolved against the current working directory so a user sitting in
/// `~/Workspace/cureocity/` can type `cureocity-live.pem` without the leading
/// `./`.
fn paste_a_path(theme: &ColorfulTheme) -> Result<PathBuf> {
    let s: String = Input::with_theme(theme)
        .with_prompt("Path to your key (absolute, relative, or starting with ~)")
        .validate_with(|s: &String| -> std::result::Result<(), &str> {
            let resolved = resolve_input_path(s);
            if !resolved.exists() {
                return Err("can't find that file (or it's not readable)");
            }
            if !resolved.is_file() {
                return Err("that's not a regular file");
            }
            Ok(())
        })
        .interact_text()
        .map_err(io_to_err)?;
    Ok(resolve_input_path(&s))
}

fn resolve_input_path(s: &str) -> PathBuf {
    let expanded = expand_tilde(s);
    if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map(|d| d.join(&expanded))
            .unwrap_or(expanded)
    }
}

fn canonicalize_path(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn add_target_to(
    project: &mut Project,
    theme: &ColorfulTheme,
    _store: &ProjectStore,
) -> Result<()> {
    let existing: Vec<String> = project
        .targets
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    let target_name: String = Input::with_theme(theme)
        .with_prompt("Name for this target?")
        .validate_with(move |s: &String| -> std::result::Result<(), &str> {
            if s.trim().is_empty() {
                return Err("looks empty — try a name");
            }
            if existing.iter().any(|n| n == s) {
                return Err("you already have a target with that name in this project");
            }
            Ok(())
        })
        .interact_text()
        .map_err(io_to_err)?;

    let has_alias = Confirm::with_theme(theme)
        .with_prompt("Do you already have a ~/.ssh/config alias for this host?")
        .default(false)
        .interact()
        .map_err(io_to_err)?;

    let target = if has_alias {
        let project_store = safessh_storage::project::ProjectStore::new(
            safessh_storage::paths::Paths::user().map_err(Error::Io)?,
        );
        prompt_ssh_config_target(&project_store, theme, &target_name)?
    } else {
        prompt_inline_target(theme, &target_name)?
    };

    project.targets.push(target);
    println!("Added.");
    Ok(())
}

fn remove_target_from(project: &mut Project, theme: &ColorfulTheme) -> Result<()> {
    if project.targets.len() <= 1 {
        return Err(Error::Config(
            "this project only has one target — add another first, or use `safessh project remove` to delete the project entirely".into(),
        ));
    }
    let names: Vec<String> = project
        .targets
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    let idx = FuzzySelect::with_theme(theme)
        .with_prompt("Which target should I remove?")
        .items(&names)
        .default(0)
        .interact()
        .map_err(io_to_err)?;
    if names[idx] == project.default_target {
        return Err(Error::Config(
            "that's the default target — change the default first, then remove this one".into(),
        ));
    }
    project.targets.remove(idx);
    println!("Removed.");
    Ok(())
}

fn change_default_target(project: &mut Project, theme: &ColorfulTheme) -> Result<()> {
    let names: Vec<String> = project
        .targets
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    let cur = names
        .iter()
        .position(|n| n == &project.default_target)
        .unwrap_or(0);
    let idx = FuzzySelect::with_theme(theme)
        .with_prompt("Which target should be the new default?")
        .items(&names)
        .default(cur)
        .interact()
        .map_err(io_to_err)?;
    project.default_target = names[idx].clone();
    println!("Default target is now '{}'.", project.default_target);
    Ok(())
}

fn edit_policy(project: &mut Project, theme: &ColorfulTheme) -> Result<()> {
    use dialoguer::MultiSelect;

    const CATEGORIES: &[&str] = &[
        "read:safe",
        "file:read",
        "file:write",
        "destructive:filesystem",
        "destructive:disk",
        "destructive:db",
        "db:read",
        "db:write",
        "privilege:escalation",
        "system:control",
        "network:listen",
        "network:tunnel",
        "exec:opaque",
    ];

    let bucket_labels = [
        "allow — run without prompting",
        "require_approval — prompt before running",
        "deny — refuse outright",
    ];
    let bucket = Select::with_theme(theme)
        .with_prompt("Which list would you like to edit?")
        .items(&bucket_labels)
        .default(0)
        .interact()
        .map_err(io_to_err)?;
    let target = match bucket {
        0 => &mut project.policy.allow,
        1 => &mut project.policy.require_approval,
        _ => &mut project.policy.deny,
    };
    let defaults: Vec<bool> = CATEGORIES
        .iter()
        .map(|c| target.iter().any(|t| t == c))
        .collect();
    let chosen = MultiSelect::with_theme(theme)
        .with_prompt("Pick categories — Space to toggle, Enter to confirm")
        .items(CATEGORIES)
        .defaults(&defaults)
        .interact()
        .map_err(io_to_err)?;
    target.clear();
    for i in chosen {
        target.push(CATEGORIES[i].to_string());
    }
    println!("Updated.");
    Ok(())
}

// --- helpers --------------------------------------------------------------

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    if s == "~" {
        return home_dir();
    }
    PathBuf::from(s)
}

fn display_path(p: &Path) -> String {
    let home = home_dir();
    if let Ok(rel) = p.strip_prefix(&home) {
        format!("~/{}", rel.display())
    } else {
        p.display().to_string()
    }
}

fn io_to_err(e: dialoguer::Error) -> Error {
    match e {
        dialoguer::Error::IO(io) => Error::Io(io),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_replaces_leading_tilde_slash() {
        std::env::set_var("HOME", "/home/test");
        assert_eq!(
            expand_tilde("~/foo/bar"),
            PathBuf::from("/home/test/foo/bar")
        );
    }

    #[test]
    fn expand_tilde_handles_bare_tilde() {
        std::env::set_var("HOME", "/home/test");
        assert_eq!(expand_tilde("~"), PathBuf::from("/home/test"));
    }

    #[test]
    fn expand_tilde_passes_absolute_paths_through() {
        assert_eq!(expand_tilde("/etc/hosts"), PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn display_path_collapses_home_to_tilde() {
        std::env::set_var("HOME", "/home/test");
        assert_eq!(
            display_path(&PathBuf::from("/home/test/.ssh/id_ed25519")),
            "~/.ssh/id_ed25519"
        );
    }
}
