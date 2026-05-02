//! Container integration tests for the real OpenSSH tunnel driver.
//!
//! Gated behind the `integration` feature flag. These tests spin up a
//! `linuxserver/openssh-server` container (requires Docker on Linux) and
//! exercise the full tunnel lifecycle:
//!
//! 1. `forward_then_close_blocks_traffic` — open tunnel, verify a TCP
//!    connection reaches the local port, kill the handle, verify the
//!    connection is refused within 2 seconds.
//!
//! 2. `ttl_expired_closes_tunnel` — open tunnel with a 1-second TTL, run
//!    `supervisor::run_inline`, assert the returned reason is
//!    `TunnelCloseReason::TtlExpired`.
//!
//! Tests skip gracefully when Docker is unavailable so CI on macOS passes
//! without Docker.

#![cfg(feature = "integration")]

use chrono::{Duration, Utc};
use safessh_cli::supervisor;
use safessh_core::tunnel::{TunnelCloseReason, TunnelId, TunnelRecord, TunnelSpec};
use safessh_ssh::driver::SshDriver;
use safessh_ssh::openssh::OpenSshDriver;
use safessh_storage::paths::Paths;
use safessh_storage::project::Target;
use safessh_storage::tunnels::TunnelStore;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::{Duration as StdDuration, Instant};
use tempfile::tempdir;
use testcontainers::{
    core::{ContainerPort, IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `Some(())` if Docker is reachable, `None` otherwise.
fn docker_available() -> Option<()> {
    let out = StdCommand::new("docker").arg("info").output().ok()?;
    if out.status.success() {
        Some(())
    } else {
        None
    }
}

/// Path to the crate-level test fixtures directory (contains `test_key` and
/// `test_key.pub`, committed as a throwaway keypair for this test only).
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Spin up `linuxserver/openssh-server` with the fixture public key injected.
/// Returns `(container, host_port)`. The container is kept alive by the
/// caller holding the `ContainerAsync` value.
async fn start_ssh_container() -> (ContainerAsync<GenericImage>, u16) {
    let pubkey = std::fs::read_to_string(fixtures_dir().join("test_key.pub"))
        .expect("read tests/fixtures/test_key.pub — run Task 11 Step 3 first");

    let image = GenericImage::new("linuxserver/openssh-server", "latest")
        .with_exposed_port(ContainerPort::Tcp(2222))
        .with_wait_for(WaitFor::message_on_stdout("done."));

    let container = image
        .with_env_var("PUBLIC_KEY", pubkey.trim())
        .with_env_var("USER_NAME", "linuxserver.io")
        .with_env_var("PASSWORD_ACCESS", "false")
        .with_env_var("SUDO_ACCESS", "false")
        .with_env_var("PUID", "1000")
        .with_env_var("PGID", "1000")
        .with_env_var("TZ", "Etc/UTC")
        .start()
        .await
        .expect("start linuxserver/openssh-server");

    let port = container
        .get_host_port_ipv4(2222.tcp())
        .await
        .expect("get mapped host port");
    (container, port)
}

/// Poll `127.0.0.1:<port>` until it accepts TCP connections (up to 30 s).
async fn wait_for_ssh(port: u16) {
    let deadline = Instant::now() + StdDuration::from_secs(30);
    loop {
        if std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            StdDuration::from_millis(500),
        )
        .is_ok()
        {
            return;
        }
        assert!(Instant::now() < deadline, "ssh port {port} never opened");
        tokio::time::sleep(StdDuration::from_millis(250)).await;
    }
}

/// Build a tempdir under `/tmp` to keep the ControlPath socket well under
/// the 104-byte Unix socket path limit on macOS.
fn short_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("safessh-tit-")
        .tempdir_in("/tmp")
        .or_else(|_| tempdir())
        .expect("create short tempdir")
}

