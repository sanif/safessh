//! `safessh <project> exec "<command>"` — headless end-to-end flow.
//!
//! Wires the CLI through `safessh-storage` (project + approvals) →
//! `safessh-policy::decide` → `safessh-audit::AuditWriter` →
//! `safessh-ssh::OpenSshDriver` → framed stdout via [`crate::output`].
//!
//! # Safety invariants
//!
//! * **SAFETY-INVARIANT-4:** the `exec_attempt` / `approval_requested` /
//!   `yolo_invocation` audit event is written **before** any user-visible
//!   stdout/stderr disclosure. This module always opens the [`AuditWriter`]
//!   and calls `append` for the gating event prior to running the SSH driver
//!   or printing the framed block.
//!
//! Three flow paths:
//!
//! * **Headless** (no TTY, `--yolo` off): persist a [`PendingRequest`], emit
//!   the structured `BLOCKED:` token via `Error::ApprovalRequired`, and exit
//!   so an agent can parse and recover. (Task 20.)
//! * **TTY** (Task 21): present a five-action `dialoguer` prompt via
//!   [`crate::prompt::ask`] and apply the user's choice in-process.
//! * **Yolo** (Task 23): bypass the policy engine entirely, audit the bypass
//!   as `yolo_invocation`, and proceed straight to exec. The global
//!   `disable_yolo` config flag short-circuits this path with
//!   [`Error::YoloRefused`] (exit 13) **before** any project lookup.

use crate::output;
use crate::prompt::{self, PromptAction};
use chrono::{Duration, Utc};
use safessh_audit::event;
use safessh_audit::jsonl::AuditWriter;
use safessh_core::error::{Error, Result};
use safessh_core::redactor::Redactor;
use safessh_core::types::{ParsedCommand, PolicyDecision};
use safessh_policy::{decide, DecisionInput};
use safessh_ssh::driver::{OutputChunk, SshDriver};
use safessh_ssh::openssh::OpenSshDriver;
use safessh_storage::approvals::{
    AlwaysStore, BlockedStore, PatternRule, PendingRequest, PendingStore, TimedRule, TimedStore,
};
use safessh_storage::paths::Paths;
use safessh_storage::project::{Project, ProjectStore};
use std::sync::{Arc, Mutex};

/// Parse argv from `TopCmd::External(...)` and dispatch the exec flow.
///
/// `args[0]` is the project name, `args[1]` is the literal `"exec"`, and
/// `args[2]` is the raw command string. Any other shape returns
/// [`Error::Usage`].
///
/// When `yolo` is `true`, the policy engine is skipped entirely. The global
/// `disable_yolo` config flag is checked **first** (before any project
/// lookup) and short-circuits with [`Error::YoloRefused`] (exit 13) when set.
/// Output framing and redaction still apply on the yolo path; a
/// `yolo_invocation` audit event is written before any user-visible output.
pub async fn run(args: Vec<String>, yolo: bool) -> Result<()> {
    // `--yolo` is declared as a top-level global flag, but clap's
    // `external_subcommand` capture passes argv through verbatim — so when
    // the user writes `safessh prod exec --yolo "..."` the flag arrives here
    // inside `args`, not parsed onto `Cli::yolo`. Strip it (anywhere in the
    // external argv) and OR it with the top-level value so both placements
    // work identically.
    let mut filtered: Vec<String> = Vec::with_capacity(args.len());
    let mut yolo_in_args = false;
    for a in args {
        if a == "--yolo" {
            yolo_in_args = true;
        } else {
            filtered.push(a);
        }
    }
    let args = filtered;
    let yolo = yolo || yolo_in_args;

    if args.len() < 3 || args[1] != "exec" {
        return Err(Error::Usage(
            "expected: safessh <project> exec \"<command>\"".into(),
        ));
    }
    let project_name = args[0].clone();
    let raw_command = args[2].clone();

    // Build paths once and share with every store; ensures the layout exists
    // for first-run users.
    let paths = Paths::user().map_err(Error::Io)?;
    paths.ensure_dirs().map_err(Error::Io)?;

    // SAFETY: yolo respects the global `disable_yolo` kill switch first,
    // before any project lookup or policy evaluation. Even an unknown project
    // name exits 13 (not "project not found") when yolo is requested but
    // globally disabled — the user explicitly asked to bypass policy and we
    // explicitly refuse.
    if yolo {
        let cfg = safessh_storage::config::load(&paths).unwrap_or_default();
        if cfg.disable_yolo {
            return Err(Error::YoloRefused);
        }
    }

    let project = ProjectStore::new(Paths::user().map_err(Error::Io)?).load(&project_name)?;

    // SAFETY-INVARIANT-4: audit writer is opened before any user-visible
    // output. Each branch below appends its gating event before printing.
    let writer = AuditWriter::open(&paths)?;

    if yolo {
        // Bypass: skip the policy engine entirely. Audit the bypass with the
        // raw command so the trail still captures intent. Output framing +
        // redactor still apply below in `exec_and_frame`.
        writer.append(&event::yolo_invocation(&project_name, &raw_command))?;
    } else {
        decide_and_record(&paths, &project, &project_name, &raw_command, &writer)?;
    }

    exec_and_frame(&paths, &project, &project_name, &raw_command, &writer).await
}

