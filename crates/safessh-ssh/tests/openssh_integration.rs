//! Integration test scaffold for `OpenSshDriver`.
//!
//! Gated behind the `integration` feature flag. The smoke test exercises
//! `OpenSshDriver::new` and `build_argv` without spawning a process.
//!
//! The sftp tests spin up a real `linuxserver/openssh-server` container and
//! exercise `read_file` / `write_file` against it. They mirror the container
//! setup from `safessh-cli/tests/e2e_integration.rs` (same image, same key
//! injection, same `ssh` wrapper trick for host-key relaxation).
//!
//! ## Host-key handling
//!
//! Same approach as the CLI e2e tests: a minimal `ssh` wrapper is installed
//! into a process-global bin dir (via `OnceLock`) and prepended to `PATH`
//! once at startup. Both sftp tests share the same wrapper directory so there
//! is no PATH-mutation race between parallel test threads.

#![cfg(feature = "integration")]

use safessh_ssh::driver::SshDriver;
use safessh_ssh::openssh::OpenSshDriver;
use safessh_storage::project::Target;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Smoke test (no container)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Container helpers (sftp tests only)
// ---------------------------------------------------------------------------

use testcontainers::{
    core::{ContainerPort, IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

/// Returns `Some(())` if Docker is available, otherwise `None` (caller skips).
fn check_docker_available() -> Option<()> {
    let out = StdCommand::new("docker").arg("info").output().ok()?;
    if out.status.success() {
        Some(())
    } else {
        None
    }
}

/// `tests/fixtures/` lives at the workspace root.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root reachable from CARGO_MANIFEST_DIR")
        .join("tests/fixtures")
}

/// Ensure the ed25519 fixture keypair exists; regenerate via the helper
/// script if missing. The keypair is gitignored so a fresh checkout has
/// no key on disk.
fn ensure_keypair() {
    let dir = fixtures_dir();
    let priv_key = dir.join("id_ed25519");
    let pub_key = dir.join("id_ed25519.pub");
    if priv_key.exists() && pub_key.exists() {
        return;
    }
    let script = dir.join("gen-keys.sh");
    let status = StdCommand::new("bash")
        .arg(&script)
        .status()
        .expect("run gen-keys.sh");
    assert!(status.success(), "gen-keys.sh must succeed");
}

/// Bring up `linuxserver/openssh-server` with our public key injected via
/// `PUBLIC_KEY`. Container exposes SSH on port 2222 internally; we map
/// to a random host port and return it. The default username is `linuxserver.io`.
async fn start_ssh_container() -> (ContainerAsync<GenericImage>, u16) {
    ensure_keypair();
    let pubkey = std::fs::read_to_string(fixtures_dir().join("id_ed25519.pub"))
        .expect("read fixture public key");

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
        .expect("get host port");
    (container, port)
}

/// Build a tempdir rooted at `/tmp` to keep ControlPath well under the
/// 104-byte Unix socket limit imposed by macOS.
fn short_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("safessh-ssh-it-")
        .tempdir_in("/tmp")
        .or_else(|_| tempfile::tempdir())
        .expect("create tempdir")
}

/// Process-global bin dir containing the `ssh` no-host-check wrapper.
/// Shared across tests so PATH is mutated once, avoiding races between
/// parallel test threads.
static WRAPPER_BIN_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Install (once) an `ssh` wrapper into a fixed per-process tempdir and
/// prepend that dir to `PATH`. The wrapper exec's the real ssh with
/// `StrictHostKeyChecking=no` and `UserKnownHostsFile=/dev/null` so the
/// container's fresh host key never blocks test connections.
fn ensure_ssh_wrapper_in_path() {
    let bin_dir = WRAPPER_BIN_DIR.get_or_init(|| {
        let d = tempfile::Builder::new()
            .prefix("safessh-ssh-wrap-")
            .tempdir_in("/tmp")
            .or_else(|_| tempfile::tempdir())
            .expect("create wrapper tempdir")
            .keep(); // keep the dir alive for the process lifetime
        install_ssh_wrapper(&d);
        d
    });

    // Prepend if not already present (idempotent).
    let prev = std::env::var("PATH").unwrap_or_default();
    let bin_str = bin_dir.display().to_string();
    if !prev.starts_with(&bin_str) {
        // SAFETY: only mutation of process-global PATH in this file;
        // the wrapper content is identical for every call, so a race
        // between two test threads only risks re-prepending the same dir.
        unsafe {
            std::env::set_var("PATH", format!("{bin_str}:{prev}"));
        }
    }
}

