//! `safessh project` subcommands: add / list / edit / remove.
//!
//! Wires `crate::cli::ProjectCmd` to `safessh-storage::ProjectStore`.

use crate::cli::{ProjectCmd, TargetCmd};
use safessh_core::error::{Error, Result};
use safessh_storage::paths::Paths;
use safessh_storage::project::{Approvals, OutputCaps, Policy, Project, ProjectStore, Target};

pub fn run(cmd: ProjectCmd) -> Result<()> {
    let paths = Paths::user().map_err(Error::Io)?;
    paths.ensure_dirs().map_err(Error::Io)?;
    let store = ProjectStore::new(paths);

    match cmd {
        ProjectCmd::Add {
            name,
            alias,
            host,
            user,
            port,
            import_ssh_config,
        } => {
            let any_flag =
                alias.is_some() || host.is_some() || user.is_some() || import_ssh_config.is_some();
            // Interactive entrypoint: no positional name AND no driving flags.
            // Bare `safessh project add` is the canonical way to create a
            // project — the flag-based form below stays as a scriptable
            // shortcut (CI, agents, hand-rolled snippets).
            if name.is_none() && !any_flag {
                if !atty::is(atty::Stream::Stdin) {
                    return Err(Error::Usage(
                        "interactive `project add` needs a real terminal — to script it, pass a project name plus `--alias`, `--host`/`--user`, or `--import-ssh-config`"
                            .into(),
                    ));
                }
                return crate::commands::interactive::add(&store);
            }
            let name = name.ok_or_else(|| {
                Error::Usage(
                    "give a project name, or run `safessh project add` with no arguments to use the interactive flow"
                        .into(),
                )
            })?;
            let target = if let Some(alias_name) = import_ssh_config {
                let snap = safessh_storage::ssh_config::SshConfigSnapshot::load(store.paths_ref())?;
                let entry = snap
                    .aliases
                    .iter()
                    .find(|a| a.alias == alias_name)
                    .ok_or_else(|| Error::Config(format!("no ssh-config alias: {alias_name}")))?;
                Target::Inline {
                    name: "default".into(),
                    host: entry.hostname.clone().unwrap_or_else(|| alias_name.clone()),
                    port: entry.port.unwrap_or(22),
                    user: entry
                        .user
                        .clone()
                        .unwrap_or_else(|| std::env::var("USER").unwrap_or_default()),
                    identity_file: entry.identity_file.clone(),
                    proxy_jump: None,
                    keychain_secret: None,
                }
            } else {
                match (alias, host, user) {
                    (Some(a), _, _) => Target::SshConfigAlias {
                        name: "default".into(),
                        ssh_config_alias: a,
                    },
                    (None, Some(h), Some(u)) => Target::Inline {
                        name: "default".into(),
                        host: h,
                        port,
                        user: u,
                        identity_file: None,
                        proxy_jump: None,
                        keychain_secret: None,
                    },
                    _ => {
                        return Err(Error::Usage(
                            "specify --alias OR (--host AND --user) OR --import-ssh-config".into(),
                        ))
                    }
                }
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
            store.save(&project)?;
            println!("Created project '{name}'.");
        }
        ProjectCmd::List => {
            for n in store.list()? {
                println!("{n}");
            }
        }
        ProjectCmd::Edit { name } => {
            // Interactive entrypoint when stdin is a TTY. The legacy
            // "spawn $EDITOR on the raw TOML" flow is still reachable when
            // the env var `SAFESSH_EDIT_RAW` is set — handy for power users
            // who want to bulk-rewrite projects without going through prompts.
            let raw_edit = std::env::var_os("SAFESSH_EDIT_RAW").is_some();
            if !raw_edit {
                if !atty::is(atty::Stream::Stdin) {
                    return Err(Error::Usage(
                        "interactive `project edit` needs a real terminal — set SAFESSH_EDIT_RAW=1 to open the project TOML in $EDITOR instead"
                            .into(),
                    ));
                }
                return crate::commands::interactive::edit(&store, name);
            }
            let name = name.ok_or_else(|| {
                Error::Usage(
                    "with SAFESSH_EDIT_RAW set, please pass the project name (e.g. `safessh project edit prod`)".into(),
                )
            })?;
            store.load(&name)?;
            let paths = Paths::user().map_err(Error::Io)?;
            let file = paths.projects_dir().join(format!("{name}.toml"));
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
            let status = std::process::Command::new(&editor)
                .arg(&file)
                .status()
                .map_err(|e| Error::Usage(format!("spawn editor: {e}")))?;
            if !status.success() {
                return Err(Error::Usage(format!("editor exited with {status}")));
            }
        }
        ProjectCmd::Remove { name } => {
            store.remove(&name)?;
            println!("Removed project: {name}");
        }
        ProjectCmd::Target { cmd } => target_run(&store, cmd)?,
    }
    Ok(())
}

fn target_run(store: &ProjectStore, cmd: TargetCmd) -> Result<()> {
    match cmd {
        TargetCmd::Add {
            project,
            name,
            alias,
            host,
            user,
            port,
            identity,
            proxy_jump,
        } => {
            let mut p = store.load(&project)?;
            if p.targets.iter().any(|t| t.name() == name) {
                return Err(Error::Usage(format!("target already exists: {name}")));
            }
            let target = match (alias, host, user) {
                (Some(a), _, _) => Target::SshConfigAlias {
                    name: name.clone(),
                    ssh_config_alias: a,
                },
                (None, Some(h), Some(u)) => Target::Inline {
                    name: name.clone(),
                    host: h,
                    port,
                    user: u,
                    identity_file: identity,
                    proxy_jump,
                    keychain_secret: None,
                },
                _ => {
                    return Err(Error::Usage(
                        "specify --alias OR (--host AND --user)".into(),
                    ))
                }
            };
            p.targets.push(target);
            store.save(&p)?;
            println!("Added target: {name}");
        }
        TargetCmd::List { project } => {
            let p = store.load(&project)?;
            for t in &p.targets {
                let marker = if t.name() == p.default_target {
                    " [default]"
                } else {
                    ""
                };
                let detail = match t {
                    Target::SshConfigAlias {
                        ssh_config_alias, ..
                    } => format!("alias={ssh_config_alias}"),
                    Target::Inline {
                        host, port, user, ..
                    } => format!("{user}@{host}:{port}"),
                };
                println!("{}{}  {}", t.name(), marker, detail);
            }
        }
        TargetCmd::Remove { project, name } => {
            let mut p = store.load(&project)?;
            if p.default_target == name {
                return Err(Error::Config("cannot remove default target".into()));
            }
            let before = p.targets.len();
            p.targets.retain(|t| t.name() != name);
            if p.targets.len() == before {
                return Err(Error::Config(format!("no such target: {name}")));
            }
            store.save(&p)?;
            println!("Removed target: {name}");
        }
    }
    Ok(())
}
