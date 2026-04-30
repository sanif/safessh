//! Integration tests for `safessh approve <token>`.
//!
//! End-to-end coverage (exec creates a pending request, approve consumes
//! it) lives in the `e2e_integration` test in Task 25 — it requires a
//! real project on disk and is gated behind the `integration` feature.

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn approve_unknown_token_fails() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("safessh")
        .unwrap()
        .env("SAFESSH_HOME", dir.path())
        .args(["approve", "nonexistent"])
        .assert()
        .failure()
        .code(2)
        .stderr(contains("no pending"));
}
