//! In-memory `SshDriver` implementation for unit tests.
//!
//! Tests register a `CannedResponse` for a `(target_name, command)` pair via
//! [`MockDriver::with_response`]. The first matching `exec` call consumes the
//! response and replays its stdout/stderr through the streaming callback.
//! Unmatched calls succeed with an empty, exit-0 response so callers don't
//! need to register every dummy invocation.

use crate::driver::{ExecResult, OutputChunk, SshDriver};
use async_trait::async_trait;
use safessh_core::error::Result;
use safessh_storage::project::Target;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct MockDriver {
    responses: Mutex<HashMap<(String, String), CannedResponse>>,
}

#[derive(Debug, Clone)]
pub struct CannedResponse {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit: i32,
}

impl MockDriver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a canned response for `(target_name, command)`. The next
    /// matching `exec` call consumes it.
    pub fn with_response(&self, target_name: &str, command: &str, r: CannedResponse) {
        self.responses
            .lock()
            .expect("MockDriver mutex poisoned")
            .insert((target_name.to_string(), command.to_string()), r);
    }
}

#[async_trait]
impl SshDriver for MockDriver {
    async fn exec<'a>(
        &'a self,
        target: &'a Target,
        command: &'a str,
        _stdout_cap: u64,
        _stderr_cap: u64,
        mut on_chunk: Box<dyn FnMut(OutputChunk) + Send + 'a>,
    ) -> Result<ExecResult> {
        let key = (target.name().to_string(), command.to_string());
        let response = self
            .responses
            .lock()
            .expect("MockDriver mutex poisoned")
            .remove(&key)
            .unwrap_or(CannedResponse {
                stdout: Vec::new(),
                stderr: Vec::new(),
                exit: 0,
            });

        let stdout_len = response.stdout.len() as u64;
        let stderr_len = response.stderr.len() as u64;

        if !response.stdout.is_empty() {
            on_chunk(OutputChunk::Stdout(response.stdout));
        }
        if !response.stderr.is_empty() {
            on_chunk(OutputChunk::Stderr(response.stderr));
        }

        Ok(ExecResult {
            exit_code: response.exit,
            stdout_bytes: stdout_len,
            stderr_bytes: stderr_len,
            duration_ms: 0,
            truncated: false,
        })
    }
}
