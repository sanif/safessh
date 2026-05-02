//! Watcher tests — confirm pending/projects/audit writes fire the right
//! `FsEvent` variant within the 800ms tolerance.

use safessh_storage::paths::Paths;
use safessh_tui::event::FsEvent;
use safessh_tui::watcher::start_watcher;
use std::time::Duration;
use tokio::sync::mpsc;

fn setup() -> (tempfile::TempDir, Paths) {
    let tmp = tempfile::tempdir().unwrap();
    // Construct Paths directly so parallel tests don't race on the
    // process-wide SAFESSH_HOME env var.
    let paths = Paths {
        config: tmp.path().join("config"),
        state: tmp.path().join("state"),
        cache: tmp.path().join("cache"),
    };
    paths.ensure_dirs().unwrap();
    (tmp, paths)
}

async fn drain_one(rx: &mut mpsc::Receiver<FsEvent>) -> Option<FsEvent> {
    tokio::time::timeout(Duration::from_millis(1500), rx.recv())
        .await
        .ok()
        .flatten()
}

/// Drain any backlog events the watcher accumulated during its first
/// debounce window. macOS FSEvents emits backlog events for recently-modified
/// subdirs (e.g. the `ensure_dirs()` calls in `setup()`); without this drain
/// the very first test write races with those backlogged events inside the
/// 200ms debounce window and the channel can deliver them in either order.
async fn drain_backlog(rx: &mut mpsc::Receiver<FsEvent>) {
    while tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .ok()
        .flatten()
        .is_some()
    {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_write_triggers_approvals_event() {
    let (_tmp, paths) = setup();
    let (tx, mut rx) = mpsc::channel(8);
    let _guard = start_watcher(&paths, tx).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain_backlog(&mut rx).await;

    let pending = paths.approvals_dir().join("pending/test.toml");
    std::fs::write(&pending, "token = \"abc\"\n").unwrap();

    let ev = drain_one(&mut rx).await;
    assert!(
        matches!(ev, Some(FsEvent::ApprovalsChanged)),
        "expected ApprovalsChanged, got: {ev:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn project_write_triggers_projects_event() {
    let (_tmp, paths) = setup();
    let (tx, mut rx) = mpsc::channel(8);
    let _guard = start_watcher(&paths, tx).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain_backlog(&mut rx).await;

    let f = paths.projects_dir().join("demo.toml");
    std::fs::write(&f, "name = \"demo\"\n").unwrap();

    let ev = drain_one(&mut rx).await;
    assert!(
        matches!(ev, Some(FsEvent::ProjectsChanged)),
        "expected ProjectsChanged, got: {ev:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_append_triggers_audit_event() {
    let (_tmp, paths) = setup();
    let (tx, mut rx) = mpsc::channel(8);
    let _guard = start_watcher(&paths, tx).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain_backlog(&mut rx).await;

    let f = paths.audit_log();
    std::fs::write(&f, "{\"event\":\"x\"}\n").unwrap();

    // Some platforms also report a directory-level event; we only assert
    // that *at least one* AuditAppended arrives within the window.
    let mut saw_audit = false;
    let deadline = std::time::Instant::now() + Duration::from_millis(2000);
    while std::time::Instant::now() < deadline {
        match drain_one(&mut rx).await {
            Some(FsEvent::AuditAppended) => {
                saw_audit = true;
                break;
            }
            Some(_) => continue,
            None => break,
        }
    }
    assert!(saw_audit, "expected AuditAppended within 2s");
}

/// 5 rapid pending writes coalesce into ≤2 events thanks to the 200ms
/// debouncer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn debouncer_collapses_burst() {
    let (_tmp, paths) = setup();
    let (tx, mut rx) = mpsc::channel(16);
    let _guard = start_watcher(&paths, tx).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain_backlog(&mut rx).await;

    for i in 0..5 {
        let f = paths
            .approvals_dir()
            .join(format!("pending/burst-{i}.toml"));
        std::fs::write(&f, "x = 1\n").unwrap();
    }
    // Wait past one debounce window for events to flush.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut count = 0;
    while let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
        count += 1;
    }
    assert!(count > 0, "expected at least one ApprovalsChanged");
    assert!(
        count <= 2,
        "burst should collapse to ≤2 events, got {count}"
    );
}
