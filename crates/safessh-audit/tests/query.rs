use safessh_audit::query::{query, Filters, Row};
use safessh_audit::sqlite::Index;
use safessh_storage::paths::Paths;
use std::io::Write;
use tempfile::tempdir;

fn paths_in(dir: &std::path::Path) -> Paths {
    Paths {
        config: dir.join("config"),
        state: dir.join("state"),
        cache: dir.join("cache"),
    }
}

fn append(path: &std::path::Path, json: serde_json::Value) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(f, "{json}").unwrap();
}

fn fixture() -> (tempfile::TempDir, Paths) {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    let log = paths.audit_log();
    append(
        &log,
        serde_json::json!({
            "schema_version":1,"timestamp":"2026-05-01T10:00:00Z","event_type":"exec_attempt",
            "project":"prod","data":{"target":"web","decision":"allow"}
        }),
    );
    append(
        &log,
        serde_json::json!({
            "schema_version":1,"timestamp":"2026-05-02T10:00:00Z","event_type":"exec_complete",
            "project":"prod","data":{"target":"web","exit_code":1}
        }),
    );
    append(
        &log,
        serde_json::json!({
            "schema_version":1,"timestamp":"2026-05-03T10:00:00Z","event_type":"exec_attempt",
            "project":"dev","data":{"target":"db","decision":"deny"}
        }),
    );
    (dir, paths)
}

#[test]
fn empty_db_returns_no_rows() {
    let dir = tempdir().unwrap();
    let paths = paths_in(dir.path());
    std::fs::create_dir_all(&paths.state).unwrap();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let rows = query(&mut idx, &Filters::default()).unwrap();
    assert!(rows.is_empty());
}

#[test]
fn empty_filter_returns_all_newest_first() {
    let (_t, paths) = fixture();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let rows: Vec<Row> = query(&mut idx, &Filters::default()).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].project.as_deref(), Some("dev"));
    assert_eq!(rows[2].project.as_deref(), Some("prod"));
}

#[test]
fn filter_by_project_and_decision() {
    let (_t, paths) = fixture();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let f = Filters {
        project: Some("prod".into()),
        decision: Some("allow".into()),
        ..Filters::default()
    };
    let rows = query(&mut idx, &f).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "exec_attempt");
}

#[test]
fn filter_by_exit_code_range() {
    let (_t, paths) = fixture();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let f = Filters {
        exit_code: Some((1, 255)),
        ..Filters::default()
    };
    let rows = query(&mut idx, &f).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "exec_complete");
}

#[test]
fn filter_by_target() {
    let (_t, paths) = fixture();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let f = Filters {
        target: Some("db".into()),
        ..Filters::default()
    };
    let rows = query(&mut idx, &f).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].project.as_deref(), Some("dev"));
}

#[test]
fn since_until_window_filters_inclusive() {
    let (_t, paths) = fixture();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let f = Filters {
        since: Some("2026-05-02T00:00:00Z".into()),
        until: Some("2026-05-02T23:59:59Z".into()),
        ..Filters::default()
    };
    let rows = query(&mut idx, &f).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "exec_complete");
}

#[test]
fn limit_zero_means_unlimited() {
    let (_t, paths) = fixture();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let f = Filters {
        limit: 0,
        ..Filters::default()
    };
    assert_eq!(query(&mut idx, &f).unwrap().len(), 3);
}

#[test]
fn limit_caps_results() {
    let (_t, paths) = fixture();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let f = Filters {
        limit: 2,
        ..Filters::default()
    };
    assert_eq!(query(&mut idx, &f).unwrap().len(), 2);
}

#[test]
fn grep_filters_against_raw_json() {
    let (_t, paths) = fixture();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let f = Filters {
        grep: Some("\"db\"".into()),
        ..Filters::default()
    };
    let rows = query(&mut idx, &f).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].project.as_deref(), Some("dev"));
}

#[test]
fn filter_by_event_type() {
    let (_t, paths) = fixture();
    let mut idx = Index::open_or_create(&paths).unwrap();
    let f = Filters {
        event_type: Some("exec_complete".into()),
        ..Filters::default()
    };
    let rows = query(&mut idx, &f).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].project.as_deref(), Some("prod"));
}
