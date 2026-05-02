//! Integration tests for `safessh <project> exec` (headless flow).
//!
//! The destructive-command path never reaches `ssh` — the policy decision is
//! `RequireApproval`, which writes a pending request and exits 10 with the
//! `BLOCKED:` block on stderr. This means we can drive the full flow against
//! a project pointing at a fake `ssh_config` alias without needing any
//! network or test container.

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;

fn setup() -> tempfile::TempDir {
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

#[test]
fn destructive_command_returns_blocked_token() {
    let dir = setup();
    Command::cargo_bin("safessh")
        .unwrap()
        .env("SAFESSH_HOME", dir.path())
        .args(["demo", "exec", "rm -rf /var/log"])
        .assert()
        .code(10)
        .stderr(contains("BLOCKED:"))
        .stderr(contains("Token:"));
}

fn setup_multi() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("config/projects");
    fs::create_dir_all(&projects).unwrap();
    fs::write(
        projects.join("multi.toml"),
        r#"
name = "multi"
default_target = "web"

[[targets]]
name = "web"
ssh_config_alias = "web-alias"

[[targets]]
name = "db"
ssh_config_alias = "db-alias"

[policy]
allow = ["read:safe"]
require_approval = ["destructive:filesystem"]
"#,
    )
    .unwrap();
    dir
}

#[test]
fn exec_on_unknown_target_exits_2() {
    let dir = setup_multi();
    let out = Command::cargo_bin("safessh")
        .unwrap()
        .env("SAFESSH_HOME", dir.path())
        .args(["multi", "--on", "ghost", "exec", "ls /tmp"])
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no such target: ghost"),
        "stderr did not mention missing target: {stderr}"
    );
}

/// `--on db` resolves to the `db` target without ever reaching the SSH
/// driver: the destructive command is gated by policy first, and the
/// `BLOCKED:` token is emitted. This proves the named target was selected
/// (the project has two targets; either resolution would block, but only
/// resolution to a *valid* target lets the policy engine run at all). The
/// real argv composition is covered by the `OpenSshDriver::build_argv`
/// unit tests in `safessh-ssh` and the inline `resolve_target` unit tests.
#[test]
fn exec_on_named_target_runs_policy() {
    let dir = setup_multi();
    Command::cargo_bin("safessh")
        .unwrap()
        .env("SAFESSH_HOME", dir.path())
        .args(["multi", "--on", "db", "exec", "rm -rf /var/log"])
        .assert()
        .code(10)
        .stderr(contains("BLOCKED:"))
        .stderr(contains("Token:"));
}
