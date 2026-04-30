//! `SshDriver` trait and shared exec types.
//!
//! Defining the abstraction here lets the rest of the workspace target a
//! single trait while alternative implementations (mock, OpenSSH subprocess,
//! future libssh-based driver, etc.) live in sibling modules.

use async_trait::async_trait;
use safessh_core::error::Result;
use safessh_storage::project::Target;

/// A single chunk of output streamed from the remote process.
#[derive(Debug, Clone)]
pub enum OutputChunk {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

/// Summary returned once an exec has completed.
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub duration_ms: u64,
    pub truncated: bool,
}

/// Abstraction over "run a command on a target and stream its output".
///
/// Implementations are expected to be cheap to clone or share via `Arc`. The
/// `on_chunk` callback receives streamed output as it arrives; the lifetime
/// `'a` ties the callback to this single `exec` call so it may borrow caller
/// state (e.g. an output sink) without requiring the future itself to be
/// `Send`-static.
#[async_trait]
pub trait SshDriver: Send + Sync {
    async fn exec<'a>(
        &'a self,
        target: &'a Target,
        command: &'a str,
        stdout_cap: u64,
        stderr_cap: u64,
        on_chunk: Box<dyn FnMut(OutputChunk) + Send + 'a>,
    ) -> Result<ExecResult>;
}
