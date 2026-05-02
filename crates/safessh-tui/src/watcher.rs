//! Notify-based filesystem watcher for the TUI.
//!
//! SAFETY-INVARIANT-12: events are *signals to re-read*, never carry
//! file contents. Screens always re-load through the storage API
//! (`PendingStore`, `ProjectStore`, `AuditWriter::tail`) which holds an
//! advisory lock and reads atomically. The watcher is just a wakeup.

use crate::event::FsEvent;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use safessh_core::error::{Error, Result};
use safessh_storage::paths::Paths;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

/// Owns the underlying debouncer; dropping it stops the watch threads.
pub struct WatcherGuard(#[allow(dead_code)] Debouncer<notify::RecommendedWatcher>);

/// Start watching `pending/`, `projects/`, and `audit.log`'s parent dir.
/// Bursts within a 200ms window collapse into a single event per kind.
pub fn start_watcher(paths: &Paths, tx: Sender<FsEvent>) -> Result<WatcherGuard> {
    let pending_dir = paths.approvals_dir().join("pending");
    let projects_dir = paths.projects_dir();
    let audit_path = paths.audit_log();
    let audit_parent = audit_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| paths.state.clone());

    std::fs::create_dir_all(&pending_dir).ok();
    std::fs::create_dir_all(&projects_dir).ok();
    std::fs::create_dir_all(&audit_parent).ok();

    // Canonicalize so the matchers compare against the same realpath
    // notify reports — on macOS, `/var/folders/...` arrives as
    // `/private/var/folders/...` because /var → /private/var.
    let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let pending_match = canon(&pending_dir);
    let projects_match = canon(&projects_dir);
    let audit_match = canon(&audit_parent).join(
        audit_path
            .file_name()
            .map(std::ffi::OsStr::to_os_string)
            .unwrap_or_default(),
    );

    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        move |res: DebounceEventResult| {
            let Ok(events) = res else { return };
            let mut saw_pending = false;
            let mut saw_projects = false;
            let mut saw_audit = false;
            for e in events {
                let p = &e.path;
                if p.starts_with(&pending_match) {
                    saw_pending = true;
                } else if p.starts_with(&projects_match) {
                    saw_projects = true;
                } else if p == &audit_match {
                    saw_audit = true;
                }
            }
            // SAFETY-INVARIANT-12: each FsEvent is a signal only; the
            // receiving screen re-reads through the storage API.
            if saw_pending {
                let _ = tx.try_send(FsEvent::ApprovalsChanged);
            }
            if saw_projects {
                let _ = tx.try_send(FsEvent::ProjectsChanged);
            }
            if saw_audit {
                let _ = tx.try_send(FsEvent::AuditAppended);
            }
        },
    )
    .map_err(|e| Error::Storage(format!("watcher init: {e}")))?;

    debouncer
        .watcher()
        .watch(&pending_dir, RecursiveMode::NonRecursive)
        .map_err(|e| Error::Storage(format!("watch pending: {e}")))?;
    debouncer
        .watcher()
        .watch(&projects_dir, RecursiveMode::NonRecursive)
        .map_err(|e| Error::Storage(format!("watch projects: {e}")))?;
    // audit.log may not exist on first launch — watch its parent dir so
    // the first append fires a CREATE we can attribute.
    debouncer
        .watcher()
        .watch(&audit_parent, RecursiveMode::NonRecursive)
        .map_err(|e| Error::Storage(format!("watch audit dir: {e}")))?;

    Ok(WatcherGuard(debouncer))
}
