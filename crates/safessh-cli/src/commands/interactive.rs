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
        .with_prompt("Project name")
        .validate_with(move |s: &String| -> std::result::Result<(), &str> {
            if s.trim().is_empty() {
                return Err("name cannot be empty");
            }
            if !s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err("only alphanumerics, '-' and '_' allowed");
            }
            if existing.iter().any(|n| n == s) {
                return Err("a project with that name already exists");
            }
            Ok(())
        })
        .interact_text()
        .map_err(io_to_err)?;

    let kind_labels = [
        "Use an ssh-config alias from ~/.ssh/config",
        "Define an inline target (host/user/port/key)",
    ];
    let kind = Select::with_theme(&theme)
        .with_prompt("How do you want to define the target?")
        .items(&kind_labels)
        .default(0)
        .interact()
        .map_err(io_to_err)?;

    let target = match kind {
        0 => prompt_ssh_config_target(store, &theme, "default")?,
        _ => prompt_inline_target(&theme, "default")?,
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

    println!("\nProject preview:");
    println!(
        "{}",
        toml::to_string_pretty(&project)
            .unwrap_or_else(|_| "(could not format preview)".to_string())
    );

    let confirm = Confirm::with_theme(&theme)
        .with_prompt("Save?")
        .default(true)
        .interact()
        .map_err(io_to_err)?;
    if !confirm {
        println!("Cancelled. Nothing written.");
        return Ok(());
    }

    store.save(&project)?;
    println!("Created project: {name}");
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

    println!("\nCurrent state of '{}':\n", project.name);
    println!(
        "{}",
        toml::to_string_pretty(&project).unwrap_or_else(|_| String::new())
    );

    loop {
        let actions = [
            "Add a target",
            "Remove a target",
            "Change default target",
            "Edit policy categories",
            "Save and exit",
            "Discard changes and exit",
        ];
        let choice = Select::with_theme(&theme)
            .with_prompt("Action")
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
                println!("Discarded.");
                return Ok(());
            }
        }
    }
}

