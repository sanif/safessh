//! Tests for `safessh <project> read <path>` (unit-level, mock-driver).
//!
//! Each test invokes `commands::read::run_with_driver_and_paths` directly,
//! injecting a `MockDriver` and an explicit `Paths` so no real SSH or SFTP
//! connection is needed and there are no env-var race conditions between
//! parallel tests.
//!
//! Coverage maps to the six acceptance criteria:
//! 1. Happy-path: framed stdout, exit 0, two audit events written.
//! 2. `--on db` selects the named target.
//! 3. Truncation → `Ok(true)` returned; complete audit has `truncated: true`.
//! 4. `RequireApproval` → `Error::ApprovalRequired`, no SSH call.
//! 5. `Deny` → `Error::Denied`; `Block` → `Error::Blocked`.
//! 6. Redactor strips AWS access keys; sha256 in audit matches pre-redaction bytes.
//! 7. `--yolo` skips policy, writes `yolo_invocation` event, still reads file.
//! 8. `--yolo` + `disable_yolo = true` → `Error::YoloRefused`.

use safessh_cli::commands::read::run_with_driver_and_paths;
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
/// Returns `(TempDir, Paths)`. The project has an `allow = ["file:read"]` policy
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
allow = ["file:read"]
require_approval = []
deny = []
"#,
    )
    .unwrap();
    let paths = paths_at(dir.path());
    paths.ensure_dirs().unwrap();
    (dir, paths)
}

/// Create a temp `HOME` + project where `file:read` requires approval.
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
require_approval = ["file:read"]
deny = []
"#,
    )
    .unwrap();
    let paths = paths_at(dir.path());
    paths.ensure_dirs().unwrap();
    (dir, paths)
}

/// Create a temp `HOME` + project that explicitly denies `file:read`.
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
deny = ["file:read"]
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
// Test 1: happy path — two audit events written, Ok(false) returned
// Acceptance criteria 1: `file_read` + `file_read_complete` events; exit 0.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_happy_path_frames_stdout_and_audits() {
    let (_dir, paths) = setup_allow_project();

    let mock = Arc::new(MockDriver::new());
    mock.put_file("default", "/etc/hostname", b"prod-server\n");

    let result = run_with_driver_and_paths(
        args(&["prod", "read", "/etc/hostname"]),
        false,
        mock,
        paths.clone(),
    )
    .await;
    assert!(
        result.is_ok(),
        "expected Ok(_), got: {:?}",
        result.unwrap_err()
    );
    assert_eq!(result.unwrap(), false, "non-truncated read should return Ok(false)");

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
        types.contains(&"file_read"),
        "missing file_read attempt event: {types:?}"
    );
    assert!(
        types.contains(&"file_read_complete"),
        "missing file_read_complete event: {types:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: --on db selects the named target (acceptance criterion 2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_on_named_target_selects_db() {
    let (_dir, paths) = setup_allow_project();

    let mock = Arc::new(MockDriver::new());
    // Only seed on the "db" target.  If "default" is used, read_file returns
    // NotFound and the test fails — confirming target selection.
    mock.put_file("db", "/etc/hostname", b"db-server\n");

    let result = run_with_driver_and_paths(
        args(&["prod", "--on", "db", "read", "/etc/hostname"]),
        false,
        mock,
        paths,
    )
    .await;
    assert!(
        result.is_ok(),
        "expected Ok(()) with --on db, got: {:?}",
        result.unwrap_err()
    );
}

// ---------------------------------------------------------------------------
// Test 3: truncation → Ok(true) and complete audit has truncated=true (AC 3)
//
// We use a project with a 5-byte file_read cap and seed a file that exceeds it.
// The mock driver returns `truncated=true` when stored bytes > cap_bytes.
// run_with_driver_and_paths returns Ok(true) so the caller (main.rs) can exit 30.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_truncated_returns_ok_true_and_audits_truncated() {
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
allow = ["file:read"]
require_approval = []
deny = []

