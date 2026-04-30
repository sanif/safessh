//! Integration tests for the policy decision engine.
//!
//! These tests exercise the safety invariants directly:
//!
//! * Parse failure must produce `RequireApproval` (SAFETY-INVARIANT-1).
//! * Block-list takes priority over the allow-list (SAFETY-INVARIANT-2).
//! * Default policy with a category covered by `policy.allow` resolves to
//!   `Allow { DefaultPolicy }` only when none of the matched categories also
//!   appear in `require_approval`.

use chrono::{Duration, Utc};
use safessh_core::types::{AllowSource, PolicyDecision};
use safessh_policy::{decide, DecisionInput};
use safessh_storage::approvals::{PatternRule, TimedRule};
use safessh_storage::project::Policy;

fn pattern(rule_id: &str, binary: &str, flags: &[&str]) -> PatternRule {
    PatternRule {
        rule_id: rule_id.into(),
        binary: binary.into(),
        flags: flags.iter().map(|s| (*s).to_string()).collect(),
        args_pattern: None,
        categories: vec![],
        created_at: Utc::now(),
    }
}

#[test]
fn unparseable_requires_approval() {
    // `rm -rf '/var` has an unterminated single quote so tokenisation fails;
    // the engine must fall back to RequireApproval rather than Allow.
    let policy = Policy::default();
    let (d, parsed) = decide(DecisionInput {
        raw: "rm -rf '/var",
        policy: &policy,
        allows: &[],
        timed: &[],
        blocks: &[],
    });
    assert!(
        matches!(d, PolicyDecision::RequireApproval { .. }),
        "expected RequireApproval, got {d:?}"
    );
    assert!(parsed.is_none(), "parsed should be None on parse failure");
    if let PolicyDecision::RequireApproval { categories, .. } = d {
        assert_eq!(categories, vec!["unparseable".to_string()]);
    }
}

#[test]
fn block_takes_priority_over_allow() {
    // Same rule shape in both allows and blocks → Block wins.
    let allows = vec![pattern("a1", "rm", &["-r", "-f"])];
    let blocks = vec![pattern("b1", "rm", &["-r", "-f"])];
    let policy = Policy::default();
    let (d, parsed) = decide(DecisionInput {
        raw: "rm -rf /var/log",
        policy: &policy,
        allows: &allows,
        timed: &[],
        blocks: &blocks,
    });
    assert!(
        matches!(d, PolicyDecision::Block { .. }),
        "expected Block, got {d:?}"
    );
    assert!(parsed.is_some());
    if let PolicyDecision::Block { rule_id, .. } = d {
        assert_eq!(rule_id, "b1");
    }
}

#[test]
fn read_safe_with_default_policy_allows() {
    // `ls /etc` is read:safe; with policy.allow containing read:safe (and no
    // require_approval entry), the engine resolves to Allow { DefaultPolicy }.
    let policy = Policy {
        allow: vec!["read:safe".into()],
        ..Policy::default()
    };
    let (d, parsed) = decide(DecisionInput {
        raw: "ls /etc",
        policy: &policy,
        allows: &[],
        timed: &[],
        blocks: &[],
    });
    assert!(
        matches!(
            d,
            PolicyDecision::Allow {
                source: AllowSource::DefaultPolicy,
                ..
            }
        ),
        "expected Allow {{ DefaultPolicy }}, got {d:?}"
    );
    assert!(parsed.is_some());
}

#[test]
fn deny_overrides_default_allow() {
    // A category in policy.deny short-circuits the allow path even if the
    // category is also in policy.allow.
    let policy = Policy {
        allow: vec!["read:safe".into()],
        deny: vec!["read:safe".into()],
        ..Policy::default()
    };
    let (d, _) = decide(DecisionInput {
        raw: "ls /etc",
        policy: &policy,
        allows: &[],
        timed: &[],
        blocks: &[],
    });
    assert!(matches!(d, PolicyDecision::Deny { .. }), "got {d:?}");
}

#[test]
fn timed_rule_wins_over_default_require_approval() {
    // Empty policy means matched categories aren't in policy.allow, so the
    // default branch would require approval. An unexpired timed rule must
    // override that and produce Allow { TimedRule }.
    let policy = Policy::default();
    let timed = vec![TimedRule {
        pattern: pattern("t1", "ls", &[]),
        expires_at: Utc::now() + Duration::hours(1),
    }];
    let (d, _) = decide(DecisionInput {
        raw: "ls /etc",
        policy: &policy,
        allows: &[],
        timed: &timed,
        blocks: &[],
    });
    assert!(
        matches!(
            d,
            PolicyDecision::Allow {
                source: AllowSource::TimedRule { .. },
                ..
            }
        ),
        "got {d:?}"
    );
}

#[test]
fn always_rule_allows_when_default_would_require_approval() {
    let policy = Policy::default();
    let allows = vec![pattern("a1", "ls", &[])];
    let (d, _) = decide(DecisionInput {
        raw: "ls /etc",
        policy: &policy,
        allows: &allows,
        timed: &[],
        blocks: &[],
    });
    assert!(
        matches!(
            d,
            PolicyDecision::Allow {
                source: AllowSource::AlwaysRule(_),
                ..
            }
        ),
        "got {d:?}"
    );
}

#[test]
fn require_approval_overrides_default_allow() {
    // Even though read:safe is in policy.allow, listing it in
    // require_approval forces the RequireApproval branch.
    let policy = Policy {
        allow: vec!["read:safe".into()],
        require_approval: vec!["read:safe".into()],
        ..Policy::default()
    };
    let (d, _) = decide(DecisionInput {
        raw: "ls /etc",
        policy: &policy,
        allows: &[],
        timed: &[],
        blocks: &[],
    });
    assert!(
        matches!(d, PolicyDecision::RequireApproval { .. }),
        "got {d:?}"
    );
}

#[test]
fn flags_match_is_subset() {
    // Rule wants -r and -f; parsed has -r, -f, -v. Subset match → Block.
    let blocks = vec![pattern("b1", "rm", &["-r", "-f"])];
    let policy = Policy::default();
    let (d, _) = decide(DecisionInput {
        raw: "rm -rfv /tmp/x",
        policy: &policy,
        allows: &[],
        timed: &[],
        blocks: &blocks,
    });
    assert!(matches!(d, PolicyDecision::Block { .. }), "got {d:?}");
}
