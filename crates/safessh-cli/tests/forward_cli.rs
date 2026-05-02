use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::tempdir;

fn write_project(dir: &std::path::Path, name: &str, body: &str) {
    let projects = dir.join("config/projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut f = std::fs::File::create(projects.join(format!("{name}.toml"))).unwrap();
    f.write_all(body.as_bytes()).unwrap();
}

#[test]
fn default_policy_blocks_with_token() {
    let td = tempdir().unwrap();
    write_project(
        td.path(),
        "prod",
        r#"
name = "prod"
default_target = "default"

[[targets]]
name = "default"
host = "h"
port = 22
user = "u"
"#,
    );
    let mut cmd = Command::cargo_bin("safessh").unwrap();
    cmd.env("SAFESSH_HOME", td.path())
        .args(["prod", "forward", "5432:db.internal:5432"]);
    cmd.assert()
        .code(10)
        .stderr(predicate::str::contains("BLOCKED"))
        .stderr(predicate::str::contains("network:tunnel"));
}

#[test]
fn unparseable_spec_exits_usage() {
    let td = tempdir().unwrap();
    write_project(
        td.path(),
        "prod",
        r#"
name = "prod"
default_target = "default"

[[targets]]
name = "default"
host = "h"
port = 22
user = "u"
"#,
    );
    let mut cmd = Command::cargo_bin("safessh").unwrap();
    cmd.env("SAFESSH_HOME", td.path())
        .args(["prod", "forward", "bogus"]);
    cmd.assert()
        .code(2)
        .stderr(predicate::str::contains("usage:"));
}