/// Run the policy engine and apply its decision. Returns `Ok(())` if the
/// caller should proceed to exec; otherwise propagates an [`Error`] variant
/// matching the policy outcome (`Blocked`, `Denied`, `ApprovalRequired`).
fn decide_and_record(
    paths: &Paths,
    project: &Project,
    project_name: &str,
    raw_command: &str,
    writer: &AuditWriter,
) -> Result<()> {
    let pending = PendingStore::new(paths);
    let timed = TimedStore::new(paths);
    let always = AlwaysStore::new(paths);
    let blocked = BlockedStore::new(paths);

    // Purge expired before reading so `list_active` reflects post-expiry truth.
    let _ = timed.purge_expired(project_name);

    let timed_rules = timed.list_active(project_name).unwrap_or_default();
    let allow_rules = always.list(project_name).unwrap_or_default();
    let block_rules = blocked.list(project_name).unwrap_or_default();

    let (decision, parsed) = decide(DecisionInput {
        raw: raw_command,
        policy: &project.policy,
        allows: &allow_rules,
        timed: &timed_rules,
        blocks: &block_rules,
    });

    let parsed = parsed.unwrap_or_else(|| ParsedCommand {
        binary: "<unparsed>".into(),
        flags: vec![],
        args: vec![],
        redirects: vec![],
        pipes: vec![],
        env_mutations: vec![],
        raw: raw_command.to_string(),
    });

    match &decision {
        PolicyDecision::Allow { source, .. } => {
            writer.append(&event::exec_attempt(
                project_name,
                &parsed,
                &format!("{source:?}"),
            ))?;
        }
        PolicyDecision::RequireApproval {
            token, categories, ..
        } => {
            // SAFETY-INVARIANT-4: write the gating audit event before any
            // user-visible output (the dialoguer prompt prints to stderr)
            // or store mutation.
            writer.append(&event::approval_requested(
                project_name,
                token.as_str(),
                categories,
                raw_command,
            ))?;

            if atty::is(atty::Stream::Stdin) {
                // TTY path: ask the user inline, apply the action immediately.
                let action =
                    prompt::ask(&parsed, categories, project.approvals.timed_default_minutes)?;
                let pattern = PatternRule {
                    rule_id: format!("rule-{}", Utc::now().timestamp_millis()),
                    binary: parsed.binary.clone(),
                    flags: parsed.flags.clone(),
                    args_pattern: None,
                    categories: categories.clone(),
                    created_at: Utc::now(),
                };
                match action {
                    PromptAction::Once => { /* fall through to exec */ }
                    PromptAction::Timed(min) => {
                        timed.add(
                            project_name,
                            TimedRule {
                                pattern,
                                expires_at: Utc::now() + Duration::minutes(min as i64),
                            },
                        )?;
                    }
                    PromptAction::Always => {
                        always.add(project_name, pattern)?;
                    }
                    PromptAction::Deny => {
                        return Err(Error::Denied("user denied".into()));
                    }
                    PromptAction::Block => {
                        let rule_id = pattern.rule_id.clone();
                        blocked.add(project_name, pattern)?;
                        return Err(Error::Blocked {
                            rule_id,
                            reason: "user blocked".into(),
                        });
                    }
                }
                // Approved (Once/Timed/Always): record the proceed decision
                // for audit parity with the pure-Allow branch.
                writer.append(&event::exec_attempt(project_name, &parsed, "user-approved"))?;
            } else {
                // Headless path: persist pending and return the structured
                // deny token so an agent can call `safessh approve <token>`.
                let req = PendingRequest {
                    token: token.as_str().to_string(),
                    project: project_name.to_string(),
                    categories: categories.clone(),
                    parsed: parsed.clone(),
                    raw: raw_command.to_string(),
                    created_at: chrono::Utc::now(),
                };
                pending.add(&req)?;
                return Err(Error::ApprovalRequired {
                    token: token.as_str().to_string(),
                    categories: categories.clone(),
                });
            }
        }
        PolicyDecision::Block { rule_id, pattern } => {
            return Err(Error::Blocked {
                rule_id: rule_id.clone(),
                reason: pattern.clone(),
            });
        }
        PolicyDecision::Deny { reason } => {
            return Err(Error::Denied(reason.clone()));
        }
    }
    Ok(())
}

/// Resolve the project's default target, run the SSH driver, and emit the
/// framed stdout/stderr block (post-redaction). Shared by the policy-allowed
/// and yolo-bypass paths so output handling is identical for both.
async fn exec_and_frame(
    paths: &Paths,
    project: &Project,
    project_name: &str,
    raw_command: &str,
    writer: &AuditWriter,
) -> Result<()> {
    let target = project
        .targets
        .iter()
        .find(|t| t.name() == project.default_target)
        .ok_or_else(|| Error::Config(format!("no target named {}", project.default_target)))?;

    let driver = OpenSshDriver::new(paths.cache.join("control-sockets"))?;
    let stdout_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let stderr_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let so = stdout_buf.clone();
    let se = stderr_buf.clone();
    let result = driver
        .exec(
            target,
            raw_command,
            project.output.stdout_cap_bytes,
            project.output.stderr_cap_bytes,
            Box::new(move |c| match c {
                OutputChunk::Stdout(b) => so.lock().unwrap().extend_from_slice(&b),
                OutputChunk::Stderr(b) => se.lock().unwrap().extend_from_slice(&b),
            }),
        )
        .await?;

    // Redact every byte before it reaches the framed wrapper on stdout.
    let r = Redactor::default();
    let stdout_red = r.redact(&stdout_buf.lock().unwrap()).0;
    let stderr_red = r.redact(&stderr_buf.lock().unwrap()).0;
    output::write_framed(
        &stdout_red,
        &stderr_red,
        result.exit_code,
        result.duration_ms,
        result.truncated,
    );

    writer.append(&event::exec_complete(
        project_name,
        result.exit_code,
        result.stdout_bytes,
        result.stderr_bytes,
        result.duration_ms,
    ))?;

    if result.exit_code != 0 {
        std::process::exit(result.exit_code);
    }
    Ok(())
}
