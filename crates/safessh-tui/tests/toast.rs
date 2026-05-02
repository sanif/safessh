//! Toast lifecycle tests — show/clear/replace semantics for the
//! external-edit banner.

use safessh_storage::paths::Paths;
use safessh_tui::App;

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

#[test]
fn show_toast_sets_state() {
    let mut app = App::new(paths());
    assert!(app.toast.is_none());
    app.show_toast("hello");
    assert!(app.toast.is_some());
    assert_eq!(app.toast.as_ref().unwrap().text, "hello");
}

#[test]
fn tick_clears_expired_toast() {
    let mut app = App::new(paths());
    app.show_toast("transient");
    // Force the toast to be already expired.
    if let Some(t) = app.toast.as_mut() {
        t.expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    }
    app.tick_toast();
    assert!(app.toast.is_none(), "expected expired toast cleared");
}

#[test]
fn second_toast_replaces_first() {
    let mut app = App::new(paths());
    app.show_toast("first");
    app.show_toast("second");
    assert_eq!(app.toast.as_ref().unwrap().text, "second");
    // Only one toast should exist (no stacking).
    let count = if app.toast.is_some() { 1 } else { 0 };
    assert_eq!(count, 1);
}
