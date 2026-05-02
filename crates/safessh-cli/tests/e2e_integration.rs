//! End-to-end integration tests for the `safessh` CLI.
//!
//! Spins up `linuxserver/openssh-server` in a Docker container and drives
//! the binary through three real-world scenarios:
//!
//! 1. Happy path: `safessh demo exec "echo hello"` → exit 0 with framing.
//! 2. Approval round-trip: destructive command → BLOCKED token → `safessh
//!    approve` → re-run succeeds.
//! 3. ControlMaster reuse: second invocation faster than first (or socket
//!    file present in the per-target sockets dir).
//!
//! Gated behind the `integration` feature: `cargo test --package
//! safessh-cli --features integration --test e2e_integration`.
//!
//! When Docker is unavailable (no daemon, missing binary) every test
//! prints a skip message via `eprintln!` and passes — matching CI
//! behavior on environments without Docker. We do NOT fail the suite
//! for missing Docker.
//!
//! ## Host-key handling
//!
//! The dockerized SSH server has a freshly-generated host key each run,
//! so strict host-key checking against the user's real `known_hosts`
//! would fail. The production driver does not expose a knob for
//! relaxing this — and shouldn't. Instead, the test suite installs a
//! tiny `ssh` wrapper script into a tempdir, prepends that tempdir to
//! `PATH`, and the wrapper exec's the system `ssh` with two extra
//! options injected: `StrictHostKeyChecking=no` and
//! `UserKnownHostsFile=/dev/null`. The relaxation is confined to the
//! test process; production safessh still benefits from full strict
//! checking.

#![cfg(feature = "integration")]

