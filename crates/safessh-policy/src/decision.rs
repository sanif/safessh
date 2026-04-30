//! Policy decision engine — pure function over a parsed command, the project
//! policy, and the three approval rule lists (allows / timed / blocks).
//!
//! # Safety invariants
//!
//! * **SAFETY-INVARIANT-1:** any parse failure produces
//!   [`PolicyDecision::RequireApproval`]. The engine NEVER returns
//!   [`PolicyDecision::Allow`] for input it could not parse.
//! * **SAFETY-INVARIANT-2:** the block-list is consulted **before** any allow
//!   path. A rule appearing in both the allow-list and the block-list resolves
//!   to [`PolicyDecision::Block`].
//!
//! Branch order in [`decide`] (top to bottom; first match wins):
//!
//! 1. Parse failure → `RequireApproval`.
//! 2. Block-list pattern match → `Block`.
//! 3. Project policy `deny` covers any matched category → `Deny`.
//! 4. Unexpired timed-rule pattern match → `Allow { TimedRule }`.
//! 5. Always-rule pattern match → `Allow { AlwaysRule }`.
//! 6. All matched categories are in `policy.allow` and none are in
//!    `policy.require_approval` → `Allow { DefaultPolicy }`.
//! 7. Otherwise → `RequireApproval`.

use crate::categories::match_all;
use crate::parser::parse;
use safessh_core::types::{AllowSource, ApprovalToken, ParsedCommand, PolicyDecision};
use safessh_storage::approvals::{PatternRule, TimedRule};
use safessh_storage::project::Policy;

/// Inputs to [`decide`]: borrowed slices so the caller keeps ownership.
pub struct DecisionInput<'a> {
    pub raw: &'a str,
    pub policy: &'a Policy,
    pub allows: &'a [PatternRule],
    pub timed: &'a [TimedRule],
    pub blocks: &'a [PatternRule],
}

/// Evaluate `input` and return the resulting [`PolicyDecision`] together with
/// the [`ParsedCommand`] (when parsing succeeded). The parsed command is
/// returned so callers can record it in audit without re-parsing.
pub fn decide(input: DecisionInput<'_>) -> (PolicyDecision, Option<ParsedCommand>) {
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

    let needs_approval = cats
        .iter()
        .any(|c| input.policy.require_approval.contains(c))
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

/// Find the first rule whose `binary` matches and whose `flags` are a subset
/// of the parsed command's flags.
fn matching_rule<'a>(parsed: &ParsedCommand, rules: &'a [PatternRule]) -> Option<&'a PatternRule> {
    rules
        .iter()
        .find(|r| r.binary == parsed.binary && flags_match(&r.flags, &parsed.flags))
}

/// Same as [`matching_rule`] but for [`TimedRule`]s, which wrap a
/// [`PatternRule`].
fn matching_timed<'a>(parsed: &ParsedCommand, rules: &'a [TimedRule]) -> Option<&'a TimedRule> {
    rules
        .iter()
        .find(|r| r.pattern.binary == parsed.binary && flags_match(&r.pattern.flags, &parsed.flags))
}

/// Subset match: every flag declared by the rule must appear in the parsed
/// command's flags. Extra parsed flags are fine — e.g. rule `[-r, -f]` matches
/// parsed `[-r, -f, -v]`.
fn flags_match(rule_flags: &[String], parsed_flags: &[String]) -> bool {
    rule_flags.iter().all(|f| parsed_flags.contains(f))
}
