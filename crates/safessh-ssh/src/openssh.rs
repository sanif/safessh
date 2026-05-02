//! OpenSSH subprocess driver.
//!
//! Spawns the system `ssh` binary, layering ControlMaster multiplexing
//! options on top so a burst of agent calls amortizes a single TCP +
//! auth handshake. Stdout/stderr are streamed through the caller's
//! `on_chunk` callback; once either stream exceeds its cap the child
//! is killed and `truncated` is flagged on the returned `ExecResult`.

use crate::control_master;
use crate::driver::{ExecResult, FileReadResult, FileWriteResult, OutputChunk, SshDriver};
use async_trait::async_trait;
use safessh_core::error::{Error, Result};
use safessh_storage::project::Target;
use std::path::PathBuf;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Driver that exec's the system `ssh` binary with ControlMaster opts.
pub struct OpenSshDriver {
    control_dir: PathBuf,
}

impl OpenSshDriver {
    /// Create the driver, materializing the ControlMaster socket
    /// directory at `control_dir` (mode 0o700 on Unix).
    pub fn new(control_dir: PathBuf) -> Result<Self> {
        control_master::ensure_dir(&control_dir)?;
        Ok(Self { control_dir })
    }

    /// Build the argv vector that would be passed to `ssh`.
    ///
    /// Public so unit tests can verify it without spawning a subprocess.
    /// The remote command is appended after `--` to prevent any leading
    /// dashes in the command from being interpreted as `ssh` flags.
    pub fn build_argv(&self, target: &Target, command: &str) -> Vec<String> {
        let mut argv = vec!["ssh".to_string()];
        argv.extend(control_master::argv_options(&self.control_dir));
        match target {
            Target::SshConfigAlias {
                ssh_config_alias, ..
            } => {
                argv.push(ssh_config_alias.clone());
            }
            Target::Inline {
                host,
                port,
                user,
                identity_file,
                proxy_jump,
                ..
            } => {
                argv.push("-p".into());
                argv.push(port.to_string());
                if let Some(idf) = identity_file {
                    argv.push("-i".into());
                    argv.push(idf.display().to_string());
                }
                if let Some(pj) = proxy_jump {
                    argv.push("-J".into());
                    argv.push(pj.clone());
                }
                argv.push(format!("{user}@{host}"));
            }
        }
        argv.push("--".into());
        argv.push(command.to_string());
        argv
    }
}

#[async_trait]
impl SshDriver for OpenSshDriver {
    async fn exec<'a>(
        &'a self,
        target: &'a Target,
        command: &'a str,
        stdout_cap: u64,
        stderr_cap: u64,
        mut on_chunk: Box<dyn FnMut(OutputChunk) + Send + 'a>,
    ) -> Result<ExecResult> {
        let argv = self.build_argv(target, command);
        let started = Instant::now();
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|e| Error::Ssh(format!("spawn: {e}")))?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Ssh("stdout pipe missing".into()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::Ssh("stderr pipe missing".into()))?;

        let mut stdout_total: u64 = 0;
        let mut stderr_total: u64 = 0;
        let mut truncated = false;
        let mut stdout_done = false;
        let mut stderr_done = false;
        let mut stdout_buf = vec![0u8; 8192];
        let mut stderr_buf = vec![0u8; 8192];

        // Drain both streams concurrently. We bias `select!` so a
        // ready stream never starves; once both reach EOF we stop.
        while !(stdout_done && stderr_done) {
            tokio::select! {
                res = stdout.read(&mut stdout_buf), if !stdout_done => match res {
                    Ok(0) => { stdout_done = true; }
                    Ok(n) => {
                        let allowed = stdout_cap.saturating_sub(stdout_total);
                        let take = (n as u64).min(allowed) as usize;
                        if take > 0 {
                            on_chunk(OutputChunk::Stdout(stdout_buf[..take].to_vec()));
                        }
                        stdout_total += take as u64;
                        if (n as u64) > allowed {
                            truncated = true;
                            let _ = child.kill().await;
                            break;
                        }
                    }
                    Err(_) => { stdout_done = true; }
                },
                res = stderr.read(&mut stderr_buf), if !stderr_done => match res {
                    Ok(0) => { stderr_done = true; }
                    Ok(n) => {
                        let allowed = stderr_cap.saturating_sub(stderr_total);
                        let take = (n as u64).min(allowed) as usize;
                        if take > 0 {
                            on_chunk(OutputChunk::Stderr(stderr_buf[..take].to_vec()));
                        }
                        stderr_total += take as u64;
                        if (n as u64) > allowed {
                            truncated = true;
                            let _ = child.kill().await;
                            break;
                        }
                    }
                    Err(_) => { stderr_done = true; }
                },
            }
        }

        let status = child
            .wait()
            .await
            .map_err(|e| Error::Ssh(format!("wait: {e}")))?;
        Ok(ExecResult {
            exit_code: status.code().unwrap_or(-1),
            stdout_bytes: stdout_total,
            stderr_bytes: stderr_total,
            duration_ms: started.elapsed().as_millis() as u64,
            truncated,
        })
    }

    async fn read_file(
        &self,
        _target: &Target,
        _path: &str,
        _cap_bytes: u64,
    ) -> Result<FileReadResult> {
        Err(Error::Storage(
            "read_file: unimplemented in OpenSshDriver until Task 5".into(),
        ))
    }

    async fn write_file(
        &self,
        _target: &Target,
        _path: &str,
        _bytes: &[u8],
    ) -> Result<FileWriteResult> {
        Err(Error::Storage(
            "write_file: unimplemented in OpenSshDriver until Task 6".into(),
        ))
    }
}
