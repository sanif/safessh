//! Task 18 acceptance tests: the CLI skeleton wires `--version`, `--help`,
//! and the documented top-level subcommands.

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn version_prints() {
    Command::cargo_bin("safessh")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("safessh 0.1.0"));
}

#[test]
fn help_lists_subcommands() {
    Command::cargo_bin("safessh")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("project"))
        .stdout(contains("approve"))
        .stdout(contains("skill"))
        .stdout(contains("audit"));
}
