use safessh_core::tunnel::TunnelSpec;
use safessh_ssh::driver::{SshDriver, TunnelExit};
use safessh_ssh::mock::MockSshDriver;
use safessh_storage::project::Target;

fn target() -> Target {
    Target::Inline {
        name: "default".into(),
        host: "host".into(),
        port: 22,
        user: "u".into(),
        identity_file: None,
        proxy_jump: None,
        keychain_secret: None,
    }
}

#[tokio::test]
async fn mock_open_tunnel_returns_handle() {
    let drv = MockSshDriver::default();
    let spec = TunnelSpec::parse("5432:db:5432").unwrap();
    let mut handle = drv.open_tunnel(&target(), &spec).await.unwrap();
    assert!(handle.ssh_pid() > 0);
    handle.kill().await.unwrap();
    let exit = handle.wait().await.unwrap();
    assert!(matches!(exit, TunnelExit::Killed));
}
