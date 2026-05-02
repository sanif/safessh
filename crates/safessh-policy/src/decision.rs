//! Policy decision engine — pure function over a parsed command, the project
//! policy, and the three approval rule lists (allows / timed / blocks).
//! v0.3 adds path-glob `file_rules` for first-class read/write operations.
//!
//! # Safety invariants
//!
//! * **SAFETY-INVARIANT-1:** any parse failure produces
//!   [`PolicyDecision::RequireApproval`].
//! * **SAFETY-INVARIANT-2:** the block-list is consulted **before** any allow
//!   path.
//! * **SAFETY-INVARIANT-14:** preset `file_rules` are evaluated **before** any
//!   per-project `file_rules` so the shipped sensitive-path deny-list cannot
//!   be overridden by a permissive project rule.

use crate::categories::match_all;
use crate::parser::parse;
use globset::{GlobBuilder, GlobSetBuilder};
use safessh_core::types::{AllowSource, ApprovalToken, ParsedCommand, PolicyDecision};
use safessh_storage::approvals::{PatternRule, TimedRule};
use safessh_storage::project::{FileDecision, FileRule, Policy};

/// Discriminator for first-class file operations. `None` means the input is
/// an exec command (the v0.2 path).
#[derive(Debug, Clone, Copy)]
pub enum FileOp<'a> {
    None,
    Read(&'a str),
    Write(&'a str),
}

impl<'a> FileOp<'a> {
    fn category(&self) -> Option<&'static str> {
        match self {
            FileOp::None => None,
            FileOp::Read(_) => Some("file:read"),
            FileOp::Write(_) => Some("file:write"),
        }
    }
    fn path(&self) -> Option<&'a str> {
        match self {
            FileOp::None => None,
            FileOp::Read(p) | FileOp::Write(p) => Some(*p),
        }
    }
}

pub struct DecisionInput<'a> {
    pub raw: &'a str,
    pub policy: &'a Policy,
    pub allows: &'a [PatternRule],
    pub timed: &'a [TimedRule],
    pub blocks: &'a [PatternRule],
    pub file_op: FileOp<'a>,
    pub preset_file_rules: &'a [FileRule],
}

pub fn decide(input: DecisionInput<'_>) -> (PolicyDecision, Option<ParsedCommand>) {
    // File operations short-circuit the AST parser entirely.
    if let (Some(category), Some(path)) = (input.file_op.category(), input.file_op.path()) {
        return (decide_file(category, path, input.policy, input.preset_file_rules), None);
    }

    // Exec path — unchanged from v0.2.
    // SAFETY-INVARIANT-1: Parse failure produces RequireApproval, never Allow.
    let parsed = match parse(input.raw) {
        Ok(mut v) if !v.is_empty() => v.remove(0),
        _ => {
            return (
                PolicyDecision::RequireApproval {
                    token: ApprovalToken::generate(),
                    categories: vec!["unparseable".into()],
                    reason: "command could not be parsed safely".into(),
                },
                None,
            );
        }
    };
    let cats = match_all(&parsed);

    // SAFETY-INVARIANT-2: Block-list checked BEFORE allow-list.
    if let Some(rule) = matching_rule(&parsed, input.blocks) {
        return (
            PolicyDecision::Block {
                rule_id: rule.rule_id.clone(),
                pattern: format!("{} {}", rule.binary, rule.flags.join(" ")),
            },
            Some(parsed),
        );
    }

    if cats.iter().any(|c| input.policy.deny.contains(c)) {
        return (
            PolicyDecision::Deny {
                reason: format!("project denies categories: {cats:?}"),
            },
            Some(parsed),
        );
    }

    if let Some(rule) = matching_timed(&parsed, input.timed) {
        return (
            PolicyDecision::Allow {
                matched_rule: Some(rule.pattern.rule_id.clone()),
                source: AllowSource::TimedRule {
                    rule_id: rule.pattern.rule_id.clone(),
                    expires_at: rule.expires_at,
                },
            },
            Some(parsed),
        );
    }

    if let Some(rule) = matching_rule(&parsed, input.allows) {
        return (
            PolicyDecision::Allow {
                matched_rule: Some(rule.rule_id.clone()),
                source: AllowSource::AlwaysRule(rule.rule_id.clone()),
            },
            Some(parsed),
        );
    }

    let needs_approval = cats.iter().any(|c| input.policy.require_approval.contains(c))
        || cats.iter().any(|c| !input.policy.allow.contains(c));
    if !needs_approval {
        return (
            PolicyDecision::Allow {
                matched_rule: None,
                source: AllowSource::DefaultPolicy,
            },
            Some(parsed),
        );
    }

    (
        PolicyDecision::RequireApproval {
            token: ApprovalToken::generate(),
            categories: cats.clone(),
            reason: format!("requires approval for categories: {cats:?}"),
        },
        Some(parsed),
    )
}

