CREATE TABLE IF NOT EXISTS events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    byte_offset     INTEGER NOT NULL,
    timestamp       TEXT    NOT NULL,
    event_type      TEXT    NOT NULL,
    project         TEXT,
    target          TEXT,
    decision        TEXT,
    exit_code       INTEGER,
    raw_json        TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_timestamp           ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_project_timestamp   ON events(project, timestamp);
CREATE INDEX IF NOT EXISTS idx_events_event_type_ts       ON events(event_type, timestamp);
CREATE INDEX IF NOT EXISTS idx_events_decision_timestamp  ON events(decision, timestamp);

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
