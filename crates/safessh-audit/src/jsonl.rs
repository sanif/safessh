//! JSONL audit writer with redaction, fsync, and rotation.

use safessh_core::error::{Error, Result};
use safessh_core::redactor::Redactor;
use safessh_core::types::AuditEvent;
use safessh_storage::paths::Paths;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// Rotate when the active log reaches 100 MiB.
const ROTATE_AT: u64 = 100 * 1024 * 1024;

/// Append-only JSONL writer for audit events.
///
/// All events are passed through `core::Redactor` before they hit disk, the
/// log file is created with mode `0600` on Unix, every record is fsynced
/// individually, and the file rotates to `audit.log.<utc-timestamp>` once it
/// crosses [`ROTATE_AT`] bytes.
pub struct AuditWriter {
    path: PathBuf,
    redactor: Redactor,
}

impl AuditWriter {
    /// Open (or implicitly create on first append) the audit log under the
    /// state directory described by `paths`.
    pub fn open(paths: &Paths) -> Result<Self> {
        let path = paths.audit_log();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::AuditWriteFailed(format!("mkdir: {e}")))?;
        }
        Ok(Self {
            path,
            redactor: Redactor::default(),
        })
    }

    /// Append one event as a JSON line, redacted, with `fsync`.
    ///
    // SAFETY-INVARIANT-4: Audit append must succeed before any user-visible
    // output for events that gate disclosure (exec attempts, approvals,
    // yolo). Any failure here returns `Error::AuditWriteFailed`, which the
    // CLI maps to exit code 50 — callers must not swallow this error.
    pub fn append(&self, event: &AuditEvent) -> Result<()> {
        self.maybe_rotate()?;

        let json = serde_json::to_string(event)
            .map_err(|e| Error::AuditWriteFailed(format!("serialize: {e}")))?;
        let (redacted, _counts) = self.redactor.redact(json.as_bytes());

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| Error::AuditWriteFailed(format!("open: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Best-effort: tightening mode on every append is cheap and
            // ensures a freshly created file is never world-readable.
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }

        file.write_all(&redacted)
            .map_err(|e| Error::AuditWriteFailed(format!("write: {e}")))?;
        file.write_all(b"\n")
            .map_err(|e| Error::AuditWriteFailed(format!("write nl: {e}")))?;
        file.sync_all()
            .map_err(|e| Error::AuditWriteFailed(format!("fsync: {e}")))?;

        Ok(())
    }

    /// If the active log has reached the rotation threshold, rename it so the
    /// next append starts fresh.
    fn maybe_rotate(&self) -> Result<()> {
        if let Ok(meta) = std::fs::metadata(&self.path) {
            if meta.len() >= ROTATE_AT {
                let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S");
                let rotated = self.path.with_file_name(format!("audit.log.{ts}"));
                std::fs::rename(&self.path, &rotated)
                    .map_err(|e| Error::AuditWriteFailed(format!("rotate: {e}")))?;
            }
        }
        Ok(())
    }
}
