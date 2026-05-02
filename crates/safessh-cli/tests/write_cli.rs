//! Tests for `safessh <project> write <path>` (unit-level, mock-driver).
//!
//! Each test invokes `commands::write::run_with_driver_and_paths_and_bytes`
//! directly, injecting a `MockDriver` and an explicit `Paths` so no real SSH
//! or SFTP connection is needed and there are no env-var race conditions between
//! parallel tests.
//!
//! Coverage maps to the acceptance criteria:
//! 1. Happy-path: bytes written to mock, two audit events written (file_write + file_write_complete).
//! 2. Cap exceeded: stdin over cap → Ok(true), mock has nothing at that path.
//! 3. RequireApproval → Error::ApprovalRequired, no driver write.
//! 4. Deny → Error::Denied; Block → Error::Blocked.
//! 5. --yolo: skips policy, mock receives bytes, audit has yolo_invocation.

use safessh_cli::commands::write::run_with_driver_and_paths_and_bytes;
use safessh_core::error::Error;
use safessh_ssh::mock::MockDriver;
use safessh_storage::paths::Paths;
use std::fs;
use std::path::Path;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a temp `HOME`, write a project TOML, build `Paths`, and ensure dirs.
/// Returns `(TempDir, Paths)`. The project has `allow = ["file:write"]` policy
/// and two targets: `default` and `db`.
fn setup_allow_project() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("config/projects");
    fs::create_dir_all(&projects).unwrap();
    fs::write(
        projects.join("prod.toml"),
        r#"
name = "prod"
default_target = "default"

[[targets]]
name = "default"
ssh_config_alias = "prod-host"

[[targets]]
name = "db"
ssh_config_alias = "prod-db"

[policy]
allow = ["file:write"]
require_approval = []
deny = []
"#,
    )
    .unwrap();
    let paths = paths_at(dir.path());
    paths.ensure_dirs().unwrap();
    (dir, paths)
}

/// Create a temp `HOME` + project where `file:write` requires approval.
fn setup_approval_project() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("config/projects");
    fs::create_dir_all(&projects).unwrap();
    fs::write(
        projects.join("prod.toml"),
        r#"
name = "prod"
default_target = "default"

[[targets]]
name = "default"
ssh_config_alias = "prod-host"

[policy]
allow = []
require_approval = ["file:write"]
deny = []
"#,
    )
    .unwrap();
    let paths = paths_at(dir.path());
    paths.ensure_dirs().unwrap();
    (dir, paths)
}

/// Create a temp `HOME` + project that explicitly denies `file:write`.
fn setup_deny_project() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("config/projects");
    fs::create_dir_all(&projects).unwrap();
    fs::write(
        projects.join("prod.toml"),
        r#"
name = "prod"
default_target = "default"

[[targets]]
name = "default"
ssh_config_alias = "prod-host"

[policy]
allow = []
require_approval = []
deny = ["file:write"]
"#,
    )
    .unwrap();
    let paths = paths_at(dir.path());
    paths.ensure_dirs().unwrap();
    (dir, paths)
}

