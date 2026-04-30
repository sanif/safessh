//! `safessh <project> exec "<command>"` — headless end-to-end flow.
//!
//! Wires the CLI through `safessh-storage` (project + approvals) →
//! `safessh-policy::decide` → `safessh-audit::AuditWriter` →
//! `safessh-ssh::OpenSshDriver` → framed stdout via [`crate::output`].
//!
//! # Safety invariants
//!
//! * **SAFETY-INVARIANT-4:** the `exec_attempt` / `approval_requested` audit
//!   event is written **before** any user-visible stdout/stderr disclosure.
//!   This module always opens the [`AuditWriter`] and calls `append` for the
//!   gating event prior to running the SSH driver or printing the framed
//!   block.
//!
//! TTY-prompt path lands in Task 21; this module's `RequireApproval` arm
//! always emits the structured `BLOCKED:` token (via `Error::ApprovalRequired`
//! in `errors::report_and_exit`) so headless agents can parse and recover.

use crate::output;
use safessh_audit::event;
use safessh_audit::jsonl::AuditWriter;
use safessh_core::error::{Error, Result};
use safessh_core::redactor::Redactor;
use safessh_core::types::{ParsedCommand, PolicyDecision};
use safessh_policy::{decide, DecisionInput};
use safessh_ssh::driver::{OutputChunk, SshDriver};
use safessh_ssh::openssh::OpenSshDriver;
use safessh_storage::approvals::{AlwaysStore, BlockedStore, PendingRequest, PendingStore, TimedStore};
use safessh_storage::paths::Paths;
use safessh_storage::project::ProjectStore;
use std::sync::{Arc, Mutex};

/// Parse argv from `TopCmd::External(...)` and dispatch the exec flow.
///
/// `args[0]` is the project name, `args[1]` is the literal `"exec"`, and
/// `args[2]` is the raw command string. Any other shape returns
/// [`Error::Usage`].
pub async fn run(args: Vec<String>) -> Result<()> {
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

    let project = ProjectStore::new(Paths::user().map_err(Error::Io)?).load(&project_name)?;

    let pending = PendingStore::new(&paths);
    let timed = TimedStore::new(&paths);
    let always = AlwaysStore::new(&paths);
    let blocked = BlockedStore::new(&paths);

    // Purge expired before reading so `list_active` reflects post-expiry truth.
    let _ = timed.purge_expired(&project_name);

    let timed_rules = timed.list_active(&project_name).unwrap_or_default();
    let allow_rules = always.list(&project_name).unwrap_or_default();
    let block_rules = blocked.list(&project_name).unwrap_or_default();

    let (decision, parsed) = decide(DecisionInput {
        raw: &raw_command,
        policy: &project.policy,
        allows: &allow_rules,
        timed: &timed_rules,
        blocks: &block_rules,
    });

    // SAFETY-INVARIANT-4: audit writer is opened before any user-visible
    // output. Each branch below appends its gating event before printing.
    let writer = AuditWriter::open(&paths)?;

    let parsed = parsed.unwrap_or_else(|| ParsedCommand {
        binary: "<unparsed>".into(),
        flags: vec![],
        args: vec![],
        redirects: vec![],
        pipes: vec![],
        env_mutations: vec![],
        raw: raw_command.clone(),
    });

    match &decision {
        PolicyDecision::Allow { source, .. } => {
            writer.append(&event::exec_attempt(
                &project_name,
                &parsed,
                &format!("{source:?}"),
            ))?;
        }
        PolicyDecision::RequireApproval {
            token, categories, ..
        } => {
            let req = PendingRequest {
                token: token.as_str().to_string(),
                project: project_name.clone(),
                categories: categories.clone(),
                parsed: parsed.clone(),
                raw: raw_command.clone(),
                created_at: chrono::Utc::now(),
            };
            // Persist the pending request BEFORE returning the structured
            // deny so `safessh approve <token>` has something to consume.
            pending.add(&req)?;
            writer.append(&event::approval_requested(
                &project_name,
                token.as_str(),
                categories,
                &raw_command,
            ))?;
            return Err(Error::ApprovalRequired {
                token: token.as_str().to_string(),
                categories: categories.clone(),
            });
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

    // Allowed. Resolve the target and execute.
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
            &raw_command,
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
        &project_name,
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
