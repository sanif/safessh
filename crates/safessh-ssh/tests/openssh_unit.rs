//! Unit tests for `OpenSshDriver::build_argv`.
//!
//! These tests exercise pure argv construction — no subprocess is
//! spawned, so they're safe to run in any environment.

use safessh_ssh::openssh::OpenSshDriver;
use safessh_storage::project::Target;

fn driver() -> OpenSshDriver {
    let dir = tempfile::tempdir().unwrap();
    OpenSshDriver::new(dir.path().to_path_buf()).unwrap()
}

#[test]
fn argv_uses_ssh_config_alias() {
    let d = driver();
    let t = Target::SshConfigAlias {
        name: "web".into(),
        ssh_config_alias: "cureocity-web".into(),
    };
    let argv = d.build_argv(&t, "ls /var");

    assert_eq!(argv[0], "ssh");
    assert!(
        argv.contains(&"cureocity-web".to_string()),
        "alias missing: {argv:?}"
    );
    assert!(argv.contains(&"--".to_string()), "-- missing: {argv:?}");
    assert!(
        argv.contains(&"ls /var".to_string()),
        "command missing: {argv:?}"
    );

    // ControlMaster opts must all be present.
    assert!(argv.iter().any(|a| a == "ControlMaster=auto"));
    assert!(argv.iter().any(|a| a.starts_with("ControlPath=")));
    assert!(argv.iter().any(|a| a == "ControlPersist=60s"));
}

#[test]
fn argv_inline_includes_port_user_host() {
    let d = driver();
    let t = Target::Inline {
        name: "db".into(),
        host: "10.0.0.1".into(),
        port: 2200,
        user: "deploy".into(),
        identity_file: Some("/keys/id".into()),
        proxy_jump: Some("bastion".into()),
        keychain_secret: None,
    };
    let argv = d.build_argv(&t, "ls");

    assert_eq!(argv[0], "ssh");
    assert!(
        argv.windows(2).any(|w| w[0] == "-p" && w[1] == "2200"),
        "missing -p 2200: {argv:?}"
    );
    assert!(
        argv.windows(2).any(|w| w[0] == "-i" && w[1] == "/keys/id"),
        "missing -i: {argv:?}"
    );
    assert!(
        argv.windows(2).any(|w| w[0] == "-J" && w[1] == "bastion"),
        "missing -J: {argv:?}"
    );
    assert!(
        argv.contains(&"deploy@10.0.0.1".to_string()),
        "missing user@host: {argv:?}"
    );
    assert!(argv.contains(&"--".to_string()));
    assert!(argv.contains(&"ls".to_string()));
}

#[test]
fn argv_inline_omits_optional_flags_when_absent() {
    let d = driver();
    let t = Target::Inline {
        name: "plain".into(),
        host: "h".into(),
        port: 22,
        user: "u".into(),
        identity_file: None,
        proxy_jump: None,
        keychain_secret: None,
    };
    let argv = d.build_argv(&t, "id");
    assert!(!argv.iter().any(|a| a == "-i"));
    assert!(!argv.iter().any(|a| a == "-J"));
    assert!(argv.contains(&"u@h".to_string()));
}
