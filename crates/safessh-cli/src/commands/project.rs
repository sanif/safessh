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
        } => {
            let target = match (alias, host, user) {
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
                        "specify --alias OR (--host AND --user)".into(),
                    ))
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
                },
                approvals: Approvals::default(),
                output: OutputCaps::default(),
            };
            store.save(&project)?;
            println!("Created project: {name}");
        }
        ProjectCmd::List => {
            for n in store.list()? {
                println!("{n}");
            }
        }
        ProjectCmd::Edit { name } => {
            // Verify it exists before launching the editor.
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
