//! Audit event constructors.
//!
//! Re-exports `AuditEvent` from `safessh-core` and provides typed helper
//! constructors for the event variants emitted by the runtime.

pub use safessh_core::types::AuditEvent;

use safessh_core::types::ParsedCommand;
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
