//! Integration tests for `safessh project target add/list/remove`.

mod common;
use common::TestEnv;

#[test]
fn target_add_alias_then_list() {
    let env = TestEnv::new();
    env.cmd()
        .args(["project", "add", "p", "--alias", "primary"])
        .assert()
        .success();
    env.cmd()
        .args([
            "project", "target", "add", "p", "--name", "db", "--alias", "db-host",
        ])
        .assert()
        .success();
    let out = env
        .cmd()
        .args(["project", "target", "list", "p"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("default [default]"),
        "expected default marker: {stdout}"
    );
    assert!(
        stdout.contains("db  alias=db-host"),
        "expected db target detail: {stdout}"
    );
}

#[test]
fn target_add_inline_writes_host_user_port() {
    let env = TestEnv::new();
    env.cmd()
        .args(["project", "add", "p", "--alias", "primary"])
        .assert()
        .success();
    env.cmd()
        .args([
            "project", "target", "add", "p", "--name", "web", "--host", "10.0.0.5", "--user",
            "deploy", "--port", "2222",
        ])
        .assert()
        .success();
    let toml = env.read_project("p");
    assert!(toml.contains("name = \"web\""), "{toml}");
    assert!(toml.contains("host = \"10.0.0.5\""), "{toml}");
    assert!(toml.contains("user = \"deploy\""), "{toml}");
    assert!(toml.contains("port = 2222"), "{toml}");
}

#[test]
fn target_add_inline_requires_host_and_user() {
    let env = TestEnv::new();
    env.cmd()
        .args(["project", "add", "p", "--alias", "primary"])
        .assert()
        .success();
    let out = env
        .cmd()
        .args([
            "project", "target", "add", "p", "--name", "x", "--host", "h",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn target_add_duplicate_name_rejected() {
    let env = TestEnv::new();
    env.cmd()
        .args(["project", "add", "p", "--alias", "primary"])
        .assert()
        .success();
    let out = env
        .cmd()
        .args([
            "project", "target", "add", "p", "--name", "default", "--alias", "x",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("already exists"),
        "expected duplicate-name error: {stderr}"
    );
}

#[test]
fn target_remove_default_refused() {
    let env = TestEnv::new();
    env.cmd()
        .args(["project", "add", "p", "--alias", "primary"])
        .assert()
        .success();
    let out = env
        .cmd()
        .args(["project", "target", "remove", "p", "--name", "default"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot remove default"));
}

#[test]
fn target_remove_unknown_errors() {
    let env = TestEnv::new();
    env.cmd()
        .args(["project", "add", "p", "--alias", "primary"])
        .assert()
        .success();
    let out = env
        .cmd()
        .args(["project", "target", "remove", "p", "--name", "ghost"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no such target"),
        "expected unknown-target error: {stderr}"
    );
}

#[test]
fn target_add_then_remove_round_trip() {
    let env = TestEnv::new();
    env.cmd()
        .args(["project", "add", "p", "--alias", "primary"])
        .assert()
        .success();
    env.cmd()
        .args([
            "project", "target", "add", "p", "--name", "db", "--alias", "db-host",
        ])
        .assert()
        .success();
    env.cmd()
        .args(["project", "target", "remove", "p", "--name", "db"])
        .assert()
        .success();
    let toml = env.read_project("p");
    assert!(
        !toml.contains("db-host"),
        "expected db target removed from TOML: {toml}"
    );
}
