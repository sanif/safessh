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
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// Driver that exec's the system `ssh` binary with ControlMaster opts.
pub struct OpenSshDriver {
    control_dir: PathBuf,
}

/// Returns the `user@host` or alias string that identifies the target to
/// OpenSSH and sftp.  Extracted so both `exec` and `sftp_batch` use the
/// same logic without duplication.
fn openssh_host_arg(target: &Target) -> String {
    match target {
        Target::SshConfigAlias {
            ssh_config_alias, ..
        } => ssh_config_alias.clone(),
        Target::Inline { host, user, .. } => format!("{user}@{host}"),
    }
}

/// Returns the ControlPath socket pattern for this target, using the same
/// `%C` expansion token as `control_master::argv_options`.
fn control_path_for(control_dir: &Path) -> PathBuf {
    control_dir.join("%C")
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

    /// Run an sftp batch script over the existing ControlMaster socket and
    /// return `(stdout_bytes, stderr_bytes, exit_code)`.
    ///
    /// The script is fed to sftp via `-b -` (stdin). `ControlMaster=no`
    /// ensures sftp reuses the master rather than opening a new handshake.
    /// `stdout_cap` kills the child once that many bytes have been collected,
    /// enabling byte-level truncation for `read_file`.
    async fn sftp_batch(
        &self,
        target: &Target,
        script: &str,
        stdout_cap: Option<u64>,
    ) -> Result<(Vec<u8>, Vec<u8>, i32)> {
        let cp = control_path_for(&self.control_dir);
        let host_arg = openssh_host_arg(target);

        // sftp uses `-P` (upper-case) for the port number, unlike `ssh -p`.
        // When the target specifies a non-default port we must pass it so that
        // the `%C` ControlPath token expands to the same hash that `ssh` used
        // when it created the master socket. Without the matching port the two
        // expansions diverge and sftp falls back to a fresh TCP connection.
        let port_args: Vec<String> = match target {
            Target::Inline { port, .. } if *port != 22 => {
                vec!["-P".to_string(), port.to_string()]
            }
            _ => vec![],
        };

        let mut cmd = Command::new("sftp");
        cmd.args(&port_args)
            .arg("-o")
            .arg(format!("ControlPath={}", cp.display()))
            .arg("-o")
            .arg("ControlMaster=no")
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-b")
            .arg("-")
            .arg(host_arg)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Storage(format!("sftp spawn: {e}")))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Storage("sftp stdin pipe missing".into()))?;
        stdin
            .write_all(script.as_bytes())
            .await
            .map_err(|e| Error::Storage(format!("sftp stdin write: {e}")))?;
        drop(stdin);

        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Storage("sftp stdout pipe missing".into()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::Storage("sftp stderr pipe missing".into()))?;

        if let Some(cap) = stdout_cap {
            let mut taken = 0u64;
            let mut chunk = [0u8; 8192];
            loop {
                let n = stdout.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                let to_take = (cap.saturating_sub(taken) as usize).min(n);
                stdout_buf.extend_from_slice(&chunk[..to_take]);
                taken += to_take as u64;
                if taken >= cap {
                    let _ = child.start_kill();
                    break;
                }
            }
        } else {
            stdout
                .read_to_end(&mut stdout_buf)
                .await
                .map_err(|e| Error::Storage(format!("sftp stdout: {e}")))?;
        }

        stderr
            .read_to_end(&mut stderr_buf)
            .await
            .map_err(|e| Error::Storage(format!("sftp stderr: {e}")))?;

        let status = child
            .wait()
            .await
            .map_err(|e| Error::Storage(format!("sftp wait: {e}")))?;
        Ok((stdout_buf, stderr_buf, status.code().unwrap_or(-1)))
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
        target: &Target,
        path: &str,
        cap_bytes: u64,
    ) -> Result<FileReadResult> {
        // Sub-batch 1: resolve the canonical path via `@realpath`.
        //
        // The `@` prefix makes sftp continue (exit 0) even if realpath fails,
        // but some sftp clients (macOS OpenSSH 10.x) do not support the
        // `realpath` command in batch mode and return "Invalid command." with a
        // non-zero exit. In that case we fall back to using `path` unchanged —
        // symlinks won't be resolved but the audit record's `canonical_path`
        // field will still contain the caller-supplied path, which is correct
        // enough for the audit trail.
        let realpath_script = format!("@realpath \"{}\"\n", path.replace('"', "\\\""));
        let (realpath_out, realpath_err, code) =
            self.sftp_batch(target, &realpath_script, None).await?;
        let canonical = if code != 0 {
            let s = String::from_utf8_lossy(&realpath_err);
            if s.contains("No such file") {
                return Err(Error::Storage(format!("no such remote file: {path}")));
            }
            // Client doesn't support realpath (e.g. macOS sftp): use path as-is.
            path.to_string()
        } else {
            String::from_utf8_lossy(&realpath_out).trim().to_string()
        };

        // Sub-batch 2: download to a local temp file then read it back.
        //
        // Using `/dev/stdout` as the sftp `get` destination does NOT work when
        // sftp's stdout is connected to a pipe (the common case when run from
        // Rust or any non-terminal parent): sftp calls `ftruncate` and `lseek`
        // on the local file before writing, which fail with ESPIPE/EINVAL on a
        // pipe, causing sftp to emit "Illegal seek" and produce no output. This
        // affects macOS OpenSSH 9.x/10.x; Linux sftp is less strict.
        //
        // Instead we download into a `tempfile::NamedTempFile`, read it back
        // (applying the byte cap manually), and delete it on drop. The local
        // I/O is negligible compared to the sftp round-trip.
        let local_tmp = tempfile::NamedTempFile::new()
            .map_err(|e| Error::Storage(format!("local read tempfile: {e}")))?;
        let local_path = local_tmp.path().to_path_buf();

        let get_script = format!(
            "get \"{}\" \"{}\"\n",
            canonical.replace('"', "\\\""),
            local_path.display().to_string().replace('"', "\\\""),
        );
        let (_out, err, code) = self.sftp_batch(target, &get_script, None).await?;
        if code != 0 {
            let s = String::from_utf8_lossy(&err);
            if s.contains("No such file") || s.contains("not found") {
                return Err(Error::Storage(format!("no such remote file: {path}")));
            }
            return Err(Error::Storage(format!("sftp get failed: {s}")));
        }

        // Apply byte cap: read up to `cap_bytes` from the local file.
        let mut file = std::fs::File::open(&local_path)
            .map_err(|e| Error::Storage(format!("open local download: {e}")))?;
        let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut bytes = Vec::with_capacity(cap_bytes.min(file_len) as usize);
        use std::io::Read;
        file.by_ref()
            .take(cap_bytes)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Storage(format!("read local download: {e}")))?;
        let truncated = file_len > cap_bytes;

        Ok(FileReadResult {
            bytes,
            canonical_path: canonical,
            truncated,
        })
    }

    async fn write_file(
        &self,
        target: &Target,
        path: &str,
        bytes: &[u8],
    ) -> Result<FileWriteResult> {
        use rand::Rng;

        // Stage bytes to a local tempfile so sftp `put` has a source.
        let mut local_tmp = tempfile::NamedTempFile::new()
            .map_err(|e| Error::Storage(format!("local tempfile: {e}")))?;
        std::io::Write::write_all(local_tmp.as_file_mut(), bytes)
            .map_err(|e| Error::Storage(format!("local tempfile write: {e}")))?;
        let local_path = local_tmp.path().to_path_buf();

        // Compute remote temp path: <dir>/.safessh.<8hex>.tmp
        let (remote_dir, _remote_name) = match path.rsplit_once('/') {
            Some((d, n)) if !d.is_empty() => (d.to_string(), n.to_string()),
            Some((_, n)) => ("/".to_string(), n.to_string()),
            None => {
                return Err(Error::Storage(format!(
                    "write_file: path must be absolute: {path}"
                )))
            }
        };
        let token: String = (0..8)
            .map(|_| format!("{:x}", rand::thread_rng().gen_range(0..16u32)))
            .collect();
        let remote_tmp = if remote_dir == "/" {
            format!("/.safessh.{token}.tmp")
        } else {
            format!("{remote_dir}/.safessh.{token}.tmp")
        };

        // SAFETY-INVARIANT-13: atomic remote write via temp+rename.
        let upload_script = format!(
            "put \"{}\" \"{}\"\nrename \"{}\" \"{}\"\n",
            local_path.display().to_string().replace('"', "\\\""),
            remote_tmp.replace('"', "\\\""),
            remote_tmp.replace('"', "\\\""),
            path.replace('"', "\\\""),
        );

        let (_out, err, code) = self.sftp_batch(target, &upload_script, None).await?;
        if code != 0 {
            // Best-effort cleanup of the remote temp.
            let cleanup = format!("rm \"{}\"\n", remote_tmp.replace('"', "\\\""));
            let _ = self.sftp_batch(target, &cleanup, None).await;

            let s = String::from_utf8_lossy(&err);
            if s.contains("No such file or directory") && s.contains(&remote_dir) {
                return Err(Error::Storage(format!(
                    "no such remote directory: {remote_dir}"
                )));
            }
            if s.contains("Failure") || s.contains("permission denied") {
                return Err(Error::Storage(format!("sftp write failed: {s}")));
            }
            return Err(Error::Storage(format!(
                "sftp write failed (code {code}): {s}"
            )));
        }

        Ok(FileWriteResult {
            canonical_path: path.to_string(),
            bytes_written: bytes.len() as u64,
        })
    }
}
