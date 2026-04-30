//! Integration tests for the SQL-aware category matchers (Task 11).

use safessh_core::types::ParsedCommand;
use safessh_policy::categories::sql::match_sql_categories;
use safessh_policy::categories::{match_all, match_shell_categories};
use safessh_policy::parser::parse;

/// Parse `s` and return the first (only) [`ParsedCommand`].
fn p(s: &str) -> ParsedCommand {
    parse(s)
        .unwrap_or_else(|e| panic!("parse({s:?}) failed: {e}"))
        .into_iter()
        .next()
        .expect("parser returned an empty vec")
}

// ---- psql ------------------------------------------------------------------

#[test]
fn psql_select_is_db_read() {
    let cats = match_sql_categories(&p(r#"psql -c "SELECT * FROM users""#));
    assert!(
        cats.contains(&"db:read".to_string()),
        "expected db:read in {cats:?}"
    );
}

#[test]
fn psql_insert_is_db_write() {
    let cats = match_sql_categories(&p(r#"psql -c "INSERT INTO x VALUES (1)""#));
    assert!(
        cats.contains(&"db:write".to_string()),
        "expected db:write in {cats:?}"
    );
}

#[test]
fn psql_update_is_db_write() {
    let cats = match_sql_categories(&p(r#"psql -c "UPDATE users SET name='a' WHERE id=1""#));
    assert!(
        cats.contains(&"db:write".to_string()),
        "expected db:write in {cats:?}"
    );
}

#[test]
fn psql_drop_is_destructive_db_only() {
    let cats = match_sql_categories(&p(r#"psql -c "DROP TABLE users""#));
    assert!(
        cats.contains(&"destructive:db".to_string()),
        "expected destructive:db in {cats:?}"
    );
    assert!(
        !cats.contains(&"db:write".to_string()),
        "DROP must not also be classified db:write: {cats:?}"
    );
}

#[test]
fn psql_truncate_is_destructive_db() {
    let cats = match_sql_categories(&p(r#"psql -c "TRUNCATE foo""#));
    assert!(
        cats.contains(&"destructive:db".to_string()),
        "expected destructive:db in {cats:?}"
    );
}

#[test]
fn psql_delete_without_where_is_destructive() {
    let cats = match_sql_categories(&p(r#"psql -c "DELETE FROM users""#));
    assert!(
        cats.contains(&"destructive:db".to_string()),
        "expected destructive:db in {cats:?}"
    );
    assert!(
        !cats.contains(&"db:write".to_string()),
        "unbounded DELETE must not be classified db:write: {cats:?}"
    );
}

#[test]
fn psql_delete_with_where_is_db_write() {
    let cats = match_sql_categories(&p(r#"psql -c "DELETE FROM users WHERE id=1""#));
    assert!(
        cats.contains(&"db:write".to_string()),
        "expected db:write in {cats:?}"
    );
    assert!(
        !cats.contains(&"destructive:db".to_string()),
        "bounded DELETE must not be classified destructive:db: {cats:?}"
    );
}

#[test]
fn psql_without_dash_c_is_empty() {
    // Interactive psql session — not statically classifiable.
    let cats = match_sql_categories(&p("psql"));
    assert!(
        cats.is_empty(),
        "interactive psql should produce no SQL categories, got {cats:?}"
    );
}

#[test]
fn psql_garbage_sql_falls_back_to_db_write() {
    // sqlparser cannot parse this. Default-restrictive: db:write.
    let cats = match_sql_categories(&p(r#"psql -c "this is not valid sql""#));
    assert_eq!(cats, vec!["db:write".to_string()]);
}

// ---- mysql -----------------------------------------------------------------

#[test]
fn mysql_select_is_db_read() {
    let cats = match_sql_categories(&p(r#"mysql -e "SELECT 1""#));
    assert!(
        cats.contains(&"db:read".to_string()),
        "expected db:read in {cats:?}"
    );
}

#[test]
fn mysql_drop_is_destructive_db() {
    let cats = match_sql_categories(&p(r#"mysql -e "DROP TABLE x""#));
    assert!(
        cats.contains(&"destructive:db".to_string()),
        "expected destructive:db in {cats:?}"
    );
}

// ---- sqlite3 ---------------------------------------------------------------

#[test]
fn sqlite3_select_is_db_read() {
    let cats = match_sql_categories(&p(r#"sqlite3 my.db "SELECT * FROM t""#));
    assert!(
        cats.contains(&"db:read".to_string()),
        "expected db:read in {cats:?}"
    );
}

#[test]
fn sqlite3_drop_is_destructive_db() {
    let cats = match_sql_categories(&p(r#"sqlite3 my.db "DROP TABLE t""#));
    assert!(
        cats.contains(&"destructive:db".to_string()),
        "expected destructive:db in {cats:?}"
    );
}

// ---- non-SQL passthrough ---------------------------------------------------

#[test]
fn non_sql_binary_returns_empty() {
    let cats = match_sql_categories(&p("ls -la"));
    assert!(
        cats.is_empty(),
        "ls should produce no SQL categories, got {cats:?}"
    );
}

// ---- match_all aggregator --------------------------------------------------

#[test]
fn match_all_combines_shell_and_sql() {
    // psql is excluded from is_exec_opaque (KNOWN_SEMANTIC_INTERPRETERS), so
    // shell categories are empty here and SQL provides db:read.
    let cmd = p(r#"psql -c "SELECT 1""#);
    let shell = match_shell_categories(&cmd);
    let sql = match_sql_categories(&cmd);
    let all = match_all(&cmd);

    // Every shell + SQL category must appear in the union.
    for cat in &shell {
        assert!(
            all.contains(cat),
            "match_all missing shell cat {cat:?}: {all:?}"
        );
    }
    for cat in &sql {
        assert!(
            all.contains(cat),
            "match_all missing sql cat {cat:?}: {all:?}"
        );
    }

    // Sorted + deduped.
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(all, sorted, "match_all output must be sorted and deduped");
}

#[test]
fn match_all_pure_shell_command() {
    // Plain shell command — only shell categories should appear.
    let cmd = p("rm -rf /tmp/foo");
    let all = match_all(&cmd);
    assert!(all.contains(&"destructive:filesystem".to_string()));
    // No SQL category because rm isn't a recognised SQL CLI.
    assert!(!all.contains(&"db:read".to_string()));
    assert!(!all.contains(&"db:write".to_string()));
    assert!(!all.contains(&"destructive:db".to_string()));
}

#[test]
fn match_all_is_sorted_and_deduped() {
    let cmd = p(r#"psql -c "DROP TABLE x""#);
    let all = match_all(&cmd);
    let mut expected = all.clone();
    expected.sort();
    expected.dedup();
    assert_eq!(all, expected);
}
