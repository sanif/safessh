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

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    #[allow(dead_code)]
    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Read JSONL from `last_indexed_offset` to EOF and INSERT new rows.
    /// Returns the number of rows successfully inserted (parse failures are
    /// skipped silently, but the offset still advances).
    ///
    /// If the log shrank or its prefix changed (rotation/truncation),
    /// resets offset to 0 first.
    pub fn catch_up(&mut self) -> Result<usize> {
        use std::io::{BufRead, BufReader, Seek, SeekFrom};

        if !self.log_path.exists() {
            return Ok(0);
        }
        let log_size = std::fs::metadata(&self.log_path)
            .map_err(Error::Io)?
            .len();

        let current_fingerprint = read_log_fingerprint(&self.log_path)?;
        let stored_fingerprint = read_meta_str(&self.conn, "log_fingerprint")?;

        let mut offset = self.last_indexed_offset()?;
        let rotated = offset > log_size
            || stored_fingerprint
                .as_deref()
                .is_some_and(|f| f != current_fingerprint);
        if rotated {
            self.conn
                .execute("DELETE FROM events", [])
                .map_err(map_rusqlite)?;
            offset = 0;
        }
        if offset == log_size {
            // Still write fingerprint in case this is the first run with content.
            write_meta(&self.conn, "log_fingerprint", &current_fingerprint)?;
            return Ok(0);
        }

        let mut file = std::fs::File::open(&self.log_path).map_err(Error::Io)?;
        file.seek(SeekFrom::Start(offset)).map_err(Error::Io)?;
        let mut reader = BufReader::new(file);

        let tx = self.conn.transaction().map_err(map_rusqlite)?;
        let mut inserted = 0usize;
        let mut current = offset;
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line).map_err(Error::Io)?;
            if n == 0 {
                break;
            }
            let line_offset = current;
            current = current.saturating_add(n as u64);

            let trimmed = line.trim_end_matches('\n');
            if trimmed.is_empty() {
                continue;
            }

            let parsed: Option<serde_json::Value> = serde_json::from_str(trimmed).ok();
            let Some(v) = parsed else { continue };

            let timestamp = v
                .get("timestamp")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let event_type = v
                .get("event_type")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let project = v.get("project").and_then(|x| x.as_str()).map(String::from);
            let target = v
                .get("data")
                .and_then(|d| d.get("target"))
                .and_then(|x| x.as_str())
                .map(String::from);
            let decision = v
                .get("data")
                .and_then(|d| d.get("decision"))
                .and_then(|x| x.as_str())
                .map(String::from);
            let exit_code = v
                .get("data")
                .and_then(|d| d.get("exit_code"))
                .and_then(|x| x.as_i64());

            tx.execute(
                "INSERT INTO events
                   (byte_offset, timestamp, event_type, project, target, decision, exit_code, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    line_offset as i64,
                    timestamp,
                    event_type,
                    project,
                    target,
                    decision,
                    exit_code,
                    trimmed,
                ],
            )
            .map_err(map_rusqlite)?;
            inserted += 1;
        }

        tx.execute(
            "INSERT INTO meta(key, value) VALUES('last_indexed_offset', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [current.to_string()],
        )
        .map_err(map_rusqlite)?;
        tx.execute(
            "INSERT INTO meta(key, value) VALUES('log_fingerprint', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [current_fingerprint.as_str()],
        )
        .map_err(map_rusqlite)?;
        tx.commit().map_err(map_rusqlite)?;
        Ok(inserted)
    }
}

/// Read up to the first 256 bytes of the log file and return them hex-encoded.
/// Used to detect rotation/truncation when file size alone is ambiguous.
fn read_log_fingerprint(path: &std::path::Path) -> Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(Error::Io)?;
    let mut buf = [0u8; 256];
    let n = f.read(&mut buf).map_err(Error::Io)?;
    let mut hex = String::with_capacity(n * 2);
    for b in &buf[..n] {
        hex.push_str(&format!("{b:02x}"));
    }
    Ok(hex)
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
