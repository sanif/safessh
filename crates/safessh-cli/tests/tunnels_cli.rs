use assert_cmd::Command;
use predicates::prelude::*;
use safessh_core::tunnel::{TunnelId, TunnelRecord, TunnelSpec};
use safessh_storage::paths::Paths;
use safessh_storage::tunnels::TunnelStore;
use tempfile::tempdir;

/// Construct `Paths` directly from the tempdir root without touching the
/// parent-process env var. Each test that launches the child binary still
/// passes `cmd.env("SAFESSH_HOME", td.path())` so the subprocess picks up
/// the same root.
fn paths_in(td: &tempfile::TempDir) -> Paths {
    let root = td.path().to_path_buf();
    let p = Paths {
        config: root.join("config"),
        state: root.join("state"),
        cache: root.join("cache"),
    };
    p.ensure_dirs().unwrap();
    p
}

fn rec(id: TunnelId, pid: i32, paths: &Paths) -> TunnelRecord {
    let now = chrono::Utc::now();
    let r = TunnelRecord {
        id,
        project: "prod".into(),
        target: "default".into(),
        spec: TunnelSpec::parse("5432:db:5432").unwrap(),
        ssh_pid: pid,
        supervisor_pid: pid,
        opened_at: now,
        expires_at: now + chrono::Duration::minutes(30),
    };
    TunnelStore::new(paths).add(&r).unwrap();
    r
}

#[test]
fn tunnels_list_empty() {
    let td = tempdir().unwrap();
    let _p = paths_in(&td);
    let mut cmd = Command::cargo_bin("safessh").unwrap();
    cmd.env("SAFESSH_HOME", td.path()).args(["tunnels", "list"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("no active tunnels"));
}

#[test]
fn tunnels_list_skips_dead_supervisors() {
    let td = tempdir().unwrap();
    let p = paths_in(&td);
    let _alive = rec(TunnelId::generate(), std::process::id() as i32, &p);
    let _dead = rec(TunnelId::generate(), 2_000_000_000, &p);
    let mut cmd = Command::cargo_bin("safessh").unwrap();
    cmd.env("SAFESSH_HOME", td.path()).args(["tunnels", "list"]);
    let out = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).into_owned();
    let live_rows: Vec<&str> = stdout.lines().filter(|l| l.contains("prod")).collect();
    assert_eq!(live_rows.len(), 1, "stdout was: {stdout}");
}

#[test]
fn tunnels_close_unknown_id() {
    let td = tempdir().unwrap();
    let _p = paths_in(&td);
    let mut cmd = Command::cargo_bin("safessh").unwrap();
    cmd.env("SAFESSH_HOME", td.path())
        .args(["tunnels", "close", "doesnotexist"]);
    cmd.assert()
        .code(1)
        .stderr(predicate::str::contains("no such tunnel"));
}
