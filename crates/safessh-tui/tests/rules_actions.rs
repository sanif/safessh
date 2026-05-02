//! Action tests for the Rules screen — verify `apply_delete` removes
//! from the correct underlying store.

use chrono::{Duration, Utc};
use safessh_storage::approvals::{AlwaysStore, BlockedStore, PatternRule, TimedRule, TimedStore};
use safessh_storage::paths::Paths;
use safessh_storage::project::{Approvals, OutputCaps, Policy, Project, ProjectStore, Target};
use safessh_tui::screens::rules::{RuleTab, RulesScreen};

fn pattern(rule_id: &str) -> PatternRule {
    PatternRule {
        rule_id: rule_id.into(),
        binary: "rm".into(),
        flags: vec!["-rf".into()],
        args_pattern: None,
        categories: vec!["destructive:filesystem".into()],
        created_at: Utc::now(),
    }
}

fn setup() -> (tempfile::TempDir, Paths) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        config: tmp.path().join("config"),
        state: tmp.path().join("state"),
        cache: tmp.path().join("cache"),
    };
    paths.ensure_dirs().unwrap();
    // Need at least one project so RulesScreen has something to point at.
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
    (tmp, paths)
}

#[test]
fn delete_timed_rule_removes_from_store() {
    let (_tmp, p) = setup();
    TimedStore::new(&p)
        .add(
            "p",
            TimedRule {
                pattern: pattern("rule-t"),
                expires_at: Utc::now() + Duration::hours(1),
            },
        )
        .unwrap();
    let mut s = RulesScreen::load(&p).unwrap();
    s.switch_tab(RuleTab::Timed);
    assert_eq!(s.timed.len(), 1);
    s.apply_delete().unwrap();
    assert_eq!(TimedStore::new(&p).list_active("p").unwrap().len(), 0);
}

#[test]
fn delete_always_rule_removes_from_store() {
    let (_tmp, p) = setup();
    AlwaysStore::new(&p).add("p", pattern("rule-a")).unwrap();
    let mut s = RulesScreen::load(&p).unwrap();
    s.switch_tab(RuleTab::Always);
    s.apply_delete().unwrap();
    assert_eq!(AlwaysStore::new(&p).list("p").unwrap().len(), 0);
}

#[test]
fn delete_blocked_rule_removes_from_store() {
    let (_tmp, p) = setup();
    BlockedStore::new(&p).add("p", pattern("rule-b")).unwrap();
    let mut s = RulesScreen::load(&p).unwrap();
    s.switch_tab(RuleTab::Blocked);
    s.apply_delete().unwrap();
    assert_eq!(BlockedStore::new(&p).list("p").unwrap().len(), 0);
}

#[test]
fn cycle_tab_wraps() {
    let (_tmp, p) = setup();
    let mut s = RulesScreen::load(&p).unwrap();
    assert_eq!(s.tab, RuleTab::Timed);
    s.cycle_tab(1);
    assert_eq!(s.tab, RuleTab::Always);
    s.cycle_tab(1);
    assert_eq!(s.tab, RuleTab::Blocked);
    s.cycle_tab(1);
    assert_eq!(s.tab, RuleTab::Timed);
    s.cycle_tab(-1);
    assert_eq!(s.tab, RuleTab::Blocked);
}
