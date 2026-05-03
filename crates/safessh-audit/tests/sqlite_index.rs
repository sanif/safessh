use safessh_audit::sqlite::Index;
use safessh_core::error::Error;
use safessh_storage::paths::Paths;
use tempfile::tempdir;

fn paths_in(dir: &std::path::Path) -> Paths {
    Paths {
        config: dir.join("config"),
        state: dir.join("state"),
        cache: dir.join("cache"),
    }
}

#[test]
fn first_open_creates_db_with_meta() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    let idx = Index::open_or_create(&paths).expect("open");
    assert_eq!(idx.last_indexed_offset().unwrap(), 0);
    assert!(idx.db_path().exists());
}

#[test]
fn second_open_is_idempotent() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    let _ = Index::open_or_create(&paths).unwrap();
    let idx = Index::open_or_create(&paths).unwrap();
    assert_eq!(idx.last_indexed_offset().unwrap(), 0);
}

#[test]
fn newer_schema_returns_typed_error() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    {
        let _ = Index::open_or_create(&paths).unwrap();
    }
    let conn = rusqlite::Connection::open(paths.audit_db()).unwrap();
    conn.execute(
        "UPDATE meta SET value = '99' WHERE key = 'schema_version'",
        [],
    )
    .unwrap();
    drop(conn);

    match Index::open_or_create(&paths) {
        Err(Error::AuditIndexNewer) => {}
        Err(other) => panic!("expected AuditIndexNewer, got {other:?}"),
        Ok(_) => panic!("expected AuditIndexNewer, got Ok"),
    }
}

#[test]
fn corrupt_db_is_recovered() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    {
        let _ = Index::open_or_create(&paths).unwrap();
    }
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(paths.audit_db())
        .unwrap();
    f.set_len(4).unwrap();
    drop(f);

    let idx = Index::open_or_create(&paths).expect("recovery");
    assert_eq!(idx.last_indexed_offset().unwrap(), 0);
}

#[test]
fn source_file_change_resets_state() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();

    // First open seeds source_file = paths.audit_log().
    {
        let _ = Index::open_or_create(&paths).unwrap();
        let conn = rusqlite::Connection::open(paths.audit_db()).unwrap();
        conn.execute(
            "INSERT INTO events(byte_offset, timestamp, event_type, raw_json) \
             VALUES(0, '2026-01-01T00:00:00Z', 'fixture', '{}')",
            [],
        ).unwrap();
        conn.execute(
            "UPDATE meta SET value = '4242' WHERE key = 'last_indexed_offset'",
            [],
        ).unwrap();
    }

    // Move the DB into a new state dir whose audit_log path differs.
    let mut other = paths.clone();
    other.state = dir.path().join("state2");
    std::fs::create_dir_all(&other.state).unwrap();
    std::fs::rename(paths.audit_db(), other.audit_db()).unwrap();

    let idx = Index::open_or_create(&other).unwrap();
    assert_eq!(idx.last_indexed_offset().unwrap(), 0);

    let conn = rusqlite::Connection::open(other.audit_db()).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

use std::io::Write;

fn write_event_line(path: &std::path::Path, line: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(f, "{line}").unwrap();
}

fn ev(event_type: &str, project: &str) -> String {
    serde_json::json!({
        "schema_version": 1,
        "timestamp": "2026-05-03T00:00:00Z",
        "event_type": event_type,
        "project": project,
        "data": {},
        "error_class": null,
        "error_message": null,
    })
    .to_string()
}

#[test]
fn catch_up_indexes_events_and_advances_offset() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    for i in 0..10 {
        write_event_line(&paths.audit_log(), &ev("exec_attempt", &format!("p{i}")));
    }
    let mut idx = Index::open_or_create(&paths).unwrap();
    let inserted = idx.catch_up().unwrap();
    assert_eq!(inserted, 10);

    let count: i64 = idx
        .conn()
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 10);

    let log_size = std::fs::metadata(paths.audit_log()).unwrap().len();
    assert_eq!(idx.last_indexed_offset().unwrap(), log_size);
}

#[test]
fn catch_up_extracts_target_decision_exit_code() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    let line = serde_json::json!({
        "schema_version": 1,
        "timestamp": "2026-05-03T00:00:00Z",
        "event_type": "exec_complete",
        "project": "prod",
        "data": { "target": "web", "exit_code": 0, "stdout_bytes": 1, "stderr_bytes": 0, "duration_ms": 5 }
    })
    .to_string();
    write_event_line(&paths.audit_log(), &line);
    let line2 = serde_json::json!({
        "schema_version": 1,
        "timestamp": "2026-05-03T00:00:01Z",
        "event_type": "exec_attempt",
        "project": "prod",
        "data": { "target": "web", "decision": "allow", "raw": "ls", "binary": "ls", "flags": [], "args": [] }
    })
    .to_string();
    write_event_line(&paths.audit_log(), &line2);

    let mut idx = Index::open_or_create(&paths).unwrap();
    idx.catch_up().unwrap();
    let row: (String, Option<String>, Option<String>, Option<i64>) = idx
        .conn()
        .query_row(
            "SELECT event_type, target, decision, exit_code FROM events ORDER BY id",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(row.0, "exec_complete");
    assert_eq!(row.1.as_deref(), Some("web"));
    assert_eq!(row.3, Some(0));
}

#[test]
fn catch_up_skips_unparseable_lines_but_advances_offset() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    write_event_line(&paths.audit_log(), "not json");
    write_event_line(&paths.audit_log(), &ev("exec_attempt", "p"));

    let mut idx = Index::open_or_create(&paths).unwrap();
    let inserted = idx.catch_up().unwrap();
    assert_eq!(inserted, 1);
    let log_size = std::fs::metadata(paths.audit_log()).unwrap().len();
    assert_eq!(idx.last_indexed_offset().unwrap(), log_size);

    let inserted2 = idx.catch_up().unwrap();
    assert_eq!(inserted2, 0);
}

#[test]
fn catch_up_resets_on_log_shrink() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    write_event_line(&paths.audit_log(), &ev("exec_attempt", "old"));
    let mut idx = Index::open_or_create(&paths).unwrap();
    idx.catch_up().unwrap();

    let f = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(paths.audit_log())
        .unwrap();
    drop(f);
    write_event_line(&paths.audit_log(), &ev("exec_attempt", "new"));

    let inserted = idx.catch_up().unwrap();
    assert_eq!(inserted, 1);
    let row: String = idx
        .conn()
        .query_row(
            "SELECT project FROM events ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(row, "new");
}

#[test]
fn catch_up_handles_ten_thousand_events() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    let log = paths.audit_log();
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .unwrap();
        for i in 0..10_000 {
            writeln!(f, "{}", ev("exec_attempt", &format!("p{i}"))).unwrap();
        }
    }
    let idx_start = std::time::Instant::now();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let inserted = idx.catch_up().unwrap();
    let elapsed = idx_start.elapsed();
    eprintln!("indexed 10k events in {elapsed:?}");
    assert_eq!(inserted, 10_000);
    assert!(
        elapsed.as_secs() < 5,
        "10k catch-up took {elapsed:?}; spec target <5s"
    );
}
