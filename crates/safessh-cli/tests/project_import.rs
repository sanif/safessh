//! Integration tests for `safessh project add --import-ssh-config <alias>`.

mod common;
use common::TestEnv;

#[test]
fn import_creates_inline_target_with_alias_values() {
    let env = TestEnv::new();
    env.write_ssh_config(
        "Host prod-host\n  HostName prod.example.com\n  User deploy\n  Port 2222\n",
    );

    env.cmd()
        .args(["project", "add", "prod", "--import-ssh-config", "prod-host"])
        .assert()
        .success();

    let toml = env.read_project("prod");
    assert!(
        toml.contains(r#"host = "prod.example.com""#),
        "expected imported HostName: {toml}"
    );
    assert!(
        toml.contains(r#"user = "deploy""#),
        "expected imported User: {toml}"
    );
    assert!(
        toml.contains("port = 2222"),
        "expected imported Port: {toml}"
    );
}

#[test]
fn import_unknown_alias_exits_1_with_message() {
    let env = TestEnv::new();
    env.write_ssh_config("Host real\n  HostName real.example.com\n");
    let out = env
        .cmd()
        .args(["project", "add", "prod", "--import-ssh-config", "ghost"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no ssh-config alias: ghost"),
        "expected unknown-alias message: {stderr}"
    );
}

#[test]
fn import_conflicts_with_alias_exits_2() {
    let env = TestEnv::new();
    let out = env
        .cmd()
        .args([
            "project",
            "add",
            "prod",
            "--import-ssh-config",
            "x",
            "--alias",
            "y",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected clap conflict to exit 2: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn import_conflicts_with_host_exits_2() {
    let env = TestEnv::new();
    let out = env
        .cmd()
        .args([
            "project",
            "add",
            "prod",
            "--import-ssh-config",
            "x",
            "--host",
            "h",
            "--user",
            "u",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// Aliases lacking `User`/`HostName` fall back to the alias name and
/// `$USER`. Verifies the import doesn't blow up on minimal configs.
#[test]
fn import_minimal_config_falls_back_to_defaults() {
    let env = TestEnv::new();
    env.write_ssh_config("Host minimal\n");
    env.cmd()
        .env("USER", "fallback-user")
        .args(["project", "add", "min", "--import-ssh-config", "minimal"])
        .assert()
        .success();

    let toml = env.read_project("min");
    assert!(
        toml.contains(r#"host = "minimal""#),
        "expected fallback host: {toml}"
    );
    assert!(
        toml.contains(r#"user = "fallback-user""#),
        "expected USER fallback: {toml}"
    );
    assert!(toml.contains("port = 22"), "expected default port: {toml}");
}
