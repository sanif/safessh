//! Tests for the in-TUI ssh-config import dialog.
//!
//! These tests mutate the process-wide `SSH_CONFIG_PATH` env var that
//! `SshConfigSnapshot::load` reads, so they must run serialized — a
//! shared mutex guards each test for the duration of its setup + assert.

use safessh_storage::paths::Paths;
use safessh_storage::project::ProjectStore;
use safessh_tui::screens::projects::ProjectsScreen;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn env_lock() -> MutexGuard<'static, ()> {
    static M: OnceLock<Mutex<()>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn setup_with_config(body: &str) -> (tempfile::TempDir, Paths) {
    let tmp = tempfile::tempdir().unwrap();
    let p = Paths {
        config: tmp.path().join("config"),
        state: tmp.path().join("state"),
        cache: tmp.path().join("cache"),
    };
    p.ensure_dirs().unwrap();
    let cfg_path = tmp.path().join("ssh_config");
    std::fs::write(&cfg_path, body).unwrap();
    std::env::set_var("SSH_CONFIG_PATH", &cfg_path);
    (tmp, p)
}

#[test]
fn dialog_loads_aliases() {
    let _g = env_lock();
    let (_tmp, p) = setup_with_config(
        "Host alpha\n  HostName a.example\n  User ua\nHost beta\n  HostName b.example\n",
    );
    let mut s = ProjectsScreen::load(&p).unwrap();
    s.open_import().unwrap();
    let dlg = s.import.as_ref().unwrap();
    let aliases: Vec<&str> = dlg.entries.iter().map(|e| e.alias.alias.as_str()).collect();
    assert!(aliases.contains(&"alpha"), "got: {aliases:?}");
    assert!(aliases.contains(&"beta"), "got: {aliases:?}");
    assert!(dlg.entries.iter().all(|e| !e.checked));
}

#[test]
fn space_toggles_check() {
    let _g = env_lock();
    let (_tmp, p) = setup_with_config("Host one\n  HostName x.example\n");
    let mut s = ProjectsScreen::load(&p).unwrap();
    s.open_import().unwrap();
    s.import_toggle();
    assert!(s.import.as_ref().unwrap().entries[0].checked);
    s.import_toggle();
    assert!(!s.import.as_ref().unwrap().entries[0].checked);
}

#[test]
fn enter_creates_only_checked() {
    let _g = env_lock();
    let (_tmp, p) = setup_with_config(
        "Host alpha\n  HostName a.example\nHost beta\n  HostName b.example\nHost gamma\n  HostName c.example\n",
    );
    let mut s = ProjectsScreen::load(&p).unwrap();
    s.open_import().unwrap();
    // Check alpha (cursor at 0) and gamma (cursor at 2).
    s.import_toggle();
    s.import_move(2);
    s.import_toggle();
    let created = s.import_commit().unwrap();
    assert_eq!(created, 2);
    let names: Vec<String> = ProjectStore::new(p.clone())
        .list()
        .unwrap()
        .into_iter()
        .collect();
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"gamma".to_string()));
    assert!(!names.contains(&"beta".to_string()));
}

#[test]
fn existing_names_skipped() {
    let _g = env_lock();
    let (_tmp, p) = setup_with_config("Host taken\n  HostName x.example\n");
    // Pre-create a project that collides with the alias name.
    let store = ProjectStore::new(p.clone());
    store
        .save(&safessh_storage::project::Project {
            name: "taken".into(),
            default_target: "default".into(),
            targets: vec![safessh_storage::project::Target::SshConfigAlias {
                name: "default".into(),
                ssh_config_alias: "old".into(),
            }],
            policy: safessh_storage::project::Policy::default(),
            approvals: safessh_storage::project::Approvals::default(),
            output: safessh_storage::project::OutputCaps::default(),
        })
        .unwrap();

    let mut s = ProjectsScreen::load(&p).unwrap();
    s.open_import().unwrap();
    s.import_toggle();
    let created = s.import_commit().unwrap();
    assert_eq!(created, 0, "existing-name aliases should be skipped");
}

#[test]
fn esc_cancels() {
    let _g = env_lock();
    let (_tmp, p) = setup_with_config("Host any\n  HostName a.example\n");
    let mut s = ProjectsScreen::load(&p).unwrap();
    s.open_import().unwrap();
    assert!(s.import.is_some());
    s.close_import();
    assert!(s.import.is_none());
}
