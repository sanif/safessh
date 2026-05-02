use safessh_core::types::PolicyDecision;
use safessh_policy::decision::{decide, DecisionInput, FileOp, TunnelOp};
use safessh_storage::policies::preset_file_rules;
use safessh_storage::project::{FileDecision, FileRule, Policy};

fn empty_policy() -> Policy {
    Policy {
        allow: vec!["read:safe".into(), "file:read".into()],
        require_approval: vec!["file:write".into()],
        deny: vec![],
        file_rules: vec![],
    }
}

fn input<'a>(policy: &'a Policy, op: FileOp<'a>) -> DecisionInput<'a> {
    DecisionInput {
        raw: "",
        policy,
        allows: &[],
        timed: &[],
        blocks: &[],
        file_op: op,
        preset_file_rules: preset_file_rules(),
        tunnel_op: TunnelOp::None,
    }
}

#[test]
fn preset_blocks_read_of_sensitive_path() {
    let mut policy = empty_policy();
    // even with a project-level allow, preset deny wins
    policy.file_rules.push(FileRule {
        category: "file:read".into(),
        paths: vec!["/etc/shadow".into()],
        decision: FileDecision::Allow,
    });
    let (decision, _) = decide(input(&policy, FileOp::Read("/etc/shadow")));
    assert!(
        matches!(decision, PolicyDecision::Deny { .. }),
        "got {decision:?}"
    );
}

#[test]
fn project_file_rule_allow_overrides_category_require_approval() {
    let mut policy = empty_policy();
    policy.allow.retain(|c| c != "file:read");
    policy.require_approval.push("file:read".into());
    policy.file_rules.push(FileRule {
        category: "file:read".into(),
        paths: vec!["/etc/nginx/*".into()],
        decision: FileDecision::Allow,
    });
    let (d, _) = decide(input(&policy, FileOp::Read("/etc/nginx/nginx.conf")));
    assert!(matches!(d, PolicyDecision::Allow { .. }));
}

#[test]
fn project_file_rule_deny_blocks_writes() {
    let mut policy = empty_policy();
    policy.file_rules.push(FileRule {
        category: "file:write".into(),
        paths: vec!["/var/lib/db/**".into()],
        decision: FileDecision::Deny,
    });
    let (d, _) = decide(input(&policy, FileOp::Write("/var/lib/db/main.sqlite")));
    assert!(matches!(d, PolicyDecision::Deny { .. }));
}

#[test]
fn no_file_rule_falls_through_to_category_allow() {
    let policy = empty_policy(); // file:read in `allow`
    let (d, _) = decide(input(&policy, FileOp::Read("/var/log/anything.log")));
    assert!(matches!(d, PolicyDecision::Allow { .. }));
}

#[test]
fn no_file_rule_falls_through_to_require_approval_for_write() {
    let policy = empty_policy(); // file:write in `require_approval`
    let (d, _) = decide(input(&policy, FileOp::Write("/tmp/anywhere")));
    assert!(matches!(d, PolicyDecision::RequireApproval { .. }));
}

#[test]
fn glob_double_star_matches_recursive() {
    let mut policy = empty_policy();
    policy.file_rules.push(FileRule {
        category: "file:read".into(),
        paths: vec!["/srv/data/**".into()],
        decision: FileDecision::Deny,
    });
    let (d, _) = decide(input(&policy, FileOp::Read("/srv/data/sub/dir/x.csv")));
    assert!(matches!(d, PolicyDecision::Deny { .. }));
}

#[test]
fn glob_single_star_does_not_cross_segment() {
    let mut policy = empty_policy();
    policy.file_rules.push(FileRule {
        category: "file:read".into(),
        paths: vec!["/etc/nginx/*".into()],
        decision: FileDecision::Deny,
    });
    // single-segment glob should NOT match the deeper path
    let (d, _) = decide(input(
        &policy,
        FileOp::Read("/etc/nginx/sites-available/foo"),
    ));
    assert!(matches!(d, PolicyDecision::Allow { .. }), "got {d:?}");
}