/// Write `ssh` and `sftp` wrapper scripts to `bin_dir`.
///
/// Both wrappers inject `StrictHostKeyChecking=no` and
/// `UserKnownHostsFile=/dev/null` so the container's freshly-generated
/// host key doesn't block connections. The sftp wrapper is needed because
/// `sftp` falls back to a direct SSH connection when the ControlMaster
/// socket path contains unexpanded tokens (e.g. `%C`) that the sftp binary
/// doesn't expand the same way as ssh on some platforms.
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

/// Wait for the container's SSH port to accept TCP connections.
async fn wait_for_ssh(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(500),
        )
        .is_ok()
        {
            return;
        }
        if Instant::now() >= deadline {
            panic!("ssh port {port} never accepted connections");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Build a `Target::Inline` pointing at the container.
fn make_target(port: u16) -> Target {
    Target::Inline {
        name: "default".into(),
        host: "127.0.0.1".into(),
        port,
        user: "linuxserver.io".into(),
        identity_file: Some(fixtures_dir().join("id_ed25519")),
        proxy_jump: None,
        keychain_secret: None,
    }
}

// ---------------------------------------------------------------------------
// sftp integration tests
// ---------------------------------------------------------------------------

/// Read `/etc/hostname` from the container and verify it's a non-empty string.
///
/// `exec("true")` is called first to bring up the ControlMaster socket that
/// `read_file` (via sftp's `ControlMaster=no`) reuses — without the master
/// socket, sftp would attempt its own TCP handshake and fail host-key checking.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_remote_file_returns_hostname() {
    if check_docker_available().is_none() {
        eprintln!("skipped: Docker unavailable");
        return;
    }

    let (_ct, port) = start_ssh_container().await;
    wait_for_ssh(port).await;

    ensure_ssh_wrapper_in_path();

    let control_dir = short_tempdir();
    let driver = OpenSshDriver::new(control_dir.path().to_path_buf()).expect("driver::new");
    let target = make_target(port);

    // Warm up the ControlMaster so sftp can reuse the mux socket.
    driver
        .exec(&target, "true", 1024, 1024, Box::new(|_| {}))
        .await
        .expect("warm-up exec");

    let result = driver
        .read_file(&target, "/etc/hostname", 4096)
        .await
        .expect("read_file /etc/hostname");

    assert!(
        !result.bytes.is_empty(),
        "expected non-empty /etc/hostname, got empty"
    );
    assert!(
        !result.truncated,
        "/etc/hostname should not be truncated at 4 KiB cap"
    );
    let content = String::from_utf8_lossy(&result.bytes);
    assert!(
        !content.trim().is_empty(),
        "hostname should be a non-empty string, got {content:?}"
    );
}

/// Write bytes to a unique path in `/tmp`, then read them back and verify the
/// round-trip is byte-exact.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_then_read_round_trips_via_sftp() {
    if check_docker_available().is_none() {
        eprintln!("skipped: Docker unavailable");
        return;
    }

    let (_ct, port) = start_ssh_container().await;
    wait_for_ssh(port).await;

    ensure_ssh_wrapper_in_path();

    let control_dir = short_tempdir();
    let driver = OpenSshDriver::new(control_dir.path().to_path_buf()).expect("driver::new");
    let target = make_target(port);

    // Warm up the ControlMaster.
    driver
        .exec(&target, "true", 1024, 1024, Box::new(|_| {}))
        .await
        .expect("warm-up exec");

    let payload = b"safessh integration test\n";
    // Use port number as a unique suffix to avoid collisions when tests run in
    // parallel against separate containers on different host ports.
    let remote_path = format!("/tmp/safessh-it-{port}.txt");

    let write_result = driver
        .write_file(&target, &remote_path, payload)
        .await
        .expect("write_file");
    assert_eq!(
        write_result.bytes_written,
        payload.len() as u64,
        "bytes_written mismatch"
    );

    let read_result = driver
        .read_file(&target, &remote_path, 65536)
        .await
        .expect("read_file after write");
    assert_eq!(read_result.bytes, payload, "round-trip bytes mismatch");
    assert!(!read_result.truncated);
}
