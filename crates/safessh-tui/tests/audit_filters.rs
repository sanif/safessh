//! Filter tests for the AuditScreen.

use safessh_audit::{event, jsonl::AuditWriter};
use safessh_core::types::ParsedCommand;
use safessh_storage::paths::Paths;
use safessh_tui::screens::audit::{AuditScreen, EditField};

fn setup() -> (tempfile::TempDir, Paths) {
    let tmp = tempfile::tempdir().unwrap();
    let p = Paths {
        config: tmp.path().join("config"),
        state: tmp.path().join("state"),
        cache: tmp.path().join("cache"),
    };
    p.ensure_dirs().unwrap();
    (tmp, p)
}

fn parsed(binary: &str) -> ParsedCommand {
    ParsedCommand {
        binary: binary.into(),
        flags: vec![],
        args: vec![],
        redirects: vec![],
        pipes: vec![],
        env_mutations: vec![],
        raw: format!("{binary} /var"),
    }
}

#[test]
fn project_filter_excludes_other_projects() {
    let (_tmp, p) = setup();
    let w = AuditWriter::open(&p).unwrap();
    w.append(&event::exec_attempt("prod", &parsed("ls"), "allow", None))
        .unwrap();
    w.append(&event::exec_attempt("dev", &parsed("ls"), "allow", None))
        .unwrap();

    let mut s = AuditScreen::load(&p).unwrap();
    assert_eq!(s.filtered_rows().len(), 2);
    s.begin_edit(EditField::Project);
    for c in "prod".chars() {
        s.push_edit_char(c);
    }
    s.finish_edit();
    assert_eq!(s.filtered_rows().len(), 1);
    assert_eq!(s.filtered_rows()[0].project.as_deref(), Some("prod"));
}

#[test]
fn type_filter_excludes_other_types() {
    let (_tmp, p) = setup();
    let w = AuditWriter::open(&p).unwrap();
    w.append(&event::exec_attempt("prod", &parsed("ls"), "allow", None))
        .unwrap();
    w.append(&event::exec_complete("prod", 0, 100, 0, 30, None))
        .unwrap();

    let mut s = AuditScreen::load(&p).unwrap();
    s.begin_edit(EditField::Type);
    for c in "exec_attempt".chars() {
        s.push_edit_char(c);
    }
    s.finish_edit();
    let visible = s.filtered_rows();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].event_type, "exec_attempt");
}

#[test]
fn grep_filter_matches_substring() {
    let (_tmp, p) = setup();
    let w = AuditWriter::open(&p).unwrap();
    w.append(&event::exec_attempt("prod", &parsed("rm"), "allow", None))
        .unwrap();
    w.append(&event::exec_attempt("prod", &parsed("ls"), "allow", None))
        .unwrap();

    let mut s = AuditScreen::load(&p).unwrap();
    s.begin_edit(EditField::Grep);
    for c in "\"binary\":\"rm\"".chars() {
        s.push_edit_char(c);
    }
    s.finish_edit();
    let visible = s.filtered_rows();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].summary, "rm");
}

#[test]
fn rotation_triggers_full_reload() {
    let (_tmp, p) = setup();
    let w = AuditWriter::open(&p).unwrap();
    for i in 0..3 {
        w.append(&event::exec_attempt(
            "prod",
            &parsed(&format!("cmd{i}")),
            "allow",
            None,
        ))
        .unwrap();
    }
    let mut s = AuditScreen::load(&p).unwrap();
    assert_eq!(s.rows().len(), 3);

    // Truncate the log; append_tail should fall back to full_reload.
    std::fs::write(p.audit_log(), "").unwrap();
    s.append_tail().unwrap();
    assert_eq!(s.rows().len(), 0);
}

#[test]
fn empty_filter_value_clears() {
    let (_tmp, p) = setup();
    let w = AuditWriter::open(&p).unwrap();
    w.append(&event::exec_attempt("prod", &parsed("ls"), "allow", None))
        .unwrap();
    w.append(&event::exec_attempt("dev", &parsed("ls"), "allow", None))
        .unwrap();

    let mut s = AuditScreen::load(&p).unwrap();
    s.begin_edit(EditField::Project);
    for c in "prod".chars() {
        s.push_edit_char(c);
    }
    s.finish_edit();
    assert_eq!(s.filtered_rows().len(), 1);

    // Re-open the project filter and finish empty → cleared.
    s.begin_edit(EditField::Project);
    s.finish_edit();
    assert_eq!(s.filtered_rows().len(), 2);
}
