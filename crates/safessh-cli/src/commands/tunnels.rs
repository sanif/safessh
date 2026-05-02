//! `safessh tunnels list / close <id>` — read-only inspection and
//! cooperative shutdown.
//!
//! `close` SIGTERMs the supervisor PID and polls the state file every
//! 50 ms; the supervisor's signal handler races to the close path,
//! writes `tunnel_close`, and removes the file before exiting. We poll
//! up to 5 s; if the file is still present we fall back to SIGKILL on
//! the supervisor as a last resort and emit a `tunnel_close` ourselves
//! with reason `parent-shutdown` so the audit trail is consistent.

use crate::cli::TunnelsCmd;
use chrono::Utc;
use safessh_audit::event;
use safessh_audit::jsonl::AuditWriter;
use safessh_core::error::{Error, Result};
use safessh_core::tunnel::{TunnelCloseReason, TunnelId};
use safessh_storage::paths::Paths;
use safessh_storage::tunnels::TunnelStore;
use std::time::Duration as StdDuration;

pub fn run(cmd: TunnelsCmd) -> Result<()> {
    let paths = Paths::user().map_err(Error::Io)?;
    paths.ensure_dirs().map_err(Error::Io)?;
    let store = TunnelStore::new(&paths);

    match cmd {
        TunnelsCmd::List => {
            let _ = store.reap_dead();
            let active = store.list_all()?;
            if active.is_empty() {
                println!("(no active tunnels)");
                return Ok(());
            }
            println!(
                "{:<8}  {:<12}  {:<12}  {:<24}  {:<8}",
                "ID", "PROJECT", "TARGET", "FORWARD", "EXPIRES"
            );
            for t in active {
                let remaining = t.expires_at - Utc::now();
                println!(
                    "{:<8}  {:<12}  {:<12}  {:<24}  {:>5} min",
                    t.id.as_str(),
                    t.project,
                    t.target,
                    format!(
                        "localhost:{} → {}:{}",
                        t.spec.local_port, t.spec.remote_host, t.spec.remote_port
                    ),
                    remaining.num_minutes().max(0)
                );
            }
            Ok(())
        }
        TunnelsCmd::Close { id } => {
            let id = TunnelId::from_str(&id);
            let rec = store.get(&id)?.ok_or_else(|| {
                Error::ProjectNotFound(format!("no such tunnel: {}", id.as_str()))
            })?;
            #[cfg(unix)]
            {
                use nix::sys::signal::{kill, Signal};
                use nix::unistd::Pid;
                let _ = kill(Pid::from_raw(rec.supervisor_pid), Signal::SIGTERM);
            }
            // Poll up to 5s for the supervisor's own cleanup.
            let deadline = std::time::Instant::now() + StdDuration::from_secs(5);
            while std::time::Instant::now() < deadline {
                if store.get(&id)?.is_none() {
                    println!("closed tunnel {}", id.as_str());
                    return Ok(());
                }
                std::thread::sleep(StdDuration::from_millis(50));
            }
            // Fallback: supervisor unresponsive. SIGKILL it ourselves and
            // emit the audit close so the trail is consistent.
            #[cfg(unix)]
            {
                use nix::sys::signal::{kill, Signal};
                use nix::unistd::Pid;
                let _ = kill(Pid::from_raw(rec.supervisor_pid), Signal::SIGKILL);
            }
            let writer = AuditWriter::open(&paths)?;
            let duration = (Utc::now() - rec.opened_at).num_seconds().max(0) as u64;
            writer.append(&event::tunnel_close(
                &rec.project,
                &rec.id,
                TunnelCloseReason::ParentShutdown,
                duration,
            ))?;
            let _ = store.remove(&id);
            println!(
                "force-closed tunnel {} (supervisor unresponsive)",
                id.as_str()
            );
            Ok(())
        }
    }
}
