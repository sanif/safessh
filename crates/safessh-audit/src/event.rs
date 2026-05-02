//! Audit event constructors.
//!
//! Re-exports `AuditEvent` from `safessh-core` and provides typed helper
//! constructors for the event variants emitted by the runtime.

pub use safessh_core::types::AuditEvent;

use safessh_core::types::{ParsedCommand, PolicyDecision};
use serde_json::json;

/// Build an `exec_attempt` audit event.
pub fn exec_attempt(project: &str, parsed: &ParsedCommand, decision: &str) -> AuditEvent {
    let mut e = AuditEvent::new("exec_attempt");
    e.project = Some(project.into());
    e.data = json!({
        "raw": parsed.raw,
        "binary": parsed.binary,
        "flags": parsed.flags,
        "args": parsed.args,
        "decision": decision,
    });
    e
}

/// Build an `exec_complete` audit event.
pub fn exec_complete(
    project: &str,
    exit: i32,
    stdout_bytes: u64,
    stderr_bytes: u64,
    duration_ms: u64,
) -> AuditEvent {
    let mut e = AuditEvent::new("exec_complete");
    e.project = Some(project.into());
    e.data = json!({
        "exit_code": exit,
        "stdout_bytes": stdout_bytes,
        "stderr_bytes": stderr_bytes,
        "duration_ms": duration_ms,
    });
    e
}

/// Build an `approval_requested` audit event.
pub fn approval_requested(
    project: &str,
    token: &str,
    categories: &[String],
    raw: &str,
) -> AuditEvent {
    let mut e = AuditEvent::new("approval_requested");
    e.project = Some(project.into());
    e.data = json!({
        "token": token,
        "categories": categories,
        "raw": raw,
    });
    e
}

/// Build a `yolo_invocation` audit event.
pub fn yolo_invocation(project: &str, raw: &str) -> AuditEvent {
    let mut e = AuditEvent::new("yolo_invocation");
    e.project = Some(project.into());
    e.data = json!({ "raw": raw, "flagged": true });
    e
}

/// Build a `file_read` or `file_write` attempt audit event.
///
/// `event_type` should be `"file_read"` or `"file_write"`. Written
/// **before** any user-visible output (SAFETY-INVARIANT-4 by callers).
pub fn file_attempt(
    event_type: &str,
    project: &str,
    path: &str,
    decision: &PolicyDecision,
) -> AuditEvent {
    let mut e = AuditEvent::new(event_type);
    e.project = Some(project.to_string());
    e.data = json!({
        "path": path,
        "decision": decision_label(decision),
    });
    e
}

/// Build a `file_read_complete` audit event.
pub fn file_read_complete(
    project: &str,
    target: &str,
    path: &str,
    bytes_returned: u64,
    sha256: &str,
    truncated: bool,
    duration_ms: u64,
) -> AuditEvent {
    let mut e = AuditEvent::new("file_read_complete");
    e.project = Some(project.to_string());
    e.data = json!({
        "target": target,
        "path": path,
        "bytes_returned": bytes_returned,
        "sha256": sha256,
        "truncated": truncated,
        "duration_ms": duration_ms,
    });
    e
}

/// Build a `file_write_complete` audit event.
pub fn file_write_complete(
    project: &str,
    target: &str,
    path: &str,
    bytes_written: u64,
    sha256: &str,
    truncated: bool,
    duration_ms: u64,
) -> AuditEvent {
    let mut e = AuditEvent::new("file_write_complete");
    e.project = Some(project.to_string());
    e.data = json!({
        "target": target,
        "path": path,
        "bytes_written": bytes_written,
        "sha256": sha256,
        "truncated": truncated,
        "duration_ms": duration_ms,
    });
    e
}

fn decision_label(d: &PolicyDecision) -> &'static str {
    match d {
        PolicyDecision::Allow { .. } => "allow",
        PolicyDecision::RequireApproval { .. } => "require_approval",
        PolicyDecision::Deny { .. } => "deny",
        PolicyDecision::Block { .. } => "block",
    }
}
