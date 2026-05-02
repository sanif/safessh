//! In-memory `SshDriver` implementation for unit tests.
//!
//! Tests register a `CannedResponse` for a `(target_name, command)` pair via
//! [`MockDriver::with_response`]. The first matching `exec` call consumes the
//! response and replays its stdout/stderr through the streaming callback.
//! Unmatched calls succeed with an empty, exit-0 response so callers don't
//! need to register every dummy invocation.
//!
//! The file-map methods (`put_file`, `read_file`, `write_file`) use an
//! in-memory `BTreeMap` keyed by `(target_name, path)`.

use crate::driver::{ExecResult, FileReadResult, FileWriteResult, OutputChunk, SshDriver};
use async_trait::async_trait;
use safessh_core::error::{Error, Result};
use safessh_storage::project::Target;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

#[derive(Default)]
pub struct MockDriver {
    responses: Mutex<HashMap<(String, String), CannedResponse>>,
    files: Mutex<BTreeMap<(String, String), Vec<u8>>>,
}

/// Type alias so tests written against the plan's `MockSshDriver` name compile.
pub type MockSshDriver = MockDriver;

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

    /// Pre-populate the in-memory file map for a target. Used in tests to seed
    /// files that `read_file` can return.
    pub fn put_file(&self, target_name: &str, path: &str, bytes: impl Into<Vec<u8>>) {
        self.files
            .lock()
            .expect("MockDriver files mutex poisoned")
            .insert((target_name.to_string(), path.to_string()), bytes.into());
    }

    /// Retrieve bytes that were written to the mock via `write_file`.
    ///
    /// Returns `None` if nothing has been written to that `(target, path)` pair.
    /// Used in tests to assert what the mock received after a `write` call.
    pub fn get_file(&self, target_name: &str, path: &str) -> Option<Vec<u8>> {
        self.files
            .lock()
            .expect("MockDriver files mutex poisoned")
            .get(&(target_name.to_string(), path.to_string()))
            .cloned()
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

    async fn read_file(
        &self,
        target: &Target,
        path: &str,
        cap_bytes: u64,
    ) -> Result<FileReadResult> {
        let key = (target.name().to_string(), path.to_string());
        let map = self.files.lock().expect("MockDriver files mutex poisoned");
        let bytes = map.get(&key).ok_or_else(|| {
            Error::Storage(format!("NotFound: no such remote file: {path}"))
        })?;
        let cap = cap_bytes as usize;
        if bytes.len() > cap {
            Ok(FileReadResult {
                bytes: bytes[..cap].to_vec(),
                canonical_path: path.to_string(),
                truncated: true,
            })
        } else {
            Ok(FileReadResult {
                bytes: bytes.clone(),
                canonical_path: path.to_string(),
                truncated: false,
            })
        }
    }

    async fn write_file(
        &self,
        target: &Target,
        path: &str,
        bytes: &[u8],
    ) -> Result<FileWriteResult> {
        let key = (target.name().to_string(), path.to_string());
        self.files
            .lock()
            .expect("MockDriver files mutex poisoned")
            .insert(key, bytes.to_vec());
        Ok(FileWriteResult {
            canonical_path: path.to_string(),
            bytes_written: bytes.len() as u64,
        })
    }
}
