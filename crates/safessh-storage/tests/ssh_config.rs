//! Tests for the ssh_config parser and mtime-cache.
//!
//! `SSH_CONFIG_PATH` and `SAFESSH_HOME` are process-global env vars. A static
//! mutex serialises these tests so they don't race each other when cargo runs
//! them in parallel within the same binary.

use safessh_storage::paths::Paths;
use safessh_storage::ssh_config::SshConfigSnapshot;
use std::io::Write;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fixture(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("ssh_config");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(body.as_bytes()).unwrap();
    path
}

fn make_paths(tmp: &tempfile::TempDir) -> Paths {
    let root = tmp.path();
    let paths = Paths {
        config: root.join("config"),
        state: root.join("state"),
        cache: root.join("cache"),
    };
    paths.ensure_dirs().unwrap();
    paths
}

#[test]
fn parses_concrete_aliases() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let p = fixture(
        tmp.path(),
        "Host alpha\n  HostName a.example.com\n  User deploy\n\nHost beta\n  HostName b.example.com\n",
    );
    std::env::set_var("SSH_CONFIG_PATH", &p);

    let paths = make_paths(&tmp);
    let snap = SshConfigSnapshot::load(&paths).unwrap();
    let names: Vec<&str> = snap.aliases.iter().map(|a| a.alias.as_str()).collect();
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[test]
fn excludes_wildcard_hosts() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let p = fixture(
        tmp.path(),
        "Host *\n  ServerAliveInterval 60\n\nHost real\n  HostName r.example.com\n",
    );
    std::env::set_var("SSH_CONFIG_PATH", &p);
    let paths = make_paths(&tmp);
    let snap = SshConfigSnapshot::load(&paths).unwrap();
    let names: Vec<&str> = snap.aliases.iter().map(|a| a.alias.as_str()).collect();
    assert_eq!(names, vec!["real"]);
}

#[test]
fn missing_file_returns_empty() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("SSH_CONFIG_PATH", tmp.path().join("does-not-exist"));
    let paths = make_paths(&tmp);
    let snap = SshConfigSnapshot::load(&paths).unwrap();
    assert!(snap.aliases.is_empty());
}

#[test]
fn snapshot_is_cached_and_reused() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let p = fixture(tmp.path(), "Host one\n  HostName one.example.com\n");
    std::env::set_var("SSH_CONFIG_PATH", &p);
    let paths = make_paths(&tmp);

    let _first = SshConfigSnapshot::load(&paths).unwrap();
    assert!(
        paths.ssh_config_snapshot().exists(),
        "snapshot should be written"
    );

    // Mutate fixture WITHOUT changing mtime by setting it back
    let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
    std::fs::write(&p, "Host two\n  HostName two.example.com\n").unwrap();
    filetime::set_file_mtime(&p, filetime::FileTime::from_system_time(mtime)).unwrap();

    let second = SshConfigSnapshot::load(&paths).unwrap();
    let names: Vec<&str> = second.aliases.iter().map(|a| a.alias.as_str()).collect();
    assert_eq!(
        names,
        vec!["one"],
        "should re-use cached snapshot when mtime unchanged"
    );
}