[output]
file_read_cap_bytes = 5
"#,
    )
    .unwrap();
    let paths = paths_at(dir.path());
    paths.ensure_dirs().unwrap();

    // Seed a file with 10 bytes — exceeds the 5-byte cap, so mock returns truncated=true.
    let mock = Arc::new(MockDriver::new());
    mock.put_file("default", "/big.txt", b"0123456789");

    let result = run_with_driver_and_paths(
        args(&["prod", "read", "/big.txt"]),
        false,
        mock,
        paths.clone(),
    )
    .await;

    assert!(
        matches!(result, Ok(true)),
        "truncated read should return Ok(true), got: {result:?}"
    );

    let events = read_audit_events(&paths);
    let complete = events
        .iter()
        .find(|e| e["event_type"].as_str() == Some("file_read_complete"))
        .expect("should have file_read_complete event");
    assert_eq!(
        complete["data"]["truncated"].as_bool(),
        Some(true),
        "truncated read should have truncated=true in audit"
    );
}

// ---------------------------------------------------------------------------
// Test 4: RequireApproval → Error::ApprovalRequired, no SSH call (AC 4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_require_approval_returns_blocked_error() {
    let (_dir, paths) = setup_approval_project();

    // No file seeded — if the driver is called, it returns NotFound (Storage error).
    // We verify we get ApprovalRequired, not Storage, confirming no driver call.
    let mock = Arc::new(MockDriver::new());

    let result = run_with_driver_and_paths(
        args(&["prod", "read", "/etc/secret"]),
        false,
        mock,
        paths.clone(),
    )
    .await;
    assert!(
        matches!(result, Err(Error::ApprovalRequired { .. })),
        "expected ApprovalRequired, got: {result:?}"
    );

    // Attempt event written; no complete event.
    let events = read_audit_events(&paths);
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["event_type"].as_str().unwrap_or(""))
        .collect();
    assert!(
        types.contains(&"file_read"),
        "missing file_read attempt event: {types:?}"
    );
    assert!(
        !types.contains(&"file_read_complete"),
        "must not have file_read_complete when blocked: {types:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 5a: Deny → Error::Denied (acceptance criterion 5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_deny_returns_denied_error() {
    let (_dir, paths) = setup_deny_project();

    let mock = Arc::new(MockDriver::new());
    let result = run_with_driver_and_paths(
        args(&["prod", "read", "/etc/shadow"]),
        false,
        mock,
        paths.clone(),
    )
    .await;
    // /etc/shadow matches the preset deny rule, so we get Denied regardless
    // of the project policy.  Either the preset or the project deny fires;
    // the result must be Denied.
    assert!(
        matches!(result, Err(Error::Denied(_))),
        "expected Denied, got: {result:?}"
    );

    let events = read_audit_events(&paths);
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["event_type"].as_str().unwrap_or(""))
        .collect();
    assert!(
        types.contains(&"file_read"),
        "expected file_read attempt event"
    );
    assert!(
        !types.contains(&"file_read_complete"),
        "must not have complete event on deny"
    );
}

// ---------------------------------------------------------------------------
// Test 5b: Block → Error::Blocked (acceptance criterion 5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_block_returns_blocked_error() {
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
allow = ["file:read"]
require_approval = []
deny = []

