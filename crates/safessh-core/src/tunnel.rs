//! Tunnel value types — id, spec, state record, close reasons.
//!
//! Pure data; no I/O, no policy logic. Downstream crates pick these up
//! via `safessh-core` so the on-disk schema and the audit-event payload
//! agree on field names without each crate redefining them.

use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt;

/// 8-character alphanumeric identifier for an active tunnel.
///
/// Alphabet excludes `0`, `1`, `l`, `o` for human readability — same
/// shape as `ApprovalToken` but a different length so the two cannot
/// be confused at a glance in audit logs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TunnelId(String);

impl TunnelId {
    pub fn generate() -> Self {
        const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
        let mut rng = rand::thread_rng();
        let id: String = (0..8)
            .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
            .collect();
        Self(id)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        Self(s.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TunnelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `<local_port>:<remote_host>:<remote_port>` — the only forwarding
/// shape v0.4 supports. Mirrors the `ssh -L` argument.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelSpec {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
}

impl TunnelSpec {
    pub fn parse(s: &str) -> Result<Self, TunnelSpecError> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 3 {
            return Err(TunnelSpecError::Shape);
        }
        let local_port: u16 = parts[0]
            .parse()
            .map_err(|_| TunnelSpecError::LocalPort(parts[0].into()))?;
        if local_port == 0 {
            return Err(TunnelSpecError::LocalPort(parts[0].into()));
        }
        let remote_host = parts[1].to_string();
        if remote_host.is_empty() {
            return Err(TunnelSpecError::RemoteHost);
        }
        let remote_port: u16 = parts[2]
            .parse()
            .map_err(|_| TunnelSpecError::RemotePort(parts[2].into()))?;
        if remote_port == 0 {
            return Err(TunnelSpecError::RemotePort(parts[2].into()));
        }
        Ok(Self {
            local_port,
            remote_host,
            remote_port,
        })
    }
}

impl fmt::Display for TunnelSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.local_port, self.remote_host, self.remote_port)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TunnelSpecError {
    #[error("expected local_port:remote_host:remote_port")]
    Shape,
    #[error("invalid local port: {0}")]
    LocalPort(String),
    #[error("remote host must be non-empty")]
    RemoteHost,
    #[error("invalid remote port: {0}")]
    RemotePort(String),
}

/// Why a tunnel closed. Serialized as `kebab-case` so audit JSONL is
/// stable across language clients.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TunnelCloseReason {
    /// `output.tunnel_ttl_minutes` elapsed while the supervisor was alive.
    TtlExpired,
    /// User invoked `safessh tunnels close <id>`.
    UserClose,
    /// The `ssh -L … -N` child exited on its own (network drop, server kill).
    SshDied,
    /// The supervisor itself received SIGTERM (e.g. host shutdown, manual kill).
    ParentShutdown,
    /// The ssh child failed to spawn / authenticate at all — supervisor never
    /// reached steady state. Lifecycle-wise the tunnel never opened, but we
    /// still record the close so list/close can clean up the file.
    FailedToStart,
}

/// What `state/tunnels/<id>.toml` holds while a tunnel is alive.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TunnelRecord {
    pub id: TunnelId,
    pub project: String,
    pub target: String,
    pub spec: TunnelSpec,
    pub ssh_pid: i32,
    /// PID of the daemonized safessh supervisor process. `tunnels close <id>`
    /// SIGTERMs this PID; the supervisor handles its child cleanup +
    /// `tunnel_close` audit.
    pub supervisor_pid: i32,
    pub opened_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
