//! Routing tests for the App skeleton: q/Ctrl-C/Esc → Quit, header
//! renders. Subagent screens (Tasks 7-10) and the help overlay (Task 11)
//! add more cases here.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use safessh_storage::paths::Paths;
use safessh_tui::{App, AppAction};

fn paths() -> Paths {
    let tmp = tempfile::tempdir().unwrap();
    let p = Paths {
        config: tmp.path().join("config"),
        state: tmp.path().join("state"),
        cache: tmp.path().join("cache"),
    };
    std::mem::forget(tmp);
    p
}

#[test]
fn q_quits() {
    let mut app = App::new(paths());
    let action = app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(matches!(action, AppAction::Quit));
}

#[test]
fn ctrl_c_quits() {
    let mut app = App::new(paths());
    let action = app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(matches!(action, AppAction::Quit));
}

#[test]
fn esc_quits_at_root() {
    let mut app = App::new(paths());
    let action = app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(action, AppAction::Quit));
}

#[test]
fn renders_header_text() {
    let backend = TestBackend::new(60, 10);
    let mut term = Terminal::new(backend).unwrap();
    let app = App::new(paths());
    term.draw(|f| app.render(f)).unwrap();
    let buf = term.backend().buffer();
    let text: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(text.contains("safessh"), "expected header text: {text:?}");
}
