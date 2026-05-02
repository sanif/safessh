//! Insta snapshots for the Rules screen — empty state and one populated
//! tab per kind.

use chrono::{Duration, Utc};
use ratatui::{backend::TestBackend, Terminal};
use safessh_storage::approvals::{AlwaysStore, BlockedStore, PatternRule, TimedRule, TimedStore};
use safessh_storage::paths::Paths;
use safessh_storage::project::{
    Approvals, FileDecision, FileRule, OutputCaps, Policy, Project, ProjectStore, Target,
};
use safessh_tui::screens::rules::{RuleTab, RulesScreen};

fn paths_with_project() -> Paths {
    let tmp = tempfile::tempdir().unwrap();
    let p = Paths {
        config: tmp.path().join("config"),
        state: tmp.path().join("state"),
        cache: tmp.path().join("cache"),
    };
    p.ensure_dirs().unwrap();
    ProjectStore::new(p.clone())
        .save(&Project {
            name: "prod".into(),
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
    std::mem::forget(tmp);
    p
}

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

fn render(screen: &RulesScreen) -> String {
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
fn empty_timed_tab() {
    let p = paths_with_project();
    let s = RulesScreen::load(&p).unwrap();
    insta::assert_snapshot!(render(&s));
}

#[test]
fn populated_always_tab() {
    let p = paths_with_project();
    AlwaysStore::new(&p)
        .add("prod", pattern("rule-allow-1"))
        .unwrap();
    let mut s = RulesScreen::load(&p).unwrap();
    s.switch_tab(RuleTab::Always);
    insta::assert_snapshot!(render(&s));
}

#[test]
fn populated_blocked_tab() {
    let p = paths_with_project();
    BlockedStore::new(&p)
        .add("prod", pattern("rule-block-1"))
        .unwrap();
    // Also stash a timed rule that won't appear since we're on Blocked.
    TimedStore::new(&p)
        .add(
            "prod",
            TimedRule {
                pattern: pattern("rule-timed-X"),
                expires_at: Utc::now() + Duration::hours(1),
            },
        )
        .unwrap();
    let mut s = RulesScreen::load(&p).unwrap();
    s.switch_tab(RuleTab::Blocked);
    insta::assert_snapshot!(render(&s));
}

#[test]
fn rules_file_tab_empty() {
    let p = paths_with_project();
    let mut s = RulesScreen::load(&p).unwrap();
    s.switch_tab(RuleTab::File);
    insta::assert_snapshot!(render(&s));
}

#[test]
fn rules_file_tab_populated() {
    let p = paths_with_project();
    let store = ProjectStore::new(p.clone());
    let mut proj = store.load("prod").unwrap();
    proj.policy.file_rules = vec![
        FileRule {
            category: "read".into(),
            paths: vec!["/etc/passwd".into(), "/etc/shadow".into()],
            decision: FileDecision::Allow,
        },
        FileRule {
            category: "write".into(),
            paths: vec!["/var/log/app.log".into()],
            decision: FileDecision::Deny,
        },
    ];
    store.save(&proj).unwrap();
    let mut s = RulesScreen::load(&p).unwrap();
    s.switch_tab(RuleTab::File);
    insta::assert_snapshot!(render(&s));
}