fn pick_existing_project(store: &ProjectStore, theme: &ColorfulTheme) -> Result<String> {
    let names = store.list().unwrap_or_default();
    if names.is_empty() {
        return Err(Error::Usage(
            "no projects exist yet — run `safessh project add` first".into(),
        ));
    }
    let idx = FuzzySelect::with_theme(theme)
        .with_prompt("Pick a project to edit")
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
            "no Host blocks found in ~/.ssh/config".into(),
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
        "Reference the alias at exec time (lets ~/.ssh/config evolve)",
        "Snapshot the alias values into safessh now (decoupled from ssh-config)",
    ];
    let mode = Select::with_theme(theme)
        .with_prompt("How should safessh use the alias?")
        .items(&mode_labels)
        .default(0)
        .interact()
        .map_err(io_to_err)?;

    let alias_idx = FuzzySelect::with_theme(theme)
        .with_prompt("Pick an ssh-config alias (type to filter)")
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
        .with_prompt("Host (e.g. db.internal)")
        .validate_with(|s: &String| -> std::result::Result<(), &str> {
            if s.trim().is_empty() {
                Err("host cannot be empty")
            } else {
                Ok(())
            }
        })
        .interact_text()
        .map_err(io_to_err)?;

    let user_default = std::env::var("USER").unwrap_or_default();
    let user_prompt = Input::with_theme(theme).with_prompt("User");
    let user: String = if user_default.is_empty() {
        user_prompt.interact_text().map_err(io_to_err)?
    } else {
        user_prompt
            .default(user_default)
            .interact_text()
            .map_err(io_to_err)?
    };

    let port: u16 = Input::with_theme(theme)
        .with_prompt("Port")
        .default(22u16)
        .interact_text()
        .map_err(io_to_err)?;

    let identity_file = if Confirm::with_theme(theme)
        .with_prompt("Specify a private key file?")
        .default(false)
        .interact()
        .map_err(io_to_err)?
    {
        Some(prompt_ssh_key_path(theme)?)
    } else {
        None
    };

    let proxy_jump = if Confirm::with_theme(theme)
        .with_prompt("Use a ProxyJump (bastion)?")
        .default(false)
        .interact()
        .map_err(io_to_err)?
    {
        let s: String = Input::with_theme(theme)
            .with_prompt("ProxyJump (e.g. user@bastion[:port])")
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

/// Pick a private key. Discovers candidates in `~/.ssh/` (skipping `.pub`,
/// `known_hosts`, `config`, `authorized_keys`) and offers a manual-path
/// fallback at the bottom of the list. Tilde paths are expanded.
fn prompt_ssh_key_path(theme: &ColorfulTheme) -> Result<PathBuf> {
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

    let mut items: Vec<String> = candidates.iter().map(|p| display_path(p)).collect();
    items.push("Type a path manually...".to_string());

    let idx = FuzzySelect::with_theme(theme)
        .with_prompt("Private key (type to filter; pick last item to enter a path)")
        .items(&items)
        .default(0)
        .interact()
        .map_err(io_to_err)?;

    if idx == items.len() - 1 {
        let s: String = Input::with_theme(theme)
            .with_prompt("Path to private key")
            .validate_with(|s: &String| -> std::result::Result<(), &str> {
                let expanded = expand_tilde(s);
                if !expanded.exists() {
                    return Err("file does not exist (or you don't have permission to stat it)");
                }
                if !expanded.is_file() {
                    return Err("not a regular file");
                }
                Ok(())
            })
            .interact_text()
            .map_err(io_to_err)?;
        Ok(expand_tilde(&s))
    } else {
        Ok(candidates[idx].clone())
    }
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
        .with_prompt("Target name (unique within project)")
        .validate_with(move |s: &String| -> std::result::Result<(), &str> {
            if s.trim().is_empty() {
                return Err("name cannot be empty");
            }
            if existing.iter().any(|n| n == s) {
                return Err("a target with that name already exists");
            }
            Ok(())
        })
        .interact_text()
        .map_err(io_to_err)?;

    let kind = Select::with_theme(theme)
        .with_prompt("Define how?")
        .items(&["Use an ssh-config alias", "Define an inline target"])
        .default(0)
        .interact()
        .map_err(io_to_err)?;

    let target = if kind == 0 {
        let snap_paths = safessh_storage::paths::Paths::user().map_err(Error::Io)?;
        let snap = SshConfigSnapshot::load(&snap_paths)?;
        if snap.aliases.is_empty() {
            return Err(Error::Config("no Host blocks in ~/.ssh/config".into()));
        }
        let labels: Vec<String> = snap.aliases.iter().map(|a| a.alias.clone()).collect();
        let mode = Select::with_theme(theme)
            .with_prompt("Reference or snapshot?")
            .items(&["Reference at exec time", "Snapshot values into safessh"])
            .default(0)
            .interact()
            .map_err(io_to_err)?;
        let idx = FuzzySelect::with_theme(theme)
            .with_prompt("Pick alias")
            .items(&labels)
            .default(0)
            .interact()
            .map_err(io_to_err)?;
        let entry = &snap.aliases[idx];
        if mode == 0 {
            Target::SshConfigAlias {
                name: target_name,
                ssh_config_alias: entry.alias.clone(),
            }
        } else {
            Target::Inline {
                name: target_name,
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
            }
        }
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
            "project has only one target — add another first or use `project remove`".into(),
        ));
    }
    let names: Vec<String> = project
        .targets
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    let idx = FuzzySelect::with_theme(theme)
        .with_prompt("Remove which target?")
        .items(&names)
        .default(0)
        .interact()
        .map_err(io_to_err)?;
    if names[idx] == project.default_target {
        return Err(Error::Config(
            "cannot remove the default target — change the default first".into(),
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
        .with_prompt("New default target")
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

    let bucket_labels = ["allow", "require_approval", "deny"];
    let bucket = Select::with_theme(theme)
        .with_prompt("Which list to edit?")
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
        .with_prompt("Toggle categories (Space to toggle, Enter to confirm)")
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
