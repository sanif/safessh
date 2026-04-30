//! Task 24 acceptance tests: `policy show`, `audit query`, and `skill`
//! subcommands.

use assert_cmd::Command;
use predicates::str::contains;

fn safessh(home: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("safessh").unwrap();
    c.env("SAFESSH_HOME", home);
    c
}

// ---------- skill ----------

#[test]
fn skill_show_claude_code() {
    Command::cargo_bin("safessh")
        .unwrap()
        .args(["skill", "show", "--target", "claude-code"])
        .assert()
        .success()
        .stdout(contains("name: safessh"));
}

#[test]
fn skill_show_default_target_is_claude_code() {
    Command::cargo_bin("safessh")
        .unwrap()
        .args(["skill", "show"])
        .assert()
        .success()
        .stdout(contains("name: safessh"));
}

#[test]
fn skill_show_agents_md_has_section_header() {
    Command::cargo_bin("safessh")
        .unwrap()
        .args(["skill", "show", "--target", "agents-md"])
        .assert()
        .success()
        .stdout(contains("## safessh"));
}

#[test]
fn skill_show_unknown_target_exits_2() {
    Command::cargo_bin("safessh")
        .unwrap()
        .args(["skill", "show", "--target", "nope"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn skill_install_claude_code_path_writes_file() {
    let dir = tempfile::tempdir().unwrap();
    let target_dir = dir.path().join("dest");
    std::fs::create_dir_all(&target_dir).unwrap();

    Command::cargo_bin("safessh")
        .unwrap()
        .args([
            "skill",
            "install",
            "--target",
            "claude-code",
            "--scope",
            "path",
            "--path",
        ])
        .arg(&target_dir)
        .assert()
        .success()
        .stdout(contains("Installed"));

    let written = std::fs::read_to_string(target_dir.join("safessh.md")).unwrap();
    assert!(written.contains("name: safessh"), "{written}");
}

#[test]
fn skill_install_agents_md_project_appends_section() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();

    Command::cargo_bin("safessh")
        .unwrap()
        .current_dir(cwd)
        .args([
            "skill",
            "install",
            "--target",
            "agents-md",
            "--scope",
            "project",
        ])
        .assert()
        .success();

    let agents = std::fs::read_to_string(cwd.join("AGENTS.md")).unwrap();
    assert!(agents.contains("## safessh"), "{agents}");
}

#[test]
fn skill_install_without_target_exits_2() {
    Command::cargo_bin("safessh")
        .unwrap()
        .args(["skill", "install"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn skill_uninstall_claude_code_path() {
    let dir = tempfile::tempdir().unwrap();
    let target_dir = dir.path().join("dest");
    std::fs::create_dir_all(&target_dir).unwrap();

    // Install, then uninstall.
    Command::cargo_bin("safessh")
        .unwrap()
        .args([
            "skill",
            "install",
            "--target",
            "claude-code",
            "--scope",
            "path",
            "--path",
        ])
        .arg(&target_dir)
        .assert()
        .success();
    assert!(target_dir.join("safessh.md").exists());

    Command::cargo_bin("safessh")
        .unwrap()
        .args([
            "skill",
            "uninstall",
            "--target",
            "claude-code",
            "--scope",
            "path",
            "--path",
        ])
        .arg(&target_dir)
        .assert()
        .success();

    assert!(!target_dir.join("safessh.md").exists());
}

#[test]
fn skill_check_runs_without_error() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("safessh")
        .unwrap()
        .current_dir(dir.path())
        .args(["skill", "check"])
        .assert()
        .success();
}

// ---------- policy show ----------

#[test]
fn policy_show_read_safe() {
    Command::cargo_bin("safessh")
        .unwrap()
        .args(["policy", "show", "read:safe"])
        .assert()
        .success()
        .stdout(contains("read:safe"))
        .stdout(contains("ls"))
        .stdout(contains("cat"));
}

#[test]
fn policy_show_destructive_filesystem() {
    Command::cargo_bin("safessh")
        .unwrap()
        .args(["policy", "show", "destructive:filesystem"])
        .assert()
        .success()
        .stdout(contains("rm"));
}

#[test]
fn policy_show_destructive_db_is_sql_aware() {
    Command::cargo_bin("safessh")
        .unwrap()
        .args(["policy", "show", "destructive:db"])
        .assert()
        .success()
        .stdout(contains("SQL-aware"));
}

#[test]
fn policy_show_unknown_exits_2() {
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["policy", "show", "nonsense:cat"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn policy_show_project_prints_policy() {
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["project", "add", "prod", "--alias", "prod-host"])
        .assert()
        .success();
    safessh(dir.path())
        .args(["policy", "show", "prod"])
        .assert()
        .success()
        .stdout(contains("Project: prod"))
        .stdout(contains("read:safe"));
}

// ---------- audit query ----------

#[test]
fn audit_query_missing_log_is_empty_success() {
    let dir = tempfile::tempdir().unwrap();
    safessh(dir.path())
        .args(["audit", "query"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn audit_query_filters_by_project_and_type() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let log = state.join("audit.log");
    let l1 = r#"{"project":"prod","event_type":"exec_attempt","msg":"a"}"#;
    let l2 = r#"{"project":"stage","event_type":"exec_attempt","msg":"b"}"#;
    let l3 = r#"{"project":"prod","event_type":"approval_grant","msg":"c"}"#;
    std::fs::write(&log, format!("{l1}\n{l2}\n{l3}\n")).unwrap();

    let out = safessh(dir.path())
        .args([
            "audit",
            "query",
            "--project",
            "prod",
            "--type",
            "exec_attempt",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(stdout.contains(l1), "expected l1 in: {stdout}");
    assert!(!stdout.contains(l2), "should not contain l2: {stdout}");
    assert!(!stdout.contains(l3), "should not contain l3: {stdout}");
}

#[test]
fn audit_query_grep_substring() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let log = state.join("audit.log");
    let l1 = r#"{"project":"prod","event_type":"exec_attempt","msg":"hello-world"}"#;
    let l2 = r#"{"project":"prod","event_type":"exec_attempt","msg":"goodbye"}"#;
    std::fs::write(&log, format!("{l1}\n{l2}\n")).unwrap();

    let out = safessh(dir.path())
        .args(["audit", "query", "--grep", "hello-world"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    assert!(stdout.contains("hello-world"), "{stdout}");
    assert!(!stdout.contains("goodbye"), "{stdout}");
}
