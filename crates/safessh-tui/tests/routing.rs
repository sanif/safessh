//! Routing tests for the App skeleton: q/Ctrl-C/Esc → Quit, header
//! renders. Subagent screens (Tasks 7-10) and the help overlay (Task 11)
//! add more cases here.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use safessh_storage::paths::Paths;
use safessh_tui::screens::Screen;
use safessh_tui::{help_text, App, AppAction};

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

#[test]
fn tab_cycles_forward() {
    let mut app = App::new(paths());
    assert_eq!(app.current, Screen::Projects);
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.current, Screen::Approvals);
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.current, Screen::Rules);
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.current, Screen::Audit);
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.current, Screen::Projects);
}

#[test]
fn shift_tab_cycles_backward() {
    let mut app = App::new(paths());
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.current, Screen::Audit);
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.current, Screen::Rules);
}

#[test]
fn question_mark_toggles_help_overlay() {
    let mut app = App::new(paths());
    assert!(!app.help_open);
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert!(app.help_open);
    // Esc closes the overlay (does NOT quit the app).
    let action = app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!matches!(action, AppAction::Quit));
    assert!(!app.help_open);
}

#[test]
fn overlay_swallows_screen_navigation() {
    let mut app = App::new(paths());
    app.help_open = true;
    // Tab would normally cycle; with overlay open it should be ignored.
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.current, Screen::Projects);
    assert!(app.help_open);
}

#[test]
fn help_text_is_public_and_non_empty() {
    let t = help_text();
    assert!(t.contains("Tab"), "expected Tab in help: {t}");
    assert!(t.contains("Approvals"));
    assert!(t.contains("Audit"));
}
