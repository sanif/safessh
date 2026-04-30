//! Approval store tests.

use chrono::{Duration, Utc};
use safessh_core::types::{ApprovalToken, ParsedCommand};
use safessh_storage::approvals::*;
use safessh_storage::paths::Paths;

fn temp_paths() -> (tempfile::TempDir, Paths) {
    // Construct `Paths` directly from a fresh tempdir. Avoid `SAFESSH_HOME`
    // here — `std::env::set_var` is process-global, so when cargo runs tests
    // in parallel within the same binary, two tests racing on `SAFESSH_HOME`
    // would see each other's directories.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let paths = Paths {
        config: root.join("config"),
        state: root.join("state"),
        cache: root.join("cache"),
    };
    paths.ensure_dirs().unwrap();
    (dir, paths)
}

fn sample_pattern(rule_id: &str) -> PatternRule {
    PatternRule {
        rule_id: rule_id.into(),
        binary: "rm".into(),
        flags: vec!["-rf".into()],
        args_pattern: None,
        categories: vec!["destructive:filesystem".into()],
        created_at: Utc::now(),
    }
}

#[test]
fn timed_rule_expires() {
    let (_d, paths) = temp_paths();
    let store = TimedStore::new(&paths);
    let rule = TimedRule {
        pattern: sample_pattern("r1"),
        expires_at: Utc::now() - Duration::hours(1),
    };
    store.add("prod", rule).unwrap();
    let active = store.list_active("prod").unwrap();
    assert!(
        active.is_empty(),
        "expired rules must not appear in list_active"
    );
}

#[test]
fn timed_rule_active_within_window() {
    let (_d, paths) = temp_paths();
    let store = TimedStore::new(&paths);
    let rule = TimedRule {
        pattern: sample_pattern("r-active"),
        expires_at: Utc::now() + Duration::hours(1),
    };
    store.add("prod", rule).unwrap();
    let active = store.list_active("prod").unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].pattern.rule_id, "r-active");
}

#[test]
fn timed_purge_expired_removes_only_old() {
    let (_d, paths) = temp_paths();
    let store = TimedStore::new(&paths);
    store
        .add(
            "prod",
            TimedRule {
                pattern: sample_pattern("old"),
                expires_at: Utc::now() - Duration::minutes(5),
            },
        )
        .unwrap();
    store
        .add(
            "prod",
            TimedRule {
                pattern: sample_pattern("fresh"),
                expires_at: Utc::now() + Duration::hours(1),
            },
        )
        .unwrap();

    let removed = store.purge_expired("prod").unwrap();
    assert_eq!(removed, 1);
    let active = store.list_active("prod").unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].pattern.rule_id, "fresh");
}

#[test]
fn pending_add_take_roundtrip() {
    let (_d, paths) = temp_paths();
    let store = PendingStore::new(&paths);
    let token = ApprovalToken::generate();
    let request = PendingRequest {
        token: token.as_str().to_string(),
        project: "prod".into(),
        categories: vec!["destructive:filesystem".into()],
        parsed: ParsedCommand {
            binary: "rm".into(),
            flags: vec!["-rf".into()],
            args: vec!["/tmp/foo".into()],
            redirects: vec![],
            pipes: vec![],
            env_mutations: vec![],
            raw: "rm -rf /tmp/foo".into(),
        },
        raw: "rm -rf /tmp/foo".into(),
        created_at: Utc::now(),
    };

    store.add(&request).unwrap();
    let taken = store.take(&token).unwrap();
    assert_eq!(taken.token, request.token);
    assert_eq!(taken.raw, "rm -rf /tmp/foo");

    // Second take must fail (file removed).
    let err = store.take(&token).unwrap_err();
    assert!(matches!(err, safessh_core::error::Error::Usage(_)));
}

#[test]
fn pending_cleanup_expired_removes_old() {
    let (_d, paths) = temp_paths();
    let store = PendingStore::new(&paths);

    let old_token = ApprovalToken::generate();
    let fresh_token = ApprovalToken::generate();

    let parsed = ParsedCommand {
        binary: "ls".into(),
        flags: vec![],
        args: vec![],
        redirects: vec![],
        pipes: vec![],
        env_mutations: vec![],
        raw: "ls".into(),
    };

    store
        .add(&PendingRequest {
            token: old_token.as_str().to_string(),
            project: "p".into(),
            categories: vec![],
            parsed: parsed.clone(),
            raw: "ls".into(),
            created_at: Utc::now() - Duration::hours(48),
        })
        .unwrap();
    store
        .add(&PendingRequest {
            token: fresh_token.as_str().to_string(),
            project: "p".into(),
            categories: vec![],
            parsed,
            raw: "ls".into(),
            created_at: Utc::now(),
        })
        .unwrap();

    let removed = store.cleanup_expired(24).unwrap();
    assert_eq!(removed, 1);

    // Fresh one is still takeable; old one isn't.
    assert!(store.take(&fresh_token).is_ok());
    assert!(store.take(&old_token).is_err());
}

#[test]
fn always_add_list_remove() {
    let (_d, paths) = temp_paths();
    let store = AlwaysStore::new(&paths);
    store.add("prod", sample_pattern("a1")).unwrap();
    store.add("prod", sample_pattern("a2")).unwrap();
    let listed = store.list("prod").unwrap();
    assert_eq!(listed.len(), 2);

    store.remove("prod", "a1").unwrap();
    let listed = store.list("prod").unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].rule_id, "a2");
}

#[test]
fn blocked_add_list_remove() {
    let (_d, paths) = temp_paths();
    let store = BlockedStore::new(&paths);
    store.add("prod", sample_pattern("b1")).unwrap();
    let listed = store.list("prod").unwrap();
    assert_eq!(listed.len(), 1);

    store.remove("prod", "b1").unwrap();
    assert!(store.list("prod").unwrap().is_empty());
}
