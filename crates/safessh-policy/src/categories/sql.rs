//! SQL-aware category matchers.
//!
//! Recognises a small set of database CLIs (`psql`, `mysql`, `sqlite3`),
//! extracts the SQL payload from their argv, parses it with `sqlparser-rs`
//! using the [`GenericDialect`], and maps the resulting [`Statement`] variants
//! to safessh policy categories: `db:read`, `db:write`, `destructive:db`.
//!
//! # Conservative on failure
//!
//! Per SAFETY-INVARIANT-1 (default-deny on uncertainty), any SQL we cannot
//! parse or recognise falls back to `db:write`. Callers can then route those
//! through `require_approval` rather than blindly allowing.
//!
//! # Interactive sessions
//!
//! `psql` / `mysql` / `sqlite3` invoked without `-c` / `-e` / a SQL argument
//! (an interactive REPL) currently returns no categories — the policy engine
//! handles those at a higher level (Task 12).
//!
//! # Argv extraction
//!
//! After Task 9's parser runs:
//! - `psql -c "SELECT 1"` → `binary="psql"`, `flags=["-c"]`, `args=["SELECT 1"]`
//! - `mysql -e "SELECT 1"` → `binary="mysql"`, `flags=["-e"]`, `args=["SELECT 1"]`
//! - `sqlite3 db.sqlite "SELECT 1"` → `binary="sqlite3"`, `args=["db.sqlite", "SELECT 1"]`
//!
//! shell-words strips quotes, so the SQL string is recovered cleanly.

use crate::ast::ParsedCommand;
use sqlparser::ast::Statement;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

/// Aggregate every SQL category that matches `cmd`.
///
/// Returns an empty `Vec` when `cmd` is not a recognised SQL CLI invocation
/// or has no SQL payload (e.g. interactive `psql` session).
///
/// On SQL parse failure the function returns `vec!["db:write"]` —
/// default-restrictive: an unknown statement is assumed to mutate state.
pub fn match_sql_categories(cmd: &ParsedCommand) -> Vec<String> {
    let sql = match extract_sql(cmd) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let stmts = match Parser::parse_sql(&GenericDialect {}, &sql) {
        Ok(s) => s,
        // SAFETY-INVARIANT-1: opaque SQL → conservative `db:write`, never empty.
        Err(_) => return vec!["db:write".into()],
    };

    let mut out: Vec<String> = Vec::new();
    for stmt in stmts {
        out.push(classify_statement(&stmt));
    }
    out.sort();
    out.dedup();
    out
}

/// Map a single [`Statement`] to a policy category string.
///
/// `DELETE` is the only variant whose category depends on its content: a
/// `DELETE` with no `WHERE` clause is treated as destructive (it nukes the
/// table contents), while a constrained `DELETE` is ordinary write traffic.
fn classify_statement(stmt: &Statement) -> String {
    match stmt {
        // Reads.
        Statement::Query(_) => "db:read".into(),
        Statement::Explain { .. }
        | Statement::ExplainTable { .. }
        | Statement::ShowTables { .. }
        | Statement::ShowColumns { .. }
        | Statement::ShowVariable { .. }
        | Statement::ShowVariables { .. }
        | Statement::ShowCreate { .. }
        | Statement::ShowFunctions { .. } => "db:read".into(),

        // Writes (non-destructive).
        Statement::Insert { .. } | Statement::Update { .. } => "db:write".into(),
        Statement::CreateTable { .. }
        | Statement::CreateIndex { .. }
        | Statement::CreateView { .. }
        | Statement::CreateSchema { .. }
        | Statement::CreateDatabase { .. }
        | Statement::AlterTable { .. }
        | Statement::AlterIndex { .. }
        | Statement::AlterView { .. } => "db:write".into(),

        // DELETE: destructive only when there's no WHERE clause.
        Statement::Delete { selection, .. } => {
            if selection.is_some() {
                "db:write".into()
            } else {
                "destructive:db".into()
            }
        }

        // Hard destructive.
        Statement::Drop { .. } | Statement::Truncate { .. } => "destructive:db".into(),

        // Anything else: conservative.
        _ => "db:write".into(),
    }
}

/// Pull the SQL string out of a recognised database CLI invocation.
///
/// * `psql -c <SQL>` and `mysql -e <SQL>`: SQL is the first positional arg.
/// * `sqlite3 [db] <SQL>`: SQL is the *last* non-empty positional arg
///   (the optional database path comes first).
///
/// Returns `None` for unrecognised binaries or when the relevant flag/arg
/// is missing (e.g. interactive `psql`).
pub fn extract_sql(cmd: &ParsedCommand) -> Option<String> {
    match cmd.binary.as_str() {
        "psql" => extract_after_flag(cmd, "-c"),
        "mysql" => extract_after_flag(cmd, "-e"),
        "sqlite3" => {
            // `sqlite3 db.sqlite "SELECT 1"` and `sqlite3 "SELECT 1"` should
            // both work. We pick the last non-empty arg, which is the SQL in
            // both forms (the database path, when present, is earlier).
            cmd.args.iter().rev().find(|a| !a.is_empty()).cloned()
        }
        _ => None,
    }
}

/// When `flag` is present, return the first positional arg as the SQL string.
///
/// Task 9's parser puts the bare flag (`-c`, `-e`) in `flags` and the
/// following quoted SQL in `args`, so we just take `args[0]`.
fn extract_after_flag(cmd: &ParsedCommand, flag: &str) -> Option<String> {
    if cmd.flags.iter().any(|f| f == flag) {
        cmd.args.first().cloned()
    } else {
        None
    }
}
