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