fn decide_file(
    category: &str,
    path: &str,
    policy: &Policy,
    preset_rules: &[FileRule],
) -> PolicyDecision {
    // SAFETY-INVARIANT-14: preset file_rules evaluated before project file_rules.
    if let Some(d) = match_file_rules(category, path, preset_rules) {
        return file_decision_to_policy(d, "preset");
    }
    if let Some(d) = match_file_rules(category, path, &policy.file_rules) {
        return file_decision_to_policy(d, "project");
    }

    // No file_rule match — fall through to category-level treatment.
    if policy.deny.iter().any(|c| c == category) {
        return PolicyDecision::Deny {
            reason: format!("project denies category {category}"),
        };
    }
    if policy.allow.iter().any(|c| c == category)
        && !policy.require_approval.iter().any(|c| c == category)
    {
        return PolicyDecision::Allow {
            matched_rule: None,
            source: AllowSource::DefaultPolicy,
        };
    }
    PolicyDecision::RequireApproval {
        token: ApprovalToken::generate(),
        categories: vec![category.into()],
        reason: format!("requires approval for category: {category}"),
    }
}

/// Evaluate a list of FileRules and return the highest-precedence matching
/// decision: block > deny > approve > allow. Returns `None` if nothing matches.
fn match_file_rules(category: &str, path: &str, rules: &[FileRule]) -> Option<FileDecision> {
    let mut highest: Option<FileDecision> = None;
    for rule in rules.iter().filter(|r| r.category == category) {
        let mut builder = GlobSetBuilder::new();
        let mut had_glob = false;
        for p in &rule.paths {
            if let Ok(g) = GlobBuilder::new(p).literal_separator(true).build() {
                builder.add(g);
                had_glob = true;
            }
        }
        if !had_glob {
            continue;
        }
        let set = match builder.build() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if !set.is_match(path) {
            continue;
        }
        // Higher precedence overwrites lower.
        highest = Some(match (highest, rule.decision) {
            (Some(FileDecision::Block), _) => FileDecision::Block,
            (_, FileDecision::Block) => FileDecision::Block,
            (Some(FileDecision::Deny), _) => FileDecision::Deny,
            (_, FileDecision::Deny) => FileDecision::Deny,
            (Some(FileDecision::Approve), _) => FileDecision::Approve,
            (_, FileDecision::Approve) => FileDecision::Approve,
            (_, d) => d,
        });
    }
    highest
}

fn file_decision_to_policy(d: FileDecision, source: &str) -> PolicyDecision {
    match d {
        FileDecision::Allow => PolicyDecision::Allow {
            matched_rule: Some(format!("{source}.file_rules")),
            source: AllowSource::DefaultPolicy,
        },
        FileDecision::Approve => PolicyDecision::RequireApproval {
            token: ApprovalToken::generate(),
            categories: vec!["file_rule".into()],
            reason: format!("{source} file_rule requires approval"),
        },
        FileDecision::Deny => PolicyDecision::Deny {
            reason: format!("{source} file_rule denies path"),
        },
        FileDecision::Block => PolicyDecision::Block {
            rule_id: format!("{source}.file_rules"),
            pattern: "file_rule block".into(),
        },
    }
}

fn matching_rule<'a>(parsed: &ParsedCommand, rules: &'a [PatternRule]) -> Option<&'a PatternRule> {
    rules
        .iter()
        .find(|r| r.binary == parsed.binary && flags_match(&r.flags, &parsed.flags))
}

fn matching_timed<'a>(parsed: &ParsedCommand, rules: &'a [TimedRule]) -> Option<&'a TimedRule> {
    rules
        .iter()
        .find(|r| r.pattern.binary == parsed.binary && flags_match(&r.pattern.flags, &parsed.flags))
}

fn flags_match(rule_flags: &[String], parsed_flags: &[String]) -> bool {
    rule_flags.iter().all(|f| parsed_flags.contains(f))
}
