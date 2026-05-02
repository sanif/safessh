//! `safessh <project> write <path>` — headless end-to-end flow.
//!
//! Reads bytes from stdin (up to `file_write_cap_bytes`) and uploads them to
//! the remote path via SFTP. Symmetric to `read.rs` but with NO redaction —
//! bytes go up exactly as received.
//!
//! # Safety invariants
//!
//! * **SAFETY-INVARIANT-4:** the `file_write` attempt audit event is written
//!   (inside `decide_file_op`) **before** any SSH I/O. This module never
//!   starts the SFTP upload before `decide_file_op` has returned successfully.
//!
//! * **SAFETY-INVARIANT-5:** no bytes written after the cap is exceeded; the
//!   call returns `Ok(true)` (truncated) without calling the driver at all.
//!
//! # Policy outcomes
//!
//! * `Allow`  → proceed to SFTP write, exit 0 (or 30 on cap exceeded).
//! * `RequireApproval` → emit `BLOCKED:<token>` on stderr, exit 10, no SSH call.
//! * `Deny`   → exit 12, no SSH call.
//! * `Block`  → exit 11, no SSH call.

use safessh_audit::event;
use safessh_audit::jsonl::AuditWriter;
use safessh_core::error::{Error, Result};
use safessh_core::types::PolicyDecision;
use safessh_ssh::driver::SshDriver;
use safessh_ssh::openssh::OpenSshDriver;
use safessh_storage::paths::Paths;
use safessh_storage::project::ProjectStore;
use std::sync::Arc;
use std::time::Instant;

/// Parse argv from `TopCmd::External(...)` and dispatch the write flow.
///
/// `args[0]` is the project name, `args[1]` is the literal `"write"`, and
/// `args[2]` is the remote path. Any other shape returns [`Error::Usage`].
///
/// Bytes are read from stdin up to `file_write_cap_bytes`; if exceeded, returns
/// `Ok(true)` (caller should exit 30) without uploading anything.
///
/// `--on <target>` anywhere in argv selects a named target; absent it, the
/// project's `default_target` is used. `--yolo` anywhere in argv (or the
/// top-level `yolo` flag) bypasses the policy engine; the global `disable_yolo`
/// config setting short-circuits this with [`Error::YoloRefused`] (exit 13).
///
/// Returns `true` when stdin exceeded the cap (caller should exit 30).
pub async fn run(args: Vec<String>, yolo: bool) -> Result<bool> {
    let paths = Paths::user().map_err(Error::Io)?;
    let driver =
        Arc::new(OpenSshDriver::new(paths.cache.join("control-sockets"))?) as Arc<dyn SshDriver>;
    run_with_driver(args, yolo, driver).await
}

/// Thin shim that reads stdin then delegates to the testable inner function.
pub async fn run_with_driver(
    args: Vec<String>,
    yolo: bool,
    driver: Arc<dyn SshDriver>,
) -> Result<bool> {
    let paths = Paths::user().map_err(Error::Io)?;
    let bytes_in = read_stdin_bounded(u64::MAX).await?;
    run_with_driver_and_paths_and_bytes(args, yolo, driver, paths, bytes_in).await
}

