//! Tests for verb dispatch in `safessh <project> <verb> ...`.
//!
//! Verifies that:
//! 1. `read` routes to `commands::read::run` (exit 0 / exit 30 on truncation).
//! 2. `write` routes to `commands::write::run` (exit 0 / exit 30 on truncation).
//! 3. `exec` still works unchanged (exit 10 on a blocked command).
//! 4. An unknown verb returns `Error::Usage` with the expected message (exit 2).
//!
//! Tests use `assert_cmd` to drive the compiled binary and inject
//! `SAFESSH_HOME` pointing at a temp directory with a project fixture,
//! exactly as `exec.rs` does.

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Set up a temp HOME with a `prod` project that allows `file:read` and
/// `file:write`, and two targets: `default` and `db`.
fn setup_prod() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("config/projects");
    fs::create_dir_all(&projects).unwrap();
    fs::write(
        projects.join("prod.toml"),
        r#"
name = "prod"
default_target = "default"

[[targets]]
name = "default"
ssh_config_alias = "definitely-not-a-real-host-zzz"

[policy]
allow = ["file:read", "file:write"]
require_approval = []
deny = []
"#,
    )
    .unwrap();
    dir
}

/// Set up a temp HOME with a `demo` project for the exec smoke test:
/// read:safe commands are allowed, destructive:filesystem requires approval.
fn setup_demo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("config/projects");
    fs::create_dir_all(&projects).unwrap();
    fs::write(
        projects.join("demo.toml"),
        r#"
name = "demo"
default_target = "default"

[[targets]]
name = "default"
ssh_config_alias = "definitely-not-a-real-host-zzz"

[policy]
allow = ["read:safe"]
require_approval = ["destructive:filesystem"]

[approvals]
timed_default_minutes = 30
"#,
    )
    .unwrap();
    dir
}

// ---------------------------------------------------------------------------
// Test 1: `read` verb routes to commands::read::run
//
// The policy allows file:read, so the binary will attempt to connect via sftp
// to the (fake) ssh alias. Since no SSH daemon is running, the driver returns
// an Ssh error. We only need to verify that the binary did NOT emit a Usage
// error (exit 2 with "expected: exec | read | write"), confirming dispatch
// reached the read handler.
// ---------------------------------------------------------------------------

#[test]
fn read_verb_routes_to_read_handler() {
    let dir = setup_prod();
    let out = Command::cargo_bin("safessh")
        .unwrap()
        .env("SAFESSH_HOME", dir.path())
        .args(["prod", "read", "/etc/hostname"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    // Must NOT have been rejected as an unknown verb (exit 2).
    assert_ne!(
        out.status.code(),
        Some(2),
        "read verb should not produce a Usage error; stderr: {stderr}"
    );
    // Must NOT emit the unknown-verb message.
    assert!(
        !stderr.contains("expected: exec | read | write"),
        "read verb should not hit the unknown-verb arm; stderr: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: `write` verb routes to commands::write::run
//
// Same approach as the read test: policy allows file:write, the binary will
// try sftp but fail on the fake host. We assert it is NOT rejected at the
// dispatch layer (no exit 2 / unknown-verb message).
// ---------------------------------------------------------------------------

#[test]
fn write_verb_routes_to_write_handler() {
    let dir = setup_prod();
    let out = Command::cargo_bin("safessh")
        .unwrap()
        .env("SAFESSH_HOME", dir.path())
        .args(["prod", "write", "/tmp/x"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_ne!(
        out.status.code(),
        Some(2),
        "write verb should not produce a Usage error; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("expected: exec | read | write"),
        "write verb should not hit the unknown-verb arm; stderr: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: `exec` still works unchanged — a blocked destructive command exits 10
// ---------------------------------------------------------------------------

#[test]
fn exec_verb_still_works() {
    let dir = setup_demo();
    Command::cargo_bin("safessh")
        .unwrap()
        .env("SAFESSH_HOME", dir.path())
        .args(["demo", "exec", "rm -rf /var/log"])
        .assert()
        .code(10)
        .stderr(contains("BLOCKED:"))
        .stderr(contains("Token:"));
}

// ---------------------------------------------------------------------------
// Test 4: unknown verb → exit 2 with "expected: exec | read | write"
// ---------------------------------------------------------------------------

#[test]
fn unknown_verb_returns_usage_error() {
    let dir = setup_prod();
    Command::cargo_bin("safessh")
        .unwrap()
        .env("SAFESSH_HOME", dir.path())
        .args(["prod", "nope"])
        .assert()
        .code(2)
        .stderr(contains("expected: exec | read | write"));
}