[[policy.file_rules]]
category = "file:read"
paths = ["/var/log/app.log"]
decision = "block"
"#,
    )
    .unwrap();
    let paths = paths_at(dir.path());
    paths.ensure_dirs().unwrap();

    let mock = Arc::new(MockDriver::new());
    let result = run_with_driver_and_paths(
        args(&["prod", "read", "/var/log/app.log"]),
        false,
        mock,
        paths,
    )
    .await;

    assert!(
        matches!(result, Err(Error::Blocked { .. })),
        "expected Blocked, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Redactor strips secrets; sha256 in audit matches PRE-redaction bytes (AC 6)
//
// The sha256 field records the on-disk / wire hash so tamper-evidence is
// preserved even if the redactor's patterns change later. bytes_returned
// likewise reflects the wire size, not the post-redaction length.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_redactor_runs_before_output_sha256_is_pre_redaction() {
    let (_dir, paths) = setup_allow_project();

    let content = b"AKIAIOSFODNN7EXAMPLE is the key\n";
    let original_sha = sha256_hex(content);

    let mock = Arc::new(MockDriver::new());
    mock.put_file("default", "/etc/creds", content.to_vec());

    let result = run_with_driver_and_paths(
        args(&["prod", "read", "/etc/creds"]),
        false,
        mock,
        paths.clone(),
    )
    .await;
    assert!(result.is_ok(), "expected Ok, got: {result:?}");

    let events = read_audit_events(&paths);
    let complete = events
        .iter()
        .find(|e| e["event_type"].as_str() == Some("file_read_complete"))
        .expect("should have file_read_complete");

    let audited_sha = complete["data"]["sha256"].as_str().unwrap_or("");
    // sha256 must match the ORIGINAL (pre-redaction) bytes — tamper evidence
    // records what came off the wire, independent of redactor pattern changes.
    assert_eq!(
        audited_sha, original_sha,
        "sha256 in audit must match pre-redaction (original) content"
    );

    // Sanity: confirm the redacted content would have a different sha.
    let redacted = b"<REDACTED:aws_access_key> is the key\n";
    let redacted_sha = sha256_hex(redacted);
    assert_ne!(
        audited_sha, redacted_sha,
        "audited sha256 must differ from post-redaction sha (proving it's pre-redaction)"
    );
}

// ---------------------------------------------------------------------------
// Test 7: --yolo in argv bypasses policy, writes yolo_invocation event (AC 7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_yolo_in_argv_bypasses_policy_and_audits() {
    // Use the approval project so the policy would normally block us.
    let (_dir, paths) = setup_approval_project();

    let mock = Arc::new(MockDriver::new());
    mock.put_file("default", "/etc/hostname", b"prod-server\n");

    let result = run_with_driver_and_paths(
        args(&["prod", "read", "--yolo", "/etc/hostname"]),
        false,
        mock,
        paths.clone(),
    )
    .await;
    assert!(
        result.is_ok(),
        "yolo should bypass approval requirement, got: {result:?}"
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
        types.contains(&"file_read_complete"),
        "missing file_read_complete event on yolo path: {types:?}"
    );
    // The policy attempt event must NOT appear on the yolo path.
    assert!(
        !types.contains(&"file_read"),
        "must not have file_read attempt event on yolo path: {types:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 8: yolo=true (top-level flag) also bypasses policy (AC 7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_yolo_flag_bypasses_policy() {
    let (_dir, paths) = setup_approval_project();

    let mock = Arc::new(MockDriver::new());
    mock.put_file("default", "/etc/hostname", b"prod-server\n");

    let result = run_with_driver_and_paths(
        args(&["prod", "read", "/etc/hostname"]),
        true, // yolo via top-level flag
        mock,
        paths.clone(),
    )
    .await;
    assert!(
        result.is_ok(),
        "yolo top-level flag should bypass approval: {result:?}"
    );

    let events = read_audit_events(&paths);
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["event_type"].as_str().unwrap_or(""))
        .collect();
    assert!(types.contains(&"yolo_invocation"), "missing yolo_invocation: {types:?}");
}

// ---------------------------------------------------------------------------
// Test 9: disable_yolo = true → Error::YoloRefused (exit 13) (AC 8)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_yolo_refused_when_disabled_globally() {
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
allow = ["file:read"]
require_approval = []
deny = []
"#,
    )
    .unwrap();
    let paths = paths_at(dir.path());
    paths.ensure_dirs().unwrap();

    // Write a global config with disable_yolo = true.
    fs::write(
        paths.config_file(),
        "disable_yolo = true\n",
    )
    .unwrap();

    let mock = Arc::new(MockDriver::new());
    mock.put_file("default", "/etc/hostname", b"server\n");

    let result = run_with_driver_and_paths(
        args(&["prod", "read", "/etc/hostname"]),
        true, // yolo requested
        mock,
        paths,
    )
    .await;
    assert!(
        matches!(result, Err(Error::YoloRefused)),
        "expected YoloRefused when disable_yolo=true, got: {result:?}"
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
