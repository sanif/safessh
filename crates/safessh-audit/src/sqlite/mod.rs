//! SQLite index over the JSONL audit log.
//!
// SAFETY-INVARIANT-4: this module is **read-side only**. It never participates
// in the audit-write path. Exec / file / tunnel paths write JSONL via
// `AuditWriter::append` and return without touching SQLite. If anything in
// here fails, the JSONL log is unaffected and callers must degrade to
// log-scan rather than blocking.

use refinery::embed_migrations;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use safessh_core::error::{Error, Result};
use safessh_storage::paths::Paths;
use std::path::PathBuf;

embed_migrations!("src/sqlite/migrations");

const SCHEMA_VERSION: i64 = 1;

pub struct Index {
    conn: Connection,
    log_path: PathBuf,
    db_path: PathBuf,
}

impl Index {
    pub fn open_or_create(paths: &Paths) -> Result<Self> {
        let db_path = paths.audit_db();
        let log_path = paths.audit_log();

        match Self::try_open(&db_path, &log_path) {
            Ok(idx) => Ok(idx),
            Err(Error::AuditIndexNewer) => Err(Error::AuditIndexNewer),
            Err(_) => {
                let _ = std::fs::remove_file(&db_path);
                Self::try_open(&db_path, &log_path)
            }
        }
    }

    fn try_open(db_path: &std::path::Path, log_path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }

        let mut conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(map_rusqlite)?;

        migrations::runner()
            .run(&mut conn)
            .map_err(|e| Error::AuditIndexFailed(format!("migrate: {e}")))?;

        let stored_version = read_meta_int(&conn, "schema_version")?.unwrap_or(SCHEMA_VERSION);
        if stored_version > SCHEMA_VERSION {
            return Err(Error::AuditIndexNewer);
        }

        let log_path_str = log_path.to_string_lossy().to_string();
        let prior_source = read_meta_str(&conn, "source_file")?;
        if prior_source.as_deref() != Some(log_path_str.as_str()) {
            conn.execute("DELETE FROM events", []).map_err(map_rusqlite)?;
            write_meta(&conn, "last_indexed_offset", "0")?;
            write_meta(&conn, "source_file", &log_path_str)?;
        }
        write_meta(&conn, "schema_version", &SCHEMA_VERSION.to_string())?;
        if read_meta_str(&conn, "last_indexed_offset")?.is_none() {
            write_meta(&conn, "last_indexed_offset", "0")?;
        }

        Ok(Self {
            conn,
            log_path: log_path.to_path_buf(),
            db_path: db_path.to_path_buf(),
        })
    }

    pub fn last_indexed_offset(&self) -> Result<u64> {
        Ok(read_meta_str(&self.conn, "last_indexed_offset")?
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    }

    pub fn db_path(&self) -> &std::path::Path {
        &self.db_path
    }

    pub fn log_path(&self) -> &std::path::Path {
        &self.log_path
    }

    #[allow(dead_code)]
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    #[allow(dead_code)]
    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

fn read_meta_int(conn: &Connection, key: &str) -> Result<Option<i64>> {
    Ok(read_meta_str(conn, key)?.and_then(|s| s.parse().ok()))
}

fn read_meta_str(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
        r.get::<_, String>(0)
    })
    .optional()
    .map_err(map_rusqlite)
}

pub(crate) fn write_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )
    .map_err(map_rusqlite)?;
    Ok(())
}

pub(crate) fn map_rusqlite(e: rusqlite::Error) -> Error {
    Error::AuditIndexFailed(format!("sqlite: {e}"))
}
