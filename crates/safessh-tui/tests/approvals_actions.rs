//! Action tests for the Approvals screen — verifies that each PickerAction
//! routes to the correct store and that the pending file is consumed.

use chrono::Utc;
use safessh_core::types::{ApprovalToken, ParsedCommand};
use safessh_storage::approvals::{
    AlwaysStore, BlockedStore, PendingRequest, PendingStore, TimedStore,
};
use safessh_storage::paths::Paths;
use safessh_storage::project::{Approvals, FileDecision, OutputCaps, Policy, Project, ProjectStore, Target};
use safessh_tui::screens::approvals::{ApprovalsScreen, PickerAction};

fn setup_with_one() -> (tempfile::TempDir, Paths, String) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        config: tmp.path().join("config"),
        state: tmp.path().join("state"),
        cache: tmp.path().join("cache"),
    };
    paths.ensure_dirs().unwrap();
    let token = "TOK001";
    PendingStore::new(&paths)
        .add(&PendingRequest {
            token: token.into(),
            project: "p".into(),
            categories: vec!["destructive:filesystem".into()],
            parsed: ParsedCommand {
                binary: "rm".into(),
                flags: vec!["-rf".into()],
                args: vec!["/x".into()],
                redirects: vec![],
                pipes: vec![],
                env_mutations: vec![],
                raw: "rm -rf /x".into(),
            },
            raw: "rm -rf /x".into(),
            created_at: Utc::now(),
            path: None,
        })
        .unwrap();
    (tmp, paths, token.into())
}

/// Create a minimal project in the temp dir and a file-read pending approval.
fn setup_file_read() -> (tempfile::TempDir, Paths, String) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        config: tmp.path().join("config"),
        state: tmp.path().join("state"),
        cache: tmp.path().join("cache"),
    };
    paths.ensure_dirs().unwrap();

    // Persist a project so ProjectStore::load succeeds.
    ProjectStore::new(paths.clone())
        .save(&Project {
            name: "p".into(),
            default_target: "default".into(),
            targets: vec![Target::SshConfigAlias {
                name: "default".into(),
                ssh_config_alias: "x".into(),
            }],
            policy: Policy::default(),
            approvals: Approvals::default(),
            output: OutputCaps::default(),
        })
        .unwrap();

    let token = "FIL001";
    PendingStore::new(&paths)
        .add(&PendingRequest {
            token: token.into(),
            project: "p".into(),
            categories: vec!["file:read".into()],
            parsed: ParsedCommand {
                binary: "file:read".into(),
                flags: vec![],
                args: vec!["/etc/hosts".into()],
                redirects: vec![],
                pipes: vec![],
                env_mutations: vec![],
                raw: "file:read /etc/hosts".into(),
            },
            raw: "file:read /etc/hosts".into(),
            created_at: Utc::now(),
            path: Some("/etc/hosts".into()),
        })
        .unwrap();
    (tmp, paths, token.into())
}

#[test]
fn once_removes_pending() {
    let (_tmp, p, tok) = setup_with_one();
    let mut s = ApprovalsScreen::load(&p).unwrap();
    s.apply_to_selected(PickerAction::Once).unwrap();
    assert!(
        PendingStore::new(&p)
            .take(&ApprovalToken::from_str(&tok))
            .is_err(),
        "expected pending file removed"
    );
}

#[test]
fn always_adds_to_always_store_and_removes_pending() {
    let (_tmp, p, _tok) = setup_with_one();
    let mut s = ApprovalsScreen::load(&p).unwrap();
    s.apply_to_selected(PickerAction::Always).unwrap();
    assert_eq!(
        AlwaysStore::new(&p).list("p").unwrap().len(),
        1,
        "expected one always-rule for project p"
    );
}

#[test]
fn timed_adds_to_timed_store() {
    let (_tmp, p, _tok) = setup_with_one();
    let mut s = ApprovalsScreen::load(&p).unwrap();
    s.apply_to_selected(PickerAction::Timed(15)).unwrap();
    assert_eq!(
        TimedStore::new(&p).list_active("p").unwrap().len(),
        1,
        "expected one timed-rule for project p"
    );
}

#[test]
fn block_adds_to_blocked_store() {
    let (_tmp, p, _tok) = setup_with_one();
    let mut s = ApprovalsScreen::load(&p).unwrap();
    s.apply_to_selected(PickerAction::Block).unwrap();
    assert_eq!(
        BlockedStore::new(&p).list("p").unwrap().len(),
        1,
        "expected one blocked-rule for project p"
    );
}

#[test]
fn deny_removes_pending_without_creating_rules() {
    let (_tmp, p, _tok) = setup_with_one();
    let mut s = ApprovalsScreen::load(&p).unwrap();
    s.apply_to_selected(PickerAction::Deny).unwrap();
    assert_eq!(s.pending.len(), 0);
    assert_eq!(AlwaysStore::new(&p).list("p").unwrap().len(), 0);
    assert_eq!(BlockedStore::new(&p).list("p").unwrap().len(), 0);
    assert_eq!(TimedStore::new(&p).list_active("p").unwrap().len(), 0);
}

/// "Always allow" on a file-read pending approval must write a `FileRule`
/// to `project.policy.file_rules` and remove the pending file.
#[test]
fn file_read_always_writes_file_rule_and_removes_pending() {
    let (_tmp, p, tok) = setup_file_read();
    let mut s = ApprovalsScreen::load(&p).unwrap();
    s.apply_to_selected(PickerAction::Always).unwrap();

    // Pending file consumed.
    assert!(
        PendingStore::new(&p)
            .take(&ApprovalToken::from_str(&tok))
            .is_err(),
        "expected pending file removed"
    );

    // A FileRule was added to the project.
    let project = ProjectStore::new(p.clone()).load("p").unwrap();
    assert_eq!(project.policy.file_rules.len(), 1);
    let rule = &project.policy.file_rules[0];
    assert_eq!(rule.category, "file:read");
    assert_eq!(rule.paths, vec!["/etc/hosts"]);
    assert_eq!(rule.decision, FileDecision::Allow);

    // No pattern rules in the exec stores.
    assert_eq!(AlwaysStore::new(&p).list("p").unwrap().len(), 0);
}

/// "Once" on a file-read pending approval consumes the pending file and
/// writes no rules (exec path regression: PatternRule must NOT be written).
#[test]
fn file_read_once_no_rule_written() {
    let (_tmp, p, tok) = setup_file_read();
    let mut s = ApprovalsScreen::load(&p).unwrap();
    s.apply_to_selected(PickerAction::Once).unwrap();

    assert!(
        PendingStore::new(&p)
            .take(&ApprovalToken::from_str(&tok))
            .is_err(),
        "expected pending file removed"
    );
    let project = ProjectStore::new(p.clone()).load("p").unwrap();
    assert_eq!(project.policy.file_rules.len(), 0);
    assert_eq!(AlwaysStore::new(&p).list("p").unwrap().len(), 0);
}