/// Install wrapper `ssh`/`sftp` scripts that inject `StrictHostKeyChecking=no`
/// and `UserKnownHostsFile=/dev/null` so the container's fresh host key never
/// blocks connections.
fn install_ssh_wrapper(bin_dir: &Path) {
    for name in ["ssh", "sftp"] {
        let wrapper = bin_dir.join(name);
        let real_bin = format!("/usr/bin/{name}");
        let body = format!(
            "#!/usr/bin/env bash\n\
             exec {real_bin} \
                 -o StrictHostKeyChecking=no \
                 -o UserKnownHostsFile=/dev/null \
                 -o LogLevel=ERROR \
                 \"$@\"\n"
        );
        std::fs::write(&wrapper, &body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

/// Prepend a directory containing wrapper `ssh`/`sftp` scripts to `PATH` so
/// the driver skips host-key checking against the container.
fn prepend_wrapper_to_path(bin_dir: &Path) {
    let prev = std::env::var("PATH").unwrap_or_default();
    let bin_str = bin_dir.display().to_string();
    // SAFETY: single-threaded setup inside the test; no concurrent PATH
    // mutations expected from tests that don't also install the wrapper.
    unsafe {
        std::env::set_var("PATH", format!("{bin_str}:{prev}"));
    }
}

/// Build a `Target::Inline` that connects to the container's SSH port using
/// the throwaway fixture key.
fn make_target(port: u16) -> Target {
    Target::Inline {
        name: "default".into(),
        host: "127.0.0.1".into(),
        port,
        user: "linuxserver.io".into(),
        identity_file: Some(fixtures_dir().join("test_key")),
        proxy_jump: None,
        keychain_secret: None,
    }
}

// ---------------------------------------------------------------------------
// Test 1: open tunnel → traffic flows → kill handle → connection refused
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forward_then_close_blocks_traffic() {
    if docker_available().is_none() {
        eprintln!("skipped: Docker unavailable");
        return;
    }

    let (_ct, ssh_port) = start_ssh_container().await;
    wait_for_ssh(ssh_port).await;

    let wrap_dir = short_tempdir();
    install_ssh_wrapper(wrap_dir.path());
    prepend_wrapper_to_path(wrap_dir.path());

    let ctrl_dir = short_tempdir();
    let drv = OpenSshDriver::new(ctrl_dir.path().to_path_buf()).expect("OpenSshDriver::new");
    let target = make_target(ssh_port);

    // Forward a random local port → container's SSH port (2222 internally,
    // but from *within the container* 127.0.0.1:22 is the local sshd).
    // Using the container-internal address means we can prove the tunnel is
    // carrying traffic without speaking SSH ourselves.
    let local_port = portpicker::pick_unused_port().expect("pick local port");
    let spec = TunnelSpec::parse(&format!("{local_port}:127.0.0.1:22")).unwrap();

    let mut handle = drv
        .open_tunnel(&target, &spec)
        .await
        .expect("open_tunnel should succeed");

    // Poll until the local forwarding port accepts a TCP connection (up to 5s).
    let connected = {
        let deadline = Instant::now() + StdDuration::from_secs(5);
        loop {
            match tokio::net::TcpStream::connect(("127.0.0.1", local_port)).await {
                Ok(_) => break true,
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(StdDuration::from_millis(100)).await;
                }
                Err(_) => break false,
            }
        }
    };
    assert!(connected, "tunnel never opened: local port {local_port} not reachable");

    handle.kill().await.expect("kill tunnel handle");
    let _ = handle.wait().await;

    // After kill, fresh connections should be refused within 2 s.
    let mut refused = false;
    let deadline = Instant::now() + StdDuration::from_secs(2);
    while Instant::now() < deadline {
        if tokio::net::TcpStream::connect(("127.0.0.1", local_port))
            .await
            .is_err()
        {
            refused = true;
            break;
        }
        tokio::time::sleep(StdDuration::from_millis(100)).await;
    }
    assert!(refused, "tunnel kept accepting connections after kill on port {local_port}");
}

// ---------------------------------------------------------------------------
// Test 2: open tunnel with 1-second TTL → supervisor returns TtlExpired
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ttl_expired_closes_tunnel() {
    if docker_available().is_none() {
        eprintln!("skipped: Docker unavailable");
        return;
    }

    let (_ct, ssh_port) = start_ssh_container().await;
    wait_for_ssh(ssh_port).await;

    let wrap_dir = short_tempdir();
    install_ssh_wrapper(wrap_dir.path());
    prepend_wrapper_to_path(wrap_dir.path());

    // Isolated SAFESSH_HOME so this test doesn't touch the real user state.
    let home_dir = tempdir().expect("tempdir for SAFESSH_HOME");
    // SAFETY: test-only env mutation, no concurrent tests sharing this env var.
    unsafe {
        std::env::set_var("SAFESSH_HOME", home_dir.path());
    }
    let paths = Paths::user().expect("Paths::user");
    paths.ensure_dirs().expect("ensure_dirs");

    let ctrl_dir = short_tempdir();
    let drv = OpenSshDriver::new(ctrl_dir.path().to_path_buf()).expect("OpenSshDriver::new");
    let target = make_target(ssh_port);

    let local_port = portpicker::pick_unused_port().expect("pick local port");
    let spec = TunnelSpec::parse(&format!("{local_port}:127.0.0.1:22")).unwrap();

    let handle = drv
        .open_tunnel(&target, &spec)
        .await
        .expect("open_tunnel");

    let now = Utc::now();
    let rec = TunnelRecord {
        id: TunnelId::generate(),
        project: "integration-test".into(),
        target: "default".into(),
        spec: spec.clone(),
        ssh_pid: handle.ssh_pid(),
        supervisor_pid: std::process::id() as i32,
        opened_at: now,
        expires_at: now + Duration::seconds(1),
    };
    TunnelStore::new(&paths)
        .add(&rec)
        .expect("TunnelStore::add");

    let cancel = CancellationToken::new();
    let reason = supervisor::run_inline(
        paths.clone(),
        rec.clone(),
        handle,
        StdDuration::from_secs(1),
        cancel,
    )
    .await
    .expect("run_inline");

    assert_eq!(
        reason,
        TunnelCloseReason::TtlExpired,
        "expected TtlExpired, got {reason:?}"
    );

    // Cleanup: reset SAFESSH_HOME so subsequent tests get a clean slate.
    unsafe {
        std::env::remove_var("SAFESSH_HOME");
    }
}
