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
