//! Integration tests for `safessh project` subcommands.
//!
//! Each test holds one `tempfile::TempDir` for its lifetime and reuses it
//! across multiple `safessh` subprocess invocations (via `SAFESSH_HOME`)
//! so the round-trip writes and reads share the same on-disk state.

use assert_cmd::Command;
use predicates::str::contains;

fn safessh(home: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("safessh").unwrap();
    c.env("SAFESSH_HOME", home);
    // Set EDITOR=true so `project edit` is a no-op that exits 0.
    c.env("EDITOR", "true");
    c
}

#[test]
fn add_with_alias_then_list() {
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["project", "add", "prod", "--alias", "prod-host"])
        .assert()
        .success()
        .stdout(contains("Created project 'prod'."));

    safessh(dir.path())
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(contains("prod"));

    // Verify the file exists with the expected SshConfigAlias target.
    let toml =
        std::fs::read_to_string(dir.path().join("config").join("projects").join("prod.toml"))
            .unwrap();
    assert!(toml.contains("ssh_config_alias = \"prod-host\""), "{toml}");
}

#[test]
fn add_with_inline_target_writes_host_user_port() {
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args([
            "project",
            "add",
            "stage",
            "--host",
            "h.example",
            "--user",
            "u",
            "--port",
            "2222",
        ])
        .assert()
        .success()
        .stdout(contains("Created project 'stage'."));

    let toml = std::fs::read_to_string(
        dir.path()
            .join("config")
            .join("projects")
            .join("stage.toml"),
    )
    .unwrap();
    assert!(toml.contains("host = \"h.example\""), "{toml}");
    assert!(toml.contains("user = \"u\""), "{toml}");
    assert!(toml.contains("port = 2222"), "{toml}");
}

#[test]
fn add_without_target_info_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["project", "add", "incomplete"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn add_with_only_host_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["project", "add", "incomplete", "--host", "h.example"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn list_sorted_alphabetically() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["zeta", "alpha", "mike"] {
        safessh(dir.path())
            .args(["project", "add", name, "--alias", "x"])
            .assert()
            .success();
    }

    let output = safessh(dir.path())
        .args(["project", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, vec!["alpha", "mike", "zeta"]);
}

#[test]
fn remove_existing_project() {
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["project", "add", "prod", "--alias", "prod-host"])
        .assert()
        .success();

    safessh(dir.path())
        .args(["project", "remove", "prod"])
        .assert()
        .success()
        .stdout(contains("Removed project: prod"));

    let out = safessh(dir.path())
        .args(["project", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(!stdout.contains("prod"), "expected prod removed: {stdout}");
}

#[test]
fn remove_nonexistent_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["project", "remove", "ghost"])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn edit_raw_runs_editor_no_op_with_true() {
    // The default `project edit` flow is interactive (dialoguer prompts);
    // assert_cmd has no TTY so we exercise the legacy raw-edit path via
    // SAFESSH_EDIT_RAW=1, which spawns $EDITOR (set to `true` in safessh()).
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["project", "add", "prod", "--alias", "prod-host"])
        .assert()
        .success();

    safessh(dir.path())
        .env("SAFESSH_EDIT_RAW", "1")
        .args(["project", "edit", "prod"])
        .assert()
        .success();
}

#[test]
fn edit_raw_nonexistent_fails() {
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .env("SAFESSH_EDIT_RAW", "1")
        .args(["project", "edit", "ghost"])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn edit_without_tty_refuses_with_usage_error() {
    // When neither SAFESSH_EDIT_RAW is set nor stdin is a TTY (assert_cmd
    // case), the default flow refuses rather than blocking on a prompt.
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["project", "edit", "any"])
        .assert()
        .failure()
        .code(2)
        .stderr(contains("interactive `project edit` needs a real terminal"));
}

#[test]
fn add_without_args_or_tty_refuses_with_usage_error() {
    // Bare `safessh project add` with no flags AND no TTY (assert_cmd) →
    // Error::Usage, exit 2. With a TTY this would have started prompting.
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["project", "add"])
        .assert()
        .failure()
        .code(2)
        .stderr(contains("interactive `project add` needs a real terminal"));
}
