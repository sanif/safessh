//! Canonical error type for the safessh workspace.
//!
//! Each variant maps to a stable exit code (per spec §7.1) and a stable
//! `error_class` string used for audit logging.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("usage: {0}")]
    Usage(String),

    #[error("approval required: token={token}")]
    ApprovalRequired {
        token: String,
        categories: Vec<String>,
    },

    #[error("blocked by rule {rule_id}: {reason}")]
    Blocked { rule_id: String, reason: String },

    #[error("denied: {0}")]
    Denied(String),

    #[error("yolo refused: disabled globally")]
    YoloRefused,

    #[error("ssh failure: {0}")]
    Ssh(String),

    #[error("connection failure: {0}")]
    Connection(String),

    #[error("output exceeded cap ({limit_bytes} bytes)")]
    OutputCapped { limit_bytes: u64 },

    #[error("storage error: {0}")]
    Storage(String),

    #[error("audit write failure: {0}")]
    AuditWriteFailed(String),

    #[error("audit index built by newer safessh; ignoring index, JSONL is unaffected")]
    AuditIndexNewer,

    #[error("audit index error: {0}")]
    AuditIndexFailed(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde: {0}")]
    Serde(String),
}

impl Error {
    /// Map to the documented exit code from spec §7.1.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Config(_) | Error::ProjectNotFound(_) => 1,
            Error::Usage(_) => 2,
            Error::ApprovalRequired { .. } => 10,
            Error::Blocked { .. } => 11,
            Error::Denied(_) => 12,
            Error::YoloRefused => 13,
            Error::Ssh(_) => 20,
            Error::Connection(_) => 21,
            Error::OutputCapped { .. } => 30,
            Error::Storage(_) | Error::Io(_) | Error::Serde(_) => 40,
            Error::AuditWriteFailed(_) => 50,
            Error::AuditIndexNewer | Error::AuditIndexFailed(_) => 40,
        }
    }

    /// Stable string identifier for audit logging.
    pub fn error_class(&self) -> &'static str {
        match self {
            Error::Config(_) => "config",
            Error::ProjectNotFound(_) => "project_not_found",
            Error::Usage(_) => "usage",
            Error::ApprovalRequired { .. } => "approval_required",
            Error::Blocked { .. } => "blocked",
            Error::Denied(_) => "denied",
            Error::YoloRefused => "yolo_refused",
            Error::Ssh(_) => "ssh_failure",
            Error::Connection(_) => "connection_failure",
            Error::OutputCapped { .. } => "output_capped",
            Error::Storage(_) => "storage",
            Error::Io(_) => "io",
            Error::Serde(_) => "serde",
            Error::AuditWriteFailed(_) => "audit_write_failed",
            Error::AuditIndexNewer => "audit_index_newer",
            Error::AuditIndexFailed(_) => "audit_index_failed",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
