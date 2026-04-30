//! Integration test scaffold for `OpenSshDriver`.
//!
//! Gated behind the `integration` feature flag. v0.1 keeps this minimal:
//! we exercise `OpenSshDriver::new` and `build_argv` to keep the feature
//! gate compiling. Task 25 will replace this with a real
//! `linuxserver/openssh-server` container test that covers spawn, auth
//! via injected key, and ControlMaster socket reuse.

#![cfg(feature = "integration")]

use safessh_ssh::openssh::OpenSshDriver;
use safessh_storage::project::Target;

#[tokio::test]
async fn driver_new_and_argv_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let driver = OpenSshDriver::new(dir.path().to_path_buf())
        .expect("OpenSshDriver::new should succeed with a fresh tempdir");

    let target = Target::Inline {
        name: "smoke".into(),
        host: "127.0.0.1".into(),
        port: 22,
        user: "root".into(),
        identity_file: None,
        proxy_jump: None,
        keychain_secret: None,
    };
    let argv = driver.build_argv(&target, "echo hello");

    assert_eq!(argv[0], "ssh");
    assert!(argv.contains(&"root@127.0.0.1".to_string()));
    assert!(argv.contains(&"echo hello".to_string()));
}
