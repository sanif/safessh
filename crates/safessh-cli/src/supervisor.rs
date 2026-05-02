//! Tunnel supervisor: holds the `ssh -L … -N` child + enforces TTL.
//!
//! Used in two shapes:
//!
//! 1. **Production** (`forward.rs::run_supervisor`, Task 9): the `forward`
//!    CLI command re-execs itself as a supervisor process. That process calls
//!    `run_inline` to block on TTL / SIGTERM / ssh-exit.
//!
//! 2. **Tests** (`run_inline`): same logic, no re-exec call.
//!
//! # Safety invariants
//!
//! * **SAFETY-INVARIANT-8 (tunnel TTL is hard):** when the `expires_at`
//!   deadline fires, the supervisor kills ssh and records `TtlExpired`. We
//!   never fall into an "approved forever" path.
//! * **SAFETY-INVARIANT-4 (audit before output):** `tunnel_close` is written
//!   via [`AuditWriter::append`] before the state file is removed, so a crash
//!   mid-cleanup leaves the audit trail intact.

use chrono::Utc;
use safessh_audit::event;
use safessh_audit::jsonl::AuditWriter;
use safessh_core::error::Result;
use safessh_core::tunnel::{TunnelCloseReason, TunnelRecord};
use safessh_ssh::driver::{TunnelExit, TunnelHandle};
use safessh_storage::paths::Paths;
use safessh_storage::tunnels::TunnelStore;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Run the supervisor loop without daemonizing — used by tests and by the
/// supervisor process in production after re-exec (Task 9).
pub async fn run_inline(
    paths: Paths,
    record: TunnelRecord,
    mut handle: Box<dyn TunnelHandle>,
    ttl: Duration,
    cancel: CancellationToken,
) -> Result<TunnelCloseReason> {
    let started = Utc::now();
    let reason = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            // SAFETY-INVARIANT-8: cancel signal → kill ssh child.
            handle.kill().await?;
            TunnelCloseReason::UserClose
        }
        _ = tokio::time::sleep(ttl) => {
            // SAFETY-INVARIANT-8: TTL deadline → kill ssh child.
            handle.kill().await?;
            TunnelCloseReason::TtlExpired
        }
        res = handle.wait() => {
            match res {
                Ok(TunnelExit::Killed) => TunnelCloseReason::UserClose,
                Ok(TunnelExit::Natural(_)) => TunnelCloseReason::SshDied,
                Err(_) => TunnelCloseReason::SshDied,
            }
        }
    };

    let duration_secs = (Utc::now() - started).num_seconds().max(0) as u64;
    // SAFETY-INVARIANT-4: audit close before removing the state file.
    let writer = AuditWriter::open(&paths)?;
    writer.append(&event::tunnel_close(
        &record.project,
        &record.id,
        reason,
        duration_secs,
    ))?;
    let _ = TunnelStore::new(&paths).remove(&record.id);
    Ok(reason)
}
