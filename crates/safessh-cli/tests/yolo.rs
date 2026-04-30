//! Integration tests for the global `--yolo` flag (Task 23).
//!
//! The yolo flag bypasses the policy engine. The global config kill switch
//! `disable_yolo = true` short-circuits with exit 13 (`Error::YoloRefused`)
//! **before** any project lookup — so we don't need a project on disk to
//! exercise the refusal path.

use assert_cmd::Command;

#[test]
fn yolo_disabled_in_global_returns_13() {
    let dir = tempfile::tempdir().unwrap();
    // Per Task 5's path layout, `Paths::user()` reads
    // `<SAFESSH_HOME>/config/config.toml`.
    let cfg = dir.path().join("config");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("config.toml"), "disable_yolo = true\n").unwrap();

    Command::cargo_bin("safessh")
        .unwrap()
        .env("SAFESSH_HOME", dir.path())
        .args(["nope", "exec", "ls", "--yolo"])
        .assert()
        .code(13);
}

#[test]
fn yolo_disabled_with_yolo_before_subcommand_returns_13() {
    // The flag is `global = true`, so it is also accepted before the
    // external subcommand position.
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("config.toml"), "disable_yolo = true\n").unwrap();

    Command::cargo_bin("safessh")
        .unwrap()
        .env("SAFESSH_HOME", dir.path())
        .args(["--yolo", "nope", "exec", "ls"])
        .assert()
        .code(13);
}
