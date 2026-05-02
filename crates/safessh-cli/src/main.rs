//! `safessh` CLI entry point.
//!
//! Task 18 wires only the clap skeleton — each subcommand is a placeholder
//! that prints a stub line. Tasks 19+ wire them to the real backends.

use clap::Parser;

mod cli;
mod commands;
mod errors;
mod output;
mod prompt;
mod supervisor;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let parsed = cli::Cli::parse();

    // `--yolo` is a top-level global flag (clap `global = true`), so it's
    // available regardless of subcommand position: both `safessh --yolo X exec
    // ...` and `safessh X exec --yolo ...` end up here with `parsed.yolo == true`.
    let yolo = parsed.yolo;

    match parsed.command {
        cli::TopCmd::External(args) => match find_verb(&args).map(|s| s.as_str()) {
            Some("exec") => {
                if let Err(e) = commands::exec::run(args, yolo).await {
                    errors::report_and_exit(e);
                }
            }
            Some("read") => match commands::read::run(args, yolo).await {
                Ok(truncated) => {
                    if truncated {
                        std::process::exit(30);
                    }
                }
                Err(e) => errors::report_and_exit(e),
            },
            Some("write") => match commands::write::run(args, yolo).await {
                Ok(truncated) => {
                    if truncated {
                        std::process::exit(30);
                    }
                }
                Err(e) => errors::report_and_exit(e),
            },
            Some("forward") => {
                if let Err(e) = commands::forward::run(args, yolo).await {
                    errors::report_and_exit(e);
                }
            }
            _ => {
                errors::report_and_exit(safessh_core::error::Error::Usage(
                    "expected: exec | read | write | forward".into(),
                ));
            }
        },
        cli::TopCmd::Project { cmd } => {
            if let Err(e) = commands::project::run(cmd) {
                errors::report_and_exit(e);
            }
        }
        cli::TopCmd::Policy { cmd } => {
            let cli::PolicyCmd::Show { what } = cmd;
            if let Err(e) = commands::policy::run(what) {
                errors::report_and_exit(e);
            }
        }
        cli::TopCmd::Approve {
            token,
            timed,
            minutes,
            always,
            block,
        } => {
            if let Err(e) = commands::approve::run(token, timed, minutes, always, block) {
                errors::report_and_exit(e);
            }
        }
        cli::TopCmd::Audit { cmd } => {
            let cli::AuditCmd::Query {
                project,
                r#type,
                grep,
            } = cmd;
            if let Err(e) = commands::audit::run(project, r#type, grep) {
                errors::report_and_exit(e);
            }
        }
        cli::TopCmd::Skill { cmd } => {
            if let Err(e) = commands::skill::run(cmd) {
                errors::report_and_exit(e);
            }
        }
        cli::TopCmd::TunnelSupervisor { record_path } => {
            if let Err(e) = commands::forward::run_supervisor(record_path).await {
                errors::report_and_exit(e);
            }
        }
        cli::TopCmd::Tui => {
            // The TUI needs a real terminal — refuse if stdin/stdout
            // is being piped (e.g. CI logs). Exit code matches what
            // we'd return for any other usage error.
            if !atty::is(atty::Stream::Stdin) || !atty::is(atty::Stream::Stdout) {
                eprintln!("safessh: error: tui requires a TTY");
                std::process::exit(1);
            }
            let paths = match safessh_storage::paths::Paths::user() {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("safessh: io: {e}");
                    std::process::exit(40);
                }
            };
            if let Err(e) = paths.ensure_dirs() {
                eprintln!("safessh: io: {e}");
                std::process::exit(40);
            }
            if let Err(e) = safessh_tui::run(paths).await {
                errors::report_and_exit(e);
            }
        }
    }
}

/// Scan `args` for the verb (exec | read | write), skipping `--yolo` and
/// `--on <target>` flags that may appear before the verb in the argv.
///
/// `args[0]` is always the project name; the verb is the next positional arg
/// after stripping any leading flags. Returns `None` if no verb is found.
fn find_verb(args: &[String]) -> Option<&String> {
    let mut iter = args.iter().skip(1); // skip the project name
    while let Some(a) = iter.next() {
        if a == "--yolo" {
            continue;
        }
        if a == "--on" {
            iter.next(); // consume the target value
            continue;
        }
        if a.starts_with("--on=") {
            continue;
        }
        return Some(a);
    }
    None
}
