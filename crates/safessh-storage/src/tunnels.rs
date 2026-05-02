//! On-disk store of currently-active tunnels.
//!
//! One TOML file per tunnel: `state/tunnels/<id>.toml`. The supervisor
//! creates it on `forward` open and removes it on close. `tunnels list`
//! enumerates these files; `tunnels close <id>` SIGTERMs the supervisor
//! pid recorded inside.
//!
//! Atomic writes go through [`atomic::write_string`]
//! (SAFETY-INVARIANT-5). `reap_dead` is best-effort cleanup for the case
//! where a supervisor crashed before it could remove its own file: we
//! poll each `supervisor_pid` with `kill(pid, 0)` and discard records
//! whose process is gone.

use crate::atomic;
use crate::paths::Paths;
use safessh_core::error::{Error, Result};
use safessh_core::tunnel::{TunnelId, TunnelRecord};
use std::path::PathBuf;

pub struct TunnelStore {
    dir: PathBuf,
}

impl TunnelStore {
    pub fn new(paths: &Paths) -> Self {
        Self {
            dir: paths.tunnels_dir(),
        }
    }

    pub fn add(&self, record: &TunnelRecord) -> Result<()> {
        let path = self.dir.join(format!("{}.toml", record.id.as_str()));
        let toml = toml::to_string_pretty(record).map_err(|e| Error::Serde(e.to_string()))?;
        atomic::write_string(&path, &toml)?;
        Ok(())
    }

    pub fn get(&self, id: &TunnelId) -> Result<Option<TunnelRecord>> {
        let path = self.dir.join(format!("{}.toml", id.as_str()));
        match std::fs::read_to_string(&path) {
            Ok(raw) => toml::from_str(&raw)
                .map(Some)
                .map_err(|e| Error::Storage(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Io(e)),
        }
    }

    pub fn list_all(&self) -> Result<Vec<TunnelRecord>> {
        if !self.dir.exists() {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(entry.path()) {
                if let Ok(rec) = toml::from_str::<TunnelRecord>(&raw) {
                    out.push(rec);
                }
            }
        }
        out.sort_by_key(|r| r.opened_at);
        Ok(out)
    }

    pub fn remove(&self, id: &TunnelId) -> Result<TunnelRecord> {
        let path = self.dir.join(format!("{}.toml", id.as_str()));
        let raw = std::fs::read_to_string(&path)
            .map_err(|_| Error::Storage(format!("no such tunnel: {}", id.as_str())))?;
        let rec: TunnelRecord =
            toml::from_str(&raw).map_err(|e| Error::Storage(e.to_string()))?;
        std::fs::remove_file(&path).ok();
        Ok(rec)
    }

    /// Drop records whose `supervisor_pid` is no longer alive. Returns the
    /// IDs of the records that were removed.
    pub fn reap_dead(&self) -> Result<Vec<TunnelId>> {
        let mut reaped = vec![];
        for rec in self.list_all()? {
            if !pid_alive(rec.supervisor_pid) {
                let path = self.dir.join(format!("{}.toml", rec.id.as_str()));
                let _ = std::fs::remove_file(&path);
                reaped.push(rec.id);
            }
        }
        Ok(reaped)
    }
}

fn pid_alive(pid: i32) -> bool {
    // `kill(pid, 0)` with no signal probes whether the PID exists and
    // we have permission to signal it; it never actually delivers a
    // signal. ESRCH means dead; EPERM means alive but ours-not-to-signal.
    // We treat both Ok and EPERM as "alive". On non-Unix this falls back
    // to a conservative `true` so the file isn't dropped under a debugger.
    #[cfg(unix)]
    {
        use nix::errno::Errno;
        use nix::unistd::Pid;
        match nix::sys::signal::kill(Pid::from_raw(pid), None) {
            Ok(_) => true,
            Err(Errno::EPERM) => true,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}
