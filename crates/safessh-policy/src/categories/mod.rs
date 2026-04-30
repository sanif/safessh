//! Command category matchers.
//!
//! Two flavours of matcher live under this module:
//!
//! * [`shell`] — looks at the command binary, flags, and args alone.
//! * [`sql`] — recognises a small set of database CLIs and parses their SQL
//!   payload with `sqlparser-rs`.
//!
//! [`match_all`] is the top-level aggregator the decision engine should call:
//! it unions both matcher families' output and produces a sorted, deduped
//! list of categories.

pub mod shell;
pub mod sql;

pub use shell::match_shell_categories;
pub use sql::match_sql_categories;

use crate::ast::ParsedCommand;

/// Return every policy category that applies to `cmd`, sorted and deduped.
///
/// Combines [`match_shell_categories`] and [`match_sql_categories`]. The two
/// families are not mutually exclusive — e.g. a hypothetical
/// `sudo psql -c "DROP TABLE x"` would yield both `privilege:escalation`
/// (shell) and `destructive:db` (SQL) — so the union is the right answer.
pub fn match_all(cmd: &ParsedCommand) -> Vec<String> {
    let mut out = match_shell_categories(cmd);
    out.extend(match_sql_categories(cmd));
    out.sort();
    out.dedup();
    out
}
