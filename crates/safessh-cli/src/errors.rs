//! Top-level error reporting: maps `safessh-core::Error` to documented exit
//! codes (per spec §7.1) and prints a user-facing message to stderr.

use safessh_core::error::Error;

/// Print a user-facing message and exit with the documented code.
///
/// Tasks 19+ call this from each subcommand handler; in Task 18 the
/// placeholder handlers don't surface `safessh-core::Error` yet, so the
/// function appears unused at this stage.
#[allow(dead_code)]
pub fn report_and_exit(err: Error) -> ! {
    let code = err.exit_code();
    match &err {
        Error::ApprovalRequired { token, categories } => {
            eprintln!("BLOCKED: {} on this project", categories.join(", "));
            eprintln!("Approve via: safessh approve {token}");
            eprintln!("Token: {token}");
        }
        Error::Blocked { rule_id, reason } => {
            eprintln!("safessh: blocked: rule {rule_id}: {reason}");
        }
        _ => eprintln!("safessh: {}: {err}", err.error_class()),
    }
    std::process::exit(code);
}