/// Read audit log lines from the given `Paths`, parsed as JSON values.
fn read_audit_events(paths: &Paths) -> Vec<serde_json::Value> {
    let log_path = paths.audit_log();
    if !log_path.exists() {
        return vec![];
    }
    let raw = fs::read_to_string(&log_path).unwrap();
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Build `Paths` rooted at an arbitrary directory (for tests that construct
/// it without going through `Paths::user()` / `SAFESSH_HOME`).
fn paths_at(root: &Path) -> Paths {
    Paths {
        config: root.join("config"),
        state: root.join("state"),
        cache: root.join("cache"),
    }
}

// ---------------------------------------------------------------------------
// Test 1: happy path — bytes written to mock, two audit events written (AC 1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn write_happy_path_sends_bytes_and_audits() {
    let (_dir, paths) = setup_allow_project();

    let mock = Arc::new(MockDriver::new());
    let payload = b"hello\n".to_vec();

    let result = run_with_driver_and_paths_and_bytes(
        args(&["prod", "write", "/tmp/x"]),
        false,
        mock.clone(),
        paths.clone(),
        payload.clone(),
    )
    .await;

    assert!(
        result.is_ok(),
        "expected Ok(_), got: {:?}",
        result.unwrap_err()
    );
    assert!(!result.unwrap(), "successful write should return Ok(false)");

    // The mock should have received the bytes.
    let stored = mock.get_file("default", "/tmp/x");
    assert_eq!(
        stored.as_deref(),
        Some(payload.as_slice()),
        "mock should have the written bytes at the path"
    );

    let events = read_audit_events(&paths);
    assert!(
        events.len() >= 2,
        "expected ≥2 audit events, got {}: {events:?}",
        events.len()
    );
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["event_type"].as_str().unwrap_or(""))
        .collect();
    assert!(
        types.contains(&"file_write"),
        "missing file_write attempt event: {types:?}"
    );
    assert!(
        types.contains(&"file_write_complete"),
        "missing file_write_complete event: {types:?}"
    );

    // Verify sha256 in the complete event matches the payload.
    let complete = events
        .iter()
        .find(|e| e["event_type"].as_str() == Some("file_write_complete"))
        .expect("should have file_write_complete event");
    let expected_sha = sha256_hex(&payload);
    let audited_sha = complete["data"]["sha256"].as_str().unwrap_or("");
    assert_eq!(
        audited_sha, expected_sha,
        "sha256 in audit must match the written bytes"
    );
}

// ---------------------------------------------------------------------------
// Test 2: cap exceeded → Ok(true), mock has nothing at that path (AC 2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn write_cap_exceeded_returns_ok_true_no_driver_call() {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("config/projects");
    fs::create_dir_all(&projects).unwrap();
    // Set a tiny cap of 5 bytes.
    fs::write(
        projects.join("prod.toml"),
        r#"
name = "prod"
default_target = "default"

[[targets]]
name = "default"
ssh_config_alias = "prod-host"

[policy]
allow = ["file:write"]
require_approval = []
deny = []

[output]
file_write_cap_bytes = 5
"#,
    )
    .unwrap();
    let paths = paths_at(dir.path());
    paths.ensure_dirs().unwrap();

    let mock = Arc::new(MockDriver::new());
    // 10 bytes — exceeds the 5-byte cap.
    let payload = b"0123456789".to_vec();

    let result = run_with_driver_and_paths_and_bytes(
        args(&["prod", "write", "/tmp/overflow"]),
        false,
        mock.clone(),
        paths.clone(),
        payload,
    )
    .await;

    assert!(
        matches!(result, Ok(true)),
        "cap-exceeded write should return Ok(true), got: {result:?}"
    );

    // The mock must NOT have received any bytes (no driver call).
    let stored = mock.get_file("default", "/tmp/overflow");
    assert!(
        stored.is_none(),
        "mock must not have received bytes when cap exceeded"
    );

    // A file_write_complete event with truncated=true should be present.
    let events = read_audit_events(&paths);
    let complete = events
        .iter()
        .find(|e| e["event_type"].as_str() == Some("file_write_complete"))
        .expect("should have file_write_complete event on cap exceeded");
    assert_eq!(
        complete["data"]["truncated"].as_bool(),
        Some(true),
        "cap-exceeded event must have truncated=true"
    );
    assert_eq!(
        complete["data"]["bytes_written"].as_u64(),
        Some(0),
        "bytes_written must be 0 on cap exceeded"
    );
}

