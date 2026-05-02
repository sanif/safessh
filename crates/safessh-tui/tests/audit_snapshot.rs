//! Insta snapshots for AuditScreen — empty state and a hand-rolled
//! audit.log (so timestamps are deterministic).

use ratatui::{backend::TestBackend, Terminal};
use safessh_storage::paths::Paths;
use safessh_tui::screens::audit::AuditScreen;

fn paths() -> Paths {
    let tmp = tempfile::tempdir().unwrap();
    let p = Paths {
        config: tmp.path().join("config"),
        state: tmp.path().join("state"),
        cache: tmp.path().join("cache"),
    };
    p.ensure_dirs().unwrap();
    std::mem::forget(tmp);
    p
}

fn render(screen: &AuditScreen) -> String {
    let backend = TestBackend::new(100, 12);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| screen.render(f, f.area())).unwrap();
    let buf = term.backend().buffer();
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let pos = ratatui::layout::Position::new(x, y);
            out.push_str(buf.cell(pos).map(|c| c.symbol()).unwrap_or(" "));
        }
        out.push('\n');
    }
    out
}

#[test]
fn empty_state() {
    let p = paths();
    let s = AuditScreen::load(&p).unwrap();
    insta::assert_snapshot!(render(&s));
}

#[test]
fn renders_recent_events() {
    let p = paths();
    // Hand-roll the audit log so timestamps are pinned.
    let log = "\
{\"schema_version\":1,\"timestamp\":\"2026-04-30T10:15:00Z\",\"event_type\":\"exec_attempt\",\"project\":\"prod\",\"data\":{\"binary\":\"ls\",\"flags\":[],\"args\":[\"/var\"],\"decision\":\"Allow\"},\"error_class\":null,\"error_message\":null}
{\"schema_version\":1,\"timestamp\":\"2026-04-30T10:15:01Z\",\"event_type\":\"exec_complete\",\"project\":\"prod\",\"data\":{\"exit_code\":0,\"stdout_bytes\":100,\"stderr_bytes\":0,\"duration_ms\":30},\"error_class\":null,\"error_message\":null}
";
    std::fs::write(p.audit_log(), log).unwrap();
    let s = AuditScreen::load(&p).unwrap();
    insta::assert_snapshot!(render(&s));
}
