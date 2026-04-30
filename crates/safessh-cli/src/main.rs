//! `safessh` CLI entry point.
//!
//! Task 18 wires only the clap skeleton — each subcommand is a placeholder
//! that prints a stub line. Tasks 19+ wire them to the real backends.

use clap::Parser;

mod cli;
mod commands;
mod errors;
mod output;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let parsed = cli::Cli::parse();

    // The `--yolo` global flag is parsed and available here; Task 23 will
    // consult it (plus the project + global `approvals.yolo` config) to
    // decide whether to bypass approval prompts.
    let _yolo = parsed.yolo;

    match parsed.command {
        cli::TopCmd::External(args) => {
            if let Err(e) = commands::exec::run(args).await {
                errors::report_and_exit(e);
            }
        }
        cli::TopCmd::Project { cmd } => {
            if let Err(e) = commands::project::run(cmd) {
                errors::report_and_exit(e);
            }
        }
        cli::TopCmd::Policy { .. } => println!("(policy not yet wired)"),
        cli::TopCmd::Approve { .. } => println!("(approve not yet wired)"),
        cli::TopCmd::Audit { .. } => println!("(audit not yet wired)"),
        cli::TopCmd::Skill { .. } => println!("(skill not yet wired)"),
        cli::TopCmd::Tui => {
            eprintln!("safessh: tui lands in v0.2");
            std::process::exit(1);
        }
    }
}
