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
        cli::TopCmd::External(args) => {
            if let Err(e) = commands::exec::run(args, yolo).await {
                errors::report_and_exit(e);
            }
        }
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
        cli::TopCmd::Tui => {
            eprintln!("safessh: tui lands in v0.2");
            std::process::exit(1);
        }
    }
}