// ---------------------------------------------------------------------------
// Test 3: RequireApproval → Error::ApprovalRequired, no driver write (AC 3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn write_require_approval_returns_blocked_error() {
    let (_dir, paths) = setup_approval_project();

    let mock = Arc::new(MockDriver::new());

    let result = run_with_driver_and_paths_and_bytes(
        args(&["prod", "write", "/tmp/secret"]),
        false,
        mock.clone(),
        paths.clone(),
        b"data".to_vec(),
    )
    .await;

    assert!(
        matches!(result, Err(Error::ApprovalRequired { .. })),
        "expected ApprovalRequired, got: {result:?}"
    );

    // No bytes should have been written to the mock.
    assert!(
        mock.get_file("default", "/tmp/secret").is_none(),
        "mock must not have received bytes on ApprovalRequired"
    );

    // Attempt event written; no complete event.
    let events = read_audit_events(&paths);
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["event_type"].as_str().unwrap_or(""))
        .collect();
    assert!(
        types.contains(&"file_write"),
        "missing file_write attempt event: {types:?}"
    );
    assert!(
        !types.contains(&"file_write_complete"),
        "must not have file_write_complete when blocked: {types:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 4a: Deny → Error::Denied (AC 4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn write_deny_returns_denied_error() {
    let (_dir, paths) = setup_deny_project();

    let mock = Arc::new(MockDriver::new());
    let result = run_with_driver_and_paths_and_bytes(
        args(&["prod", "write", "/tmp/blocked"]),
        false,
        mock.clone(),
        paths.clone(),
        b"data".to_vec(),
    )
    .await;

    assert!(
        matches!(result, Err(Error::Denied(_))),
        "expected Denied, got: {result:?}"
    );

    assert!(
        mock.get_file("default", "/tmp/blocked").is_none(),
        "mock must not have received bytes on Denied"
    );

    let events = read_audit_events(&paths);
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["event_type"].as_str().unwrap_or(""))
        .collect();
    assert!(
        types.contains(&"file_write"),
        "expected file_write attempt event"
    );
    assert!(
        !types.contains(&"file_write_complete"),
        "must not have complete event on deny"
    );
}

// ---------------------------------------------------------------------------
// Test 4b: Block → Error::Blocked (AC 4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn write_block_returns_blocked_error() {
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("config/projects");
    fs::create_dir_all(&projects).unwrap();
    fs::write(
        projects.join("prod.toml"),
        r#"
name = "prod"
default_target = "default"

[[targets]]
name = "default"
ssh_config_alias = "prod-host"

[policy]
allow = ["file:write"]
require_approval = []
deny = []

[[policy.file_rules]]
category = "file:write"
paths = ["/etc/passwd"]
decision = "block"
"#,
    )
    .unwrap();
    let paths = paths_at(dir.path());
    paths.ensure_dirs().unwrap();

    let mock = Arc::new(MockDriver::new());
    let result = run_with_driver_and_paths_and_bytes(
        args(&["prod", "write", "/etc/passwd"]),
        false,
        mock.clone(),
        paths.clone(),
        b"root:x:0:0".to_vec(),
    )
    .await;

    assert!(
        matches!(result, Err(Error::Blocked { .. })),
        "expected Blocked, got: {result:?}"
    );

    assert!(
        mock.get_file("default", "/etc/passwd").is_none(),
        "mock must not have received bytes on Blocked"
    );
}

// ---------------------------------------------------------------------------
// Test 5: --yolo bypasses policy, mock receives bytes, audit has yolo_invocation (AC 5/6/7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn write_yolo_bypasses_policy_and_sends_bytes() {
    // Use the approval project so the policy would normally block us.
    let (_dir, paths) = setup_approval_project();

    let mock = Arc::new(MockDriver::new());
    let payload = b"yolo content\n".to_vec();

    let result = run_with_driver_and_paths_and_bytes(
        args(&["prod", "write", "--yolo", "/tmp/yolo"]),
        false,
        mock.clone(),
        paths.clone(),
        payload.clone(),
    )
    .await;

    assert!(
        result.is_ok(),
        "yolo should bypass approval requirement, got: {result:?}"
    );
    assert!(!result.unwrap(), "yolo write should return Ok(false)");

    // Mock must have received the bytes.
    let stored = mock.get_file("default", "/tmp/yolo");
    assert_eq!(
        stored.as_deref(),
        Some(payload.as_slice()),
        "mock should have yolo-written bytes"
    );

    let events = read_audit_events(&paths);
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["event_type"].as_str().unwrap_or(""))
        .collect();
    assert!(
        types.contains(&"yolo_invocation"),
        "missing yolo_invocation audit event: {types:?}"
    );
    assert!(
        types.contains(&"file_write_complete"),
        "missing file_write_complete event on yolo path: {types:?}"
    );
    // The policy attempt event must NOT appear on the yolo path.
    assert!(
        !types.contains(&"file_write"),
        "must not have file_write attempt event on yolo path: {types:?}"
    );
}

// ---------------------------------------------------------------------------
// Helper: sha256 hex digest (mirrors file_common::sha256_hex)
// ---------------------------------------------------------------------------
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}
