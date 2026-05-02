//! `safessh <project> read <path>` — headless end-to-end flow.
//!
//! Wires the CLI through `safessh-storage` (project lookup) →
//! `file_common::decide_file_op` (policy + audit) →
//! `SshDriver::read_file` → `Redactor` → framed stdout →
//! `file_read_complete` audit event.
//!
//! # Safety invariants
//!
//! * **SAFETY-INVARIANT-4:** the `file_read` attempt audit event is written
//!   (inside `decide_file_op`) **before** any user-visible stdout/stderr
//!   disclosure. This module never prints the framed block before
//!   `decide_file_op` has returned successfully.
//!
//! # Policy outcomes
//!
//! * `Allow`  → proceed to SFTP read, frame stdout, exit 0 (or 30 on truncation).
//! * `RequireApproval` → emit `BLOCKED:<token>` on stderr, exit 10, no SSH call.
//! * `Deny`   → exit 12, no SSH call.
//! * `Block`  → exit 11, no SSH call.

use crate::output;
use safessh_audit::event;
use safessh_audit::jsonl::AuditWriter;
use safessh_core::error::{Error, Result};
use safessh_core::redactor::Redactor;
use safessh_core::types::PolicyDecision;
use safessh_ssh::driver::SshDriver;
use safessh_ssh::openssh::OpenSshDriver;
use safessh_storage::paths::Paths;
use safessh_storage::project::ProjectStore;
use std::sync::Arc;
use std::time::Instant;

/// Parse argv from `TopCmd::External(...)` and dispatch the read flow.
///
/// `args[0]` is the project name, `args[1]` is the literal `"read"`, and
/// `args[2]` is the remote path. Any other shape returns [`Error::Usage`].
///
/// `--on <target>` anywhere in argv selects a named target; absent it, the
/// project's `default_target` is used.
// Wired into `main.rs` in Task 10; allow until then.
#[allow(dead_code)]
pub async fn run(args: Vec<String>) -> Result<()> {
    let paths = Paths::user().map_err(Error::Io)?;
    let driver =
        Arc::new(OpenSshDriver::new(paths.cache.join("control-sockets"))?) as Arc<dyn SshDriver>;
    run_with_driver_and_paths(args, driver, paths).await
}

/// Thin shim used when the caller already constructed `Paths::user()`.
#[allow(dead_code)]
pub async fn run_with_driver(args: Vec<String>, driver: Arc<dyn SshDriver>) -> Result<()> {
    let paths = Paths::user().map_err(Error::Io)?;
    run_with_driver_and_paths(args, driver, paths).await
}

/// Core implementation shared by `run` and `run_with_driver`.
pub async fn run_with_driver_and_paths(
    args: Vec<String>,
    driver: Arc<dyn SshDriver>,
    paths: Paths,
) -> Result<()> {
    let (args, on_target) = parse_read_extras(args);

    if args.len() < 3 || args[1] != "read" {
        return Err(Error::Usage(
            "expected: safessh <project> read <path>".into(),
        ));
    }
    let project_name = args[0].clone();
    let path = args[2].clone();

    paths.ensure_dirs().map_err(Error::Io)?;

    let project = ProjectStore::new(paths.clone()).load(&project_name)?;

    // SAFETY-INVARIANT-4: audit writer opened before any user-visible output.
    let writer = AuditWriter::open(&paths)?;

    // decide_file_op writes the attempt audit event before returning.
    let decision = super::file_common::decide_file_op(
        &paths,
        &project,
        &project_name,
        super::file_common::FileKind::Read,
        &path,
        &writer,
    )?;

    match &decision {
        PolicyDecision::Allow { .. } => { /* fall through to SFTP read */ }
        PolicyDecision::RequireApproval {
            token, categories, ..
        } => {
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

    // Resolve target; --on <name> overrides default_target.
    let want = on_target
        .as_deref()
        .unwrap_or(project.default_target.as_str());
    let target = project
        .targets
        .iter()
        .find(|t| t.name() == want)
        .ok_or_else(|| Error::Usage(format!("no such target: {want}")))?;

    let start = Instant::now();
    let result = driver
        .read_file(target, &path, project.output.file_read_cap_bytes)
        .await?;
    let duration_ms = start.elapsed().as_millis() as u64;

    // SAFETY-INVARIANT-6 (per Redactor): redact before framing.
    let r = Redactor::default();
    let (redacted, _) = r.redact(&result.bytes);

    output::write_framed(&redacted, b"", 0, duration_ms, result.truncated);

    // SAFETY-INVARIANT-4: complete audit event written after output framing.
    writer.append(&event::file_read_complete(
        &project_name,
        target.name(),
        &result.canonical_path,
        redacted.len() as u64,
        &super::file_common::sha256_hex(&redacted),
        result.truncated,
        duration_ms,
    ))?;

    if result.truncated {
        // Exit 30 signals truncation to the calling agent.
        std::process::exit(30);
    }
    Ok(())
}

/// Strip `--on <target>` (or `--on=<target>`) from the argv slice.
///
/// Returns `(remaining_args, on_target_value)`.
// pub(super) is sufficient visibility; allow lint until Task 10 wires dispatch.
#[allow(dead_code)]
pub(super) fn parse_read_extras(args: Vec<String>) -> (Vec<String>, Option<String>) {
    let mut filtered: Vec<String> = Vec::with_capacity(args.len());
    let mut on_target: Option<String> = None;
    let mut iter = args.into_iter();
    while let Some(a) = iter.next() {
        if a == "--on" {
            on_target = iter.next();
            continue;
        }
        if let Some(rest) = a.strip_prefix("--on=") {
            on_target = Some(rest.to_string());
            continue;
        }
        filtered.push(a);
    }
    (filtered, on_target)
}
