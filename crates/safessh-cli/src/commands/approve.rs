//! `safessh approve <token>` — consume a pending approval request and
//! apply the chosen action (once / timed / always / block).
//!
//! The pending file is always removed (via `PendingStore::take`), even on
//! the "once" path: granting once means "let this single retry through",
//! and the user is expected to immediately re-run the original command.

use chrono::{Duration, Utc};
use safessh_core::error::{Error, Result};
use safessh_core::types::ApprovalToken;
use safessh_storage::approvals::{
    AlwaysStore, BlockedStore, PatternRule, PendingStore, TimedRule, TimedStore,
};
use safessh_storage::paths::Paths;

pub fn run(
    token: String,
    timed: bool,
    minutes: Option<u32>,
    always: bool,
    block: bool,
) -> Result<()> {
    let paths = Paths::user().map_err(Error::Io)?;
    paths.ensure_dirs().map_err(Error::Io)?;

    let pending = PendingStore::new(&paths);
    let req = pending.take(&ApprovalToken::from_str(&token))?;

    // Build the pattern from the request's parsed command. `args_pattern`
    // is left None at v0.1 — exact-match approval rules; pattern matching
    // is a v0.2 enhancement.
    let pattern = PatternRule {
        rule_id: format!("rule-{}", Utc::now().timestamp_millis()),
        binary: req.parsed.binary.clone(),
        flags: req.parsed.flags.clone(),
        args_pattern: None,
        categories: req.categories.clone(),
        created_at: Utc::now(),
    };

    // Priority: block > always > timed > once. Mutually exclusive flags
    // would be cleaner at clap level, but this ordering also gives a sane
    // fallback if the user accidentally passes more than one.
    if block {
        BlockedStore::new(&paths).add(&req.project, pattern)?;
        println!("Blocked persistently.");
    } else if always {
        AlwaysStore::new(&paths).add(&req.project, pattern)?;
        println!("Granted always.");
    } else if timed {
        let mins = minutes.unwrap_or(30);
        TimedStore::new(&paths).add(
            &req.project,
            TimedRule {
                pattern,
                expires_at: Utc::now() + Duration::minutes(mins as i64),
            },
        )?;
        println!("Granted for {mins} minutes.");
    } else {
        // Once: pending is already removed via `take`; nothing else to do.
        println!("Granted once.");
    }
    Ok(())
}