use assert_cmd::Command;
use predicates::str::contains;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::{Duration, Instant};
use testcontainers::{
    core::{ContainerPort, IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

/// Returns `Some(())` if Docker is available, otherwise `None` and the
/// caller must skip+pass. Doing this once per test (rather than as a
/// `#[ctor]`) keeps each test individually skippable in `cargo test`
/// output.
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
/// to a random host port and return it. The default username configured
/// by the image is `linuxserver.io`.
async fn start_ssh_container() -> (ContainerAsync<GenericImage>, u16) {
    ensure_keypair();
    let pubkey = std::fs::read_to_string(fixtures_dir().join("id_ed25519.pub"))
        .expect("read fixture public key");

    let image = GenericImage::new("linuxserver/openssh-server", "latest")
        .with_exposed_port(ContainerPort::Tcp(2222))
        // The image logs "[ls.io-init] done." once SSH is listening; using
        // a string match is the most reliable signal we have without a
        // dedicated module.
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

/// Build a tempdir rooted at `/tmp` rather than the platform default.
/// macOS's default `$TMPDIR` lives under `/var/folders/...` which makes
/// the resulting `ControlPath` exceed the 104-byte Unix socket limit.
fn short_tempdir() -> tempfile::TempDir {
    let base = std::path::PathBuf::from("/tmp");
    tempfile::Builder::new()
        .prefix("safessh-e2e-")
        .tempdir_in(&base)
        .or_else(|_| tempfile::tempdir())
        .expect("create tempdir")
}

/// Test environment: `SAFESSH_HOME` plus an `ssh` wrapper installed
/// into a separate tempdir that gets prepended to `PATH`.
struct TestEnv {
    safessh_home: tempfile::TempDir,
    bin_dir: tempfile::TempDir,
}

impl TestEnv {
    fn new() -> Self {
        let safessh_home = short_tempdir();
        let bin_dir = short_tempdir();
        install_ssh_wrapper(bin_dir.path());
        Self {
            safessh_home,
            bin_dir,
        }
    }

    fn safessh_home(&self) -> &Path {
        self.safessh_home.path()
    }

    /// Build a `safessh` invocation with `SAFESSH_HOME` set and the test
    /// `ssh` wrapper prepended to `PATH`.
    fn safessh(&self) -> Command {
        let mut c = Command::cargo_bin("safessh").unwrap();
        c.env("SAFESSH_HOME", self.safessh_home.path());
        let prev_path = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", self.bin_dir.path().display(), prev_path);
        c.env("PATH", new_path);
        c
    }
}

/// Drop a `ssh` wrapper into `bin_dir` that exec's the real `ssh` with
/// two extra options injected at the front of argv:
/// `StrictHostKeyChecking=no` and `UserKnownHostsFile=/dev/null`.
/// The wrapper is intentionally minimal — any change to ssh's flag
/// parsing that breaks this would also break the production driver.
fn install_ssh_wrapper(bin_dir: &Path) {
    let wrapper = bin_dir.join("ssh");
    let body = "#!/usr/bin/env bash\n\
                exec /usr/bin/ssh \
                    -o StrictHostKeyChecking=no \
                    -o UserKnownHostsFile=/dev/null \
                    -o LogLevel=ERROR \
                    \"$@\"\n";
    std::fs::write(&wrapper, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Write a project TOML pointing the named project at the dockerized SSH
/// server using inline target params. The keypair lives in the
/// workspace `tests/fixtures/` dir.
///
/// The `project_name` is used as both the TOML `name` field and the file name
/// so tests that need a different project slug (e.g. `"prod"`) don't have to
/// duplicate this boilerplate.
fn write_project_named(home: &Path, port: u16, project_name: &str) {
    let projects = home.join("config/projects");
    std::fs::create_dir_all(&projects).unwrap();
    let id = fixtures_dir().join("id_ed25519");
    let toml = format!(
        r#"
name = "{project_name}"
default_target = "default"

[[targets]]
name = "default"
host = "127.0.0.1"
port = {port}
user = "linuxserver.io"
identity_file = "{}"

[policy]
allow = ["read:safe"]
require_approval = ["destructive:filesystem"]

[approvals]
timed_default_minutes = 30
"#,
        id.display()
    );
    std::fs::write(projects.join(format!("{project_name}.toml")), toml).unwrap();
}

/// Write a project TOML pointing the demo project at the dockerized SSH
/// server using inline target params. The keypair lives in the
/// workspace `tests/fixtures/` dir.
fn write_project(home: &Path, port: u16) {
    let projects = home.join("config/projects");
    std::fs::create_dir_all(&projects).unwrap();
    let id = fixtures_dir().join("id_ed25519");
    let toml = format!(
        r#"
name = "demo"
default_target = "default"

[[targets]]
name = "default"
host = "127.0.0.1"
port = {port}
user = "linuxserver.io"
identity_file = "{}"

[policy]
allow = ["read:safe"]
require_approval = ["destructive:filesystem"]

[approvals]
timed_default_minutes = 30
"#,
        id.display()
    );
    std::fs::write(projects.join("demo.toml"), toml).unwrap();
}

/// Wait for the container's SSH port to actually accept TCP connections.
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn happy_path() {
    if check_docker_available().is_none() {
        eprintln!("skipped: Docker unavailable");
        return;
    }

    let (_ct, port) = start_ssh_container().await;
    wait_for_ssh(port).await;
    let env = TestEnv::new();
    write_project(env.safessh_home(), port);

    env.safessh()
        .args(["demo", "exec", "echo hello"])
        .assert()
        .success()
        .stdout(contains("hello"))
        .stdout(contains("<exit code=\"0\""));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn approval_round_trip() {
    if check_docker_available().is_none() {
        eprintln!("skipped: Docker unavailable");
        return;
    }

    let (_ct, port) = start_ssh_container().await;
    wait_for_ssh(port).await;
    let env = TestEnv::new();
    write_project(env.safessh_home(), port);

    // 1) Destructive command → exit 10 with `Token: <token>` on stderr.
    let out = env
        .safessh()
        .args(["demo", "exec", "rm -rf /tmp/x"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(10),
        "destructive cmd should exit 10; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let token = stderr
        .lines()
        .find(|l| l.starts_with("Token:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .expect("token line in stderr")
        .to_string();

    // 2) Approve via `--timed`. The default "once" path only removes
    //    the pending entry without persisting any rule, so the re-run
    //    would re-trigger the policy engine. `--timed` adds a
    //    `TimedRule` keyed on the parsed binary+flags, which the next
    //    invocation matches.
    env.safessh()
        .args(["approve", &token, "--timed", "--minutes", "5"])
        .assert()
        .success();

    // 3) Re-run the same destructive command — should now succeed.
    env.safessh()
        .args(["demo", "exec", "rm -rf /tmp/x"])
        .assert()
        .success();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn control_master_reuse_is_faster() {
    if check_docker_available().is_none() {
        eprintln!("skipped: Docker unavailable");
        return;
    }

    let (_ct, port) = start_ssh_container().await;
    wait_for_ssh(port).await;
    let env = TestEnv::new();
    write_project(env.safessh_home(), port);

    let run = || {
        env.safessh()
            .args(["demo", "exec", "echo hi"])
            .output()
            .unwrap()
    };

    let t1 = Instant::now();
    let r1 = run();
    let d1 = t1.elapsed();
    assert!(
        r1.status.success(),
        "first invocation must succeed; stderr={}",
        String::from_utf8_lossy(&r1.stderr)
    );

    let t2 = Instant::now();
    let r2 = run();
    let d2 = t2.elapsed();
    assert!(
        r2.status.success(),
        "second invocation must succeed; stderr={}",
        String::from_utf8_lossy(&r2.stderr)
    );

    // Primary signal: a control socket file lives under
    // `<SAFESSH_HOME>/cache/control-sockets/`. Existence proves
    // ControlMaster is active across invocations.
    let sockets_dir = env.safessh_home().join("cache/control-sockets");
    let has_socket = sockets_dir
        .read_dir()
        .map(|rd| rd.flatten().count() > 0)
        .unwrap_or(false);

    // Secondary signal: timing. Use a generous `<= d1` rather than a
    // strict `< d1 / 2` because cold-start variance on shared CI
    // runners can dwarf the SSH handshake savings.
    assert!(
        has_socket || d2 <= d1,
        "expected ControlMaster reuse: socket_present={has_socket}, d1={d1:?}, d2={d2:?}"
    );
}

/// `safessh prod read /etc/shadow` must exit 12 (Denied by preset deny-list,
/// SAFETY-INVARIANT-14) and must NOT produce a `file_read_complete` audit
/// event — the policy engine must short-circuit before any SSH I/O.
///
/// A real container is used so the test verifies the CLI plumbing end-to-end;
/// the policy decision fires before any SFTP call so the container itself is
/// never contacted for the shadow read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_blocks_etc_shadow_preset() {
    if check_docker_available().is_none() {
        eprintln!("skipped: Docker unavailable");
        return;
    }

    let (_ct, port) = start_ssh_container().await;
    wait_for_ssh(port).await;
    let env = TestEnv::new();
    write_project_named(env.safessh_home(), port, "prod");

    // Ensure state dirs exist so the audit log can be written.
    std::fs::create_dir_all(env.safessh_home().join("state")).unwrap();

    let out = env
        .safessh()
        .args(["prod", "read", "/etc/shadow"])
        .output()
        .unwrap();

    // SAFETY-INVARIANT-14: preset deny-list blocks /etc/shadow → exit 12.
    assert_eq!(
        out.status.code(),
        Some(12),
        "expected exit 12 (Denied) for /etc/shadow; got {:?}; stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );

    // The audit log must contain a `file_read` attempt event with decision=deny.
    // It must NOT contain a `file_read_complete` event (no SSH call was made).
    let audit_path = env.safessh_home().join("state/audit.log");
    let audit_content = std::fs::read_to_string(&audit_path).unwrap_or_default();

    let has_attempt = audit_content.lines().any(|line| {
        let v: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
        v["event_type"] == "file_read"
            && v["data"]["path"] == "/etc/shadow"
            && v["data"]["decision"] == "deny"
    });
    assert!(
        has_attempt,
        "expected file_read attempt event with decision=deny in audit log; log:\n{audit_content}"
    );

    let has_complete = audit_content.lines().any(|line| {
        let v: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
        v["event_type"] == "file_read_complete"
    });
    assert!(
        !has_complete,
        "expected NO file_read_complete event (SSH must not have been called); log:\n{audit_content}"
    );
}
