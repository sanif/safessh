use safessh_storage::paths::Paths;
use safessh_storage::project::{Approvals, OutputCaps, Policy, Project, ProjectStore, Target};

fn temp_paths() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("SAFESSH_HOME", dir.path());
    let paths = Paths::user().unwrap();
    paths.ensure_dirs().unwrap();
    (dir, paths)
}

#[test]
fn save_and_load_project() {
    let (_dir, paths) = temp_paths();
    let store = ProjectStore::new(paths);
    let project = Project {
        name: "cureocity".into(),
        default_target: "web".into(),
        targets: vec![Target::SshConfigAlias {
            name: "web".into(),
            ssh_config_alias: "cureocity-web".into(),
        }],
        policy: Policy {
            allow: vec!["read:safe".into()],
            require_approval: vec!["destructive:filesystem".into()],
            deny: vec![],
        },
        approvals: Approvals::default(),
        output: OutputCaps::default(),
    };
    store.save(&project).unwrap();
    let loaded = store.load("cureocity").unwrap();
    assert_eq!(loaded.name, "cureocity");
    assert_eq!(loaded.default_target, "web");
    assert_eq!(loaded.targets.len(), 1);
    assert_eq!(loaded.targets[0].name(), "web");
    assert_eq!(loaded.policy.allow, vec!["read:safe".to_string()]);
    assert_eq!(loaded.approvals.timed_default_minutes, 30);
    assert!(!loaded.approvals.yolo);
    assert_eq!(loaded.output.stdout_cap_bytes, 1_048_576);
    assert_eq!(loaded.output.stderr_cap_bytes, 262_144);
    assert_eq!(loaded.output.file_read_cap_bytes, 5_242_880);
    assert_eq!(loaded.output.tunnel_ttl_minutes, 30);

    let listed = store.list().unwrap();
    assert_eq!(listed, vec!["cureocity".to_string()]);

    store.remove("cureocity").unwrap();
    let err = store.load("cureocity").unwrap_err();
    match err {
        safessh_core::error::Error::ProjectNotFound(id) => assert_eq!(id, "cureocity"),
        other => panic!("expected ProjectNotFound, got {other:?}"),
    }
}
