//! Audit event constructors.
//!
//! Re-exports `AuditEvent` from `safessh-core` and provides typed helper
//! constructors for the event variants emitted by the runtime.

pub use safessh_core::types::AuditEvent;

use safessh_core::types::{ParsedCommand, PolicyDecision};
use serde_json::json;

/// Build an `exec_attempt` audit event. `target` is the resolved target name
/// (when known) so `audit query --target` can match exec events. The field is
/// additive: omitted from the emitted JSON when `None`, so older lines without
/// it still parse fine and `schema_version` stays at 1.
pub fn exec_attempt(
    project: &str,
    parsed: &ParsedCommand,
    decision: &str,
    target: Option<&str>,
) -> AuditEvent {
    let mut e = AuditEvent::new("exec_attempt");
    e.project = Some(project.into());
    let mut data = serde_json::Map::new();
    data.insert("raw".into(), serde_json::Value::String(parsed.raw.clone()));
    data.insert(
        "binary".into(),
        serde_json::Value::String(parsed.binary.clone()),
    );
    data.insert(
        "flags".into(),
        serde_json::to_value(&parsed.flags).unwrap_or_default(),
    );
    data.insert(
        "args".into(),
        serde_json::to_value(&parsed.args).unwrap_or_default(),
    );
    data.insert(
        "decision".into(),
        serde_json::Value::String(decision.into()),
    );
    if let Some(t) = target {
        data.insert("target".into(), serde_json::Value::String(t.into()));
    }
    e.data = serde_json::Value::Object(data);
    e
}

/// Build an `exec_complete` audit event. `target` is the resolved target name
/// (when known); additive — omitted when `None`.
pub fn exec_complete(
    project: &str,
    exit: i32,
    stdout_bytes: u64,
    stderr_bytes: u64,
    duration_ms: u64,
    target: Option<&str>,
) -> AuditEvent {
    let mut e = AuditEvent::new("exec_complete");
    e.project = Some(project.into());
    let mut data = serde_json::Map::new();
    data.insert("exit_code".into(), json!(exit));
    data.insert("stdout_bytes".into(), json!(stdout_bytes));
    data.insert("stderr_bytes".into(), json!(stderr_bytes));
    data.insert("duration_ms".into(), json!(duration_ms));
    if let Some(t) = target {
        data.insert("target".into(), serde_json::Value::String(t.into()));
    }
    e.data = serde_json::Value::Object(data);
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

/// Build a `tunnel_open` audit event. Carries the `opacity_warning` string
/// so anyone tailing the audit log sees the warning inline.
pub fn tunnel_open(
    project: &str,
    target: &str,
    id: &safessh_core::tunnel::TunnelId,
    spec: &safessh_core::tunnel::TunnelSpec,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> AuditEvent {
    let mut e = AuditEvent::new("tunnel_open");
    e.project = Some(project.to_string());
    e.data = json!({
        "id": id.as_str(),
        "target": target,
        "local_port": spec.local_port,
        "remote_host": spec.remote_host,
        "remote_port": spec.remote_port,
        "expires_at": expires_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "opacity_warning": "tunnel traffic is opaque to safessh",
    });
    e
}

/// Build a `tunnel_close` audit event. `duration_secs` is the wall-clock
/// span between `tunnel_open` and the close, set by the supervisor.
pub fn tunnel_close(
    project: &str,
    id: &safessh_core::tunnel::TunnelId,
    reason: safessh_core::tunnel::TunnelCloseReason,
    duration_secs: u64,
) -> AuditEvent {
    let mut e = AuditEvent::new("tunnel_close");
    e.project = Some(project.to_string());
    let reason_str = serde_json::to_value(reason)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "unknown".into());
    e.data = json!({
        "id": id.as_str(),
        "reason": reason_str,
        "duration_secs": duration_secs,
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
