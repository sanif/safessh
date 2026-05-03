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
