use chrono::{Duration, Utc};
use safessh_core::types::{AllowSource, PolicyDecision};
use safessh_policy::decision::{decide, DecisionInput, FileOp, TunnelOp};
use safessh_storage::approvals::{PatternRule, TimedRule};
use safessh_storage::project::Policy;

fn empty_policy() -> Policy {
    Policy::default()
}

fn run(
    op: TunnelOp,
    policy: &Policy,
    allows: &[PatternRule],
    timed: &[TimedRule],
    blocks: &[PatternRule],
) -> PolicyDecision {
    decide(DecisionInput {
        raw: "",
        policy,
        allows,
        timed,
        blocks,
        file_op: FileOp::None,
        preset_file_rules: &[],
        tunnel_op: op,
    })
    .0
}

#[test]
fn default_policy_requires_approval() {
    let d = run(
        TunnelOp::Forward("5432:db:5432"),
        &empty_policy(),
        &[],
        &[],
        &[],
    );
    assert!(matches!(d, PolicyDecision::RequireApproval { .. }));
}

#[test]
fn project_deny_returns_deny() {
    let mut p = empty_policy();
    p.deny.push("network:tunnel".into());
    let d = run(TunnelOp::Forward("5432:db:5432"), &p, &[], &[], &[]);
    assert!(matches!(d, PolicyDecision::Deny { .. }));
}

#[test]
fn project_allow_returns_allow() {
    let mut p = empty_policy();
    p.allow.push("network:tunnel".into());
    let d = run(TunnelOp::Forward("5432:db:5432"), &p, &[], &[], &[]);
    assert!(matches!(d, PolicyDecision::Allow { .. }));
}

#[test]
fn always_rule_with_category_returns_allow() {
    let rule = PatternRule {
        rule_id: "r1".into(),
        binary: "@network:tunnel".into(),
        flags: vec![],
        args_pattern: None,
        categories: vec!["network:tunnel".into()],
        category: Some("network:tunnel".into()),
        created_at: Utc::now(),
    };
    let d = run(
        TunnelOp::Forward("5432:db:5432"),
        &empty_policy(),
        &[rule],
        &[],
        &[],
    );
    match d {
        PolicyDecision::Allow {
            source: AllowSource::AlwaysRule(_),
            ..
        } => {}
        other => panic!("expected AlwaysRule allow, got {other:?}"),
    }
}

#[test]
fn block_rule_with_category_returns_block() {
    let rule = PatternRule {
        rule_id: "b1".into(),
        binary: "@network:tunnel".into(),
        flags: vec![],
        args_pattern: None,
        categories: vec!["network:tunnel".into()],
        category: Some("network:tunnel".into()),
        created_at: Utc::now(),
    };
    let d = run(
        TunnelOp::Forward("5432:db:5432"),
        &empty_policy(),
        &[],
        &[],
        &[rule],
    );
    assert!(matches!(d, PolicyDecision::Block { .. }));
}

#[test]
fn timed_rule_with_category_returns_allow() {
    let rule = TimedRule {
        pattern: PatternRule {
            rule_id: "t1".into(),
            binary: "@network:tunnel".into(),
            flags: vec![],
            args_pattern: None,
            categories: vec!["network:tunnel".into()],
            category: Some("network:tunnel".into()),
            created_at: Utc::now(),
        },
        expires_at: Utc::now() + Duration::minutes(15),
    };
    let d = run(
        TunnelOp::Forward("5432:db:5432"),
        &empty_policy(),
        &[],
        &[rule],
        &[],
    );
    match d {
        PolicyDecision::Allow {
            source: AllowSource::TimedRule { .. },
            ..
        } => {}
        other => panic!("expected TimedRule allow, got {other:?}"),
    }
}
