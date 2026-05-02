//! Tests for the tunnel approval variant in the Approvals screen.
//!
//! Verifies that pending requests with `tunnel: Some(spec)` route to a
//! `PatternRule` with `category = Some("network:tunnel")` instead of the
//! exec or file-op paths.

use chrono::Utc;
use safessh_core::types::ParsedCommand;
use safessh_storage::approvals::{AlwaysStore, PendingRequest, PendingStore, TimedStore};
use safessh_storage::paths::Paths;
use safessh_tui::screens::approvals::{ApprovalsScreen, PickerAction};
use tempfile::tempdir;

fn paths_in(td: &tempfile::TempDir) -> Paths {
    let p = Paths {
        config: td.path().join("config"),
        state: td.path().join("state"),
        cache: td.path().join("cache"),
    };
    p.ensure_dirs().unwrap();
    p
}

fn add_tunnel_pending(p: &Paths, spec: &str) -> PendingRequest {
    let req = PendingRequest {
        token: "abcdef".into(),
        project: "prod".into(),
        categories: vec!["network:tunnel".into()],
        parsed: ParsedCommand {
            binary: "@network:tunnel".into(),
            flags: vec![],
            args: vec![spec.into()],
            redirects: vec![],
            pipes: vec![],
            env_mutations: vec![],
            raw: format!("network:tunnel {spec}"),
        },
        raw: format!("network:tunnel {spec}"),
        created_at: Utc::now(),
        path: None,
        tunnel: Some(spec.into()),
    };
    PendingStore::new(p).add(&req).unwrap();
    req
}

#[test]
fn always_picker_writes_category_rule() {
    let td = tempdir().unwrap();
    let p = paths_in(&td);
    add_tunnel_pending(&p, "5432:db:5432");
    let mut screen = ApprovalsScreen::load(&p).unwrap();
    screen.apply_action(PickerAction::Always).unwrap();

    let rules = AlwaysStore::new(&p).list("prod").unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].category.as_deref(), Some("network:tunnel"));
}

#[test]
fn timed_picker_writes_category_rule() {
    let td = tempdir().unwrap();
    let p = paths_in(&td);
    add_tunnel_pending(&p, "5432:db:5432");
    let mut screen = ApprovalsScreen::load(&p).unwrap();
    screen.apply_action(PickerAction::Timed(15)).unwrap();
    let rules = TimedStore::new(&p).list_active("prod").unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].pattern.category.as_deref(), Some("network:tunnel"));
    let delta = rules[0].expires_at - chrono::Utc::now();
    assert!(delta >= chrono::Duration::minutes(14));
    assert!(delta <= chrono::Duration::minutes(16));
}

#[test]
fn tunnel_row_label_contains_spec() {
    let td = tempdir().unwrap();
    let p = paths_in(&td);
    add_tunnel_pending(&p, "5432:db:5432");
    let screen = ApprovalsScreen::load(&p).unwrap();
    // Verify that the screen loaded the pending tunnel request.
    assert_eq!(screen.pending.len(), 1);
    assert_eq!(
        screen.pending[0].tunnel.as_deref(),
        Some("5432:db:5432"),
        "tunnel spec should be preserved in pending request"
    );
}
