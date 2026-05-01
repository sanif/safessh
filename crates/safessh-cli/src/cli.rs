//! CLI top-level command structure (clap-derived).
//!
//! Subcommand bodies are placeholders in Task 18; later tasks wire them
//! to `safessh-storage`, `safessh-policy`, `safessh-audit`, etc.

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "safessh", version, about = "Personal SSH proxy for LLM agents")]
pub struct Cli {
    /// Skip the policy engine entirely and run the SSH command directly.
    /// Refused (exit 13) when `disable_yolo = true` in the global config.
    /// Audited as `yolo_invocation`. Output cap and redactor still apply.
    #[arg(long, global = true)]
    pub yolo: bool,

    #[command(subcommand)]
    pub command: TopCmd,
}

#[derive(Subcommand, Debug)]
pub enum TopCmd {
    /// Run command on a project (default subcommand when first arg is a project name).
    /// Captured via `external_subcommand` so argv passes through verbatim,
    /// e.g. `safessh prod exec "ls -la"`.
    #[command(external_subcommand)]
    External(Vec<String>),

    /// Manage projects
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },

    /// Inspect policies
    Policy {
        #[command(subcommand)]
        cmd: PolicyCmd,
    },

    /// Approve a pending request by token
    Approve {
        token: String,
        #[arg(long)]
        timed: bool,
        #[arg(long)]
        minutes: Option<u32>,
        #[arg(long)]
        always: bool,
        #[arg(long)]
        block: bool,
    },

    /// Inspect audit log
    Audit {
        #[command(subcommand)]
        cmd: AuditCmd,
    },

    /// Install / inspect agent skills
    Skill {
        #[command(subcommand)]
        cmd: SkillCmd,
    },

    /// Launch the TUI (v0.2 - placeholder in v0.1)
    Tui,
}

#[derive(Subcommand, Debug)]
pub enum ProjectCmd {
    Add {
        name: String,
        #[arg(long)]
        alias: Option<String>,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long, default_value_t = 22)]
        port: u16,
        /// Materialize the new project's first target by importing values
        /// from `~/.ssh/config` (or `$SSH_CONFIG_PATH`). The matching `Host`
        /// block's `HostName`/`User`/`Port`/`IdentityFile` populate an
        /// `Inline` target. `ProxyJump` is *not* imported (ssh2-config 0.3
        /// does not expose it); use `--alias` to defer to ssh-config at
        /// exec time when ProxyJump is required.
        #[arg(long, conflicts_with_all = ["alias", "host", "user"])]
        import_ssh_config: Option<String>,
    },
    List,
    Edit {
        name: String,
    },
    Remove {
        name: String,
    },
    /// Manage the target list of an existing project.
    Target {
        #[command(subcommand)]
        cmd: TargetCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum TargetCmd {
    /// Append a new target to a project. Either `--alias` (ssh-config
    /// reference) or both `--host` and `--user` (inline target) must be
    /// supplied; mixing the two forms is rejected.
    Add {
        /// Name of the project to add the target to.
        project: String,
        /// Name for the new target (must be unique within the project).
        #[arg(long)]
        name: String,
        #[arg(long)]
        alias: Option<String>,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long, default_value_t = 22)]
        port: u16,
        #[arg(long)]
        identity: Option<std::path::PathBuf>,
        #[arg(long)]
        proxy_jump: Option<String>,
    },
    /// List targets for a project, marking the default with `[default]`.
    List { project: String },
    /// Remove a target by name. Refuses to remove the project's
    /// `default_target` (re-point it first via `project edit`).
    Remove {
        project: String,
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum PolicyCmd {
    Show { what: String },
}

#[derive(Subcommand, Debug)]
pub enum AuditCmd {
    Query {
        #[arg(long)]
        project: Option<String>,
        #[arg(long, value_name = "EVENT_TYPE")]
        r#type: Option<String>,
        #[arg(long, value_name = "PATTERN")]
        grep: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum SkillCmd {
    Install {
        #[arg(long)]
        target: Option<String>,
        #[arg(long, value_enum, default_value_t = SkillScope::User)]
        scope: SkillScope,
        #[arg(long)]
        path: Option<std::path::PathBuf>,
    },
    Uninstall {
        #[arg(long)]
        target: Option<String>,
        #[arg(long, value_enum, default_value_t = SkillScope::User)]
        scope: SkillScope,
        #[arg(long)]
        path: Option<std::path::PathBuf>,
    },
    Show {
        #[arg(long)]
        target: Option<String>,
    },
    Check,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum SkillScope {
    User,
    Project,
    Path,
}