/// Core implementation shared by `run_with_driver` and the integration tests.
///
/// Takes `bytes_in` directly so tests can inject arbitrary payloads without
/// touching stdin. Returns `Ok(true)` when stdin exceeded the cap (truncated),
/// `Ok(false)` for a successful complete write.
pub async fn run_with_driver_and_paths_and_bytes(
    args: Vec<String>,
    yolo: bool,
    driver: Arc<dyn SshDriver>,
    paths: Paths,
    bytes_in: Vec<u8>,
) -> Result<bool> {
    let (args, yolo_in_args, on_target) = parse_write_extras(args);
    let yolo = yolo || yolo_in_args;

    if args.len() < 3 || args[1] != "write" {
        return Err(Error::Usage(
            "expected: safessh <project> write <path>".into(),
        ));
    }
    let project_name = args[0].clone();
    let path = args[2].clone();

    paths.ensure_dirs().map_err(Error::Io)?;

    // SAFETY: yolo respects the global `disable_yolo` kill switch before any
    // project lookup or policy evaluation.
    if yolo {
        let cfg = safessh_storage::config::load(&paths).unwrap_or_default();
        if cfg.disable_yolo {
            return Err(Error::YoloRefused);
        }
    }

    let project = ProjectStore::new(paths.clone()).load(&project_name)?;

    // Check the cap before opening the audit writer. If exceeded, write a
    // truncated-complete event and return Ok(true) — no driver call.
    let cap = project.output.file_write_cap_bytes as usize;
    if bytes_in.len() > cap {
        // SAFETY-INVARIANT-4: audit-write before returning (no user-visible output follows).
        // SAFETY-INVARIANT-5: no bytes sent to driver when over cap.
        let writer = AuditWriter::open(&paths)?;
        writer.append(&event::file_write_complete(
            &project_name,
            "",
            &path,
            0,
            "",
            true,
            0,
        ))?;
        return Ok(true);
    }

    // SAFETY-INVARIANT-4: audit writer opened before any user-visible output.
    let writer = AuditWriter::open(&paths)?;

    if yolo {
        // Bypass: skip the policy engine entirely. Audit the bypass.
        // SAFETY-INVARIANT-7: yolo only bypasses the policy engine; audit
        // logging still occurs (and logs MORE — the yolo_invocation event).
        writer.append(&event::yolo_invocation(
            &project_name,
            &format!("write {path}"),
        ))?;
    } else {
        // decide_file_op writes the attempt audit event before returning.
        let decision = super::file_common::decide_file_op(
            &paths,
            &project,
            &project_name,
            super::file_common::FileKind::Write,
            &path,
            &writer,
        )?;

        match &decision {
            PolicyDecision::Allow { .. } => { /* fall through to SFTP write */ }
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

    // sha256 is over the bytes-as-written (no redaction on write).
    let sha = super::file_common::sha256_hex(&bytes_in);
    let bytes_len = bytes_in.len() as u64;

    let start = Instant::now();
    let result = driver.write_file(target, &path, &bytes_in).await?;
    let duration_ms = start.elapsed().as_millis() as u64;

    // SAFETY-INVARIANT-4: complete audit event written after the upload.
    writer.append(&event::file_write_complete(
        &project_name,
        target.name(),
        &result.canonical_path,
        bytes_len,
        &sha,
        false,
        duration_ms,
    ))?;

    Ok(false)
}

/// Read at most `max_bytes + 1` bytes from stdin so callers can detect overflow.
///
/// In practice the write subcommand reads all of stdin before calling
/// `run_with_driver_and_paths_and_bytes`; tests skip this entirely by
/// calling the `_and_bytes` variant directly.
#[allow(dead_code)]
async fn read_stdin_bounded(max_bytes: u64) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut stdin = tokio::io::stdin();
    let mut buf = Vec::new();
    // Read one extra byte so callers can detect that the cap was exceeded.
    let limit = (max_bytes as usize).saturating_add(1);
    buf.reserve(limit.min(65_536));
    let mut chunk = [0u8; 8192];
    loop {
        let n = stdin.read(&mut chunk).await.map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > limit {
            break;
        }
    }
    Ok(buf)
}

/// Strip `--yolo`, `--on <target>` (or `--on=<target>`) from the argv slice.
///
/// Returns `(remaining_args, yolo_seen, on_target_value)`.
pub(super) fn parse_write_extras(
    args: Vec<String>,
) -> (Vec<String>, bool, Option<String>) {
    let mut filtered: Vec<String> = Vec::with_capacity(args.len());
    let mut yolo = false;
    let mut on_target: Option<String> = None;
    let mut iter = args.into_iter();
    while let Some(a) = iter.next() {
        if a == "--yolo" {
            yolo = true;
            continue;
        }
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
    (filtered, yolo, on_target)
}
