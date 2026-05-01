//! Insta snapshots for the Approvals screen.

use chrono::Utc;
use ratatui::{backend::TestBackend, Terminal};
use safessh_core::types::ParsedCommand;
use safessh_storage::approvals::{PendingRequest, PendingStore};
use safessh_storage::paths::Paths;
use safessh_tui::screens::approvals::ApprovalsScreen;

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

fn render(screen: &ApprovalsScreen) -> String {
    let backend = TestBackend::new(100, 16);
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
    let s = ApprovalsScreen::load(&p).unwrap();
    insta::assert_snapshot!(render(&s));
}

#[test]
fn one_pending_request() {
    let p = paths();
    // Pin created_at to "now" so the AGE column always reads "now",
    // making the snapshot stable across runs.
    PendingStore::new(&p)
        .add(&PendingRequest {
            token: "ABC123".into(),
            project: "prod".into(),
            categories: vec!["destructive:filesystem".into()],
            parsed: ParsedCommand {
                binary: "rm".into(),
                flags: vec!["-rf".into()],
                args: vec!["/tmp/x".into()],
                redirects: vec![],
                pipes: vec![],
                env_mutations: vec![],
                raw: "rm -rf /tmp/x".into(),
            },
            raw: "rm -rf /tmp/x".into(),
            created_at: Utc::now(),
        })
        .unwrap();
    let s = ApprovalsScreen::load(&p).unwrap();
    insta::assert_snapshot!(render(&s));
}
