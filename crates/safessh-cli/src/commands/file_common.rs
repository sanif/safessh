//! Shared helpers for the `read` and `write` subcommands.
//!
//! Centralizes the audit-write-before-output discipline (SAFETY-INVARIANT-4):
//! every decide() call appends an audit event before any user-visible output.
//! No SSH I/O happens in this module — driver calls live in `read.rs` / `write.rs`.

use safessh_audit::event;
use safessh_audit::jsonl::AuditWriter;
use safessh_core::error::Result;
use safessh_core::types::{ParsedCommand, PolicyDecision};
use safessh_policy::decision::{decide, DecisionInput, FileOp};
use safessh_storage::approvals::{AlwaysStore, BlockedStore, PendingRequest, PendingStore, TimedStore};
use safessh_storage::paths::Paths;
use safessh_storage::policies::preset_file_rules;
use safessh_storage::project::Project;
use sha2::{Digest, Sha256};

/// What kind of file op the caller is performing — drives audit event-type.
// `Write` is used by `write.rs` (Task 9); `Read` by `read.rs` (Task 8).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum FileKind {
    Read,
    Write,
}

impl FileKind {
    pub fn attempt_event_type(&self) -> &'static str {
        match self {
            FileKind::Read => "file_read",
            FileKind::Write => "file_write",
        }
    }

    // Used by `write.rs` (Task 9).
    #[allow(dead_code)]
    pub fn complete_event_type(&self) -> &'static str {
        match self {
            FileKind::Read => "file_read_complete",
            FileKind::Write => "file_write_complete",
        }
    }

    /// The `file:read` / `file:write` category string for this op kind.
    fn category(&self) -> &'static str {
        match self {
            FileKind::Read => "file:read",
            FileKind::Write => "file:write",
        }
    }
}

/// Run the policy engine for a file operation and write the attempt audit event.
///
/// Loads always/timed/blocked rules for the project, builds a `DecisionInput`
/// with the appropriate `FileOp` and preset_file_rules, calls `decide()`, and
/// writes the attempt audit event **before** returning.
///
/// When the decision is `RequireApproval` and no TTY is attached (headless
/// path), a [`PendingRequest`] is persisted with the `path` field set so
/// the TUI Approvals screen can render and act on the file-op approval.
///
/// # Safety invariants
///
/// * **SAFETY-INVARIANT-4:** the attempt audit event is written here before
///   returning to the caller. No user-visible output (SSH I/O, framing) may
///   occur before this function is called and its event appended.
// Called by `read.rs` and `write.rs` (Task 9); dead-code lint fires on the
// binary target until Task 10 wires the dispatch.
#[allow(dead_code)]
pub fn decide_file_op(
    paths: &Paths,
    project: &Project,
    project_name: &str,
    kind: FileKind,
    path: &str,
    writer: &AuditWriter,
) -> Result<PolicyDecision> {
    let timed = TimedStore::new(paths);
    let always = AlwaysStore::new(paths);
    let blocked = BlockedStore::new(paths);

    // Purge expired before reading so list_active reflects post-expiry truth.
    let _ = timed.purge_expired(project_name);

    let timed_rules = timed.list_active(project_name).unwrap_or_default();
    let allow_rules = always.list(project_name).unwrap_or_default();
    let block_rules = blocked.list(project_name).unwrap_or_default();

    let op = match kind {
        FileKind::Read => FileOp::Read(path),
        FileKind::Write => FileOp::Write(path),
    };

    let (decision, _) = decide(DecisionInput {
        raw: "",
        policy: &project.policy,
        allows: &allow_rules,
        timed: &timed_rules,
        blocks: &block_rules,
        file_op: op,
        preset_file_rules: preset_file_rules(),
    });

    // SAFETY-INVARIANT-4: audit-write before any user-visible output.
    let evt = event::file_attempt(kind.attempt_event_type(), project_name, path, &decision);
    writer.append(&evt)?;

    // Persist a pending file for RequireApproval on the headless path so the
    // TUI Approvals screen can show the file-op row and act on it.
    if let PolicyDecision::RequireApproval {
        token, categories, ..
    } = &decision
    {
        if !atty::is(atty::Stream::Stdin) {
            let req = PendingRequest {
                token: token.as_str().to_string(),
                project: project_name.to_string(),
                categories: categories.clone(),
                // File-op approvals don't parse a shell command; use the
                // category as a synthetic binary-equivalent so the TUI can
                // still display something meaningful in the parsed slot.
                parsed: ParsedCommand {
                    binary: kind.category().to_string(),
                    flags: vec![],
                    args: vec![path.to_string()],
                    redirects: vec![],
                    pipes: vec![],
                    env_mutations: vec![],
                    raw: format!("{} {}", kind.category(), path),
                },
                raw: format!("{} {}", kind.category(), path),
                created_at: chrono::Utc::now(),
                path: Some(path.to_string()),
            };
            PendingStore::new(paths).add(&req)?;
        }
    }

    Ok(decision)
}

/// Compute the SHA-256 hex digest of `bytes`.
// Used by `read.rs` and `write.rs` (Task 9).
#[allow(dead_code)]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}
