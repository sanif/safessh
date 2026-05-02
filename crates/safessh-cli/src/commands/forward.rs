//! `safessh <project> forward <spec>` — policy-gated tunnel open + re-exec supervisor.
//!
//! Flow (simplified — parent does NOT open ssh):
//! 1. Parse argv, validate spec.
//! 2. Policy decide → Allow / RequireApproval / Block / Deny.
//! 3. Generate TunnelId, opened_at, expires_at.
//! 4. Write TunnelRecord with placeholder pids (-1).
//! 5. Write tunnel_open audit event.
//! 6. Spawn detached `safessh __tunnel-supervisor <state-path>` child.
//! 7. Print success line and return Ok(()).
//!
//! The child (`run_supervisor`) reads the record, opens its own ssh handle,
//! updates supervisor_pid + ssh_pid, then calls `supervisor::run_inline`.

use crate::supervisor;
use chrono::Duration;
use safessh_audit::event;
use safessh_audit::jsonl::AuditWriter;
use safessh_core::error::{Error, Result};
use safessh_core::tunnel::{TunnelId, TunnelRecord, TunnelSpec};
use safessh_core::types::{ParsedCommand, PolicyDecision};
use safessh_policy::decision::{decide, DecisionInput, FileOp, TunnelOp};
use safessh_ssh::driver::SshDriver;
use safessh_ssh::openssh::OpenSshDriver;
use safessh_storage::approvals::{
    AlwaysStore, BlockedStore, PendingRequest, PendingStore, TimedStore,
};
use safessh_storage::paths::Paths;
use safessh_storage::policies::preset_file_rules;
use safessh_storage::project::ProjectStore;
use safessh_storage::tunnels::TunnelStore;
use std::path::Path;
use std::sync::Arc;

pub async fn run(args: Vec<String>, yolo: bool) -> Result<()> {
    let paths = Paths::user().map_err(Error::Io)?;
    paths.ensure_dirs().map_err(Error::Io)?;

    // argv shape: [<project>, "forward", <spec>, … flags].
    let (clean_args, yolo_in_args, on_target) = strip_extras(args);
    let yolo = yolo || yolo_in_args;

    if clean_args.len() < 3 || clean_args[1] != "forward" {
        return Err(Error::Usage(
            "expected: safessh <project> forward <local>:<remote_host>:<remote_port>".into(),
        ));
    }
    let project_name = clean_args[0].clone();
    let raw_spec = clean_args[2].clone();

    // Validate the spec before doing any I/O — exits 2 on bad shape.
    // SAFETY-INVARIANT-1: parse failure → Usage error (exit 2), never silently Allow.
    let spec = TunnelSpec::parse(&raw_spec)
        .map_err(|e| Error::Usage(format!("invalid forward spec: {e}")))?;

    let project = ProjectStore::new(paths.clone()).load(&project_name)?;
    let writer = AuditWriter::open(&paths)?;

    if yolo {
        let cfg = safessh_storage::config::load(&paths).unwrap_or_default();
        if cfg.disable_yolo {
            return Err(Error::YoloRefused);
        }
        writer.append(&event::yolo_invocation(
            &project_name,
            &format!("forward {raw_spec}"),
        ))?;
    } else {
        let timed = TimedStore::new(&paths);
        let _ = timed.purge_expired(&project_name);
        let timed_rules = timed.list_active(&project_name).unwrap_or_default();
        let allow_rules = AlwaysStore::new(&paths)
            .list(&project_name)
            .unwrap_or_default();
        let block_rules = BlockedStore::new(&paths)
            .list(&project_name)
            .unwrap_or_default();

        // SAFETY-INVARIANT-2: block/deny checked before allow.
        let (decision, _) = decide(DecisionInput {
            raw: "",
            policy: &project.policy,
            allows: &allow_rules,
            timed: &timed_rules,
            blocks: &block_rules,
            file_op: FileOp::None,
            preset_file_rules: preset_file_rules(),
            tunnel_op: TunnelOp::Forward(&raw_spec),
        });

        match &decision {
            PolicyDecision::Allow { .. } => {}
            PolicyDecision::RequireApproval {
                token, categories, ..
            } => {
                // Persist a pending request so TUI / `safessh approve` can act on it.
                if !atty::is(atty::Stream::Stdin) {
                    let req = PendingRequest {
                        token: token.as_str().to_string(),
                        project: project_name.clone(),
                        categories: categories.clone(),
                        parsed: ParsedCommand {
                            binary: "@network:tunnel".into(),
                            flags: vec![],
                            args: vec![raw_spec.clone()],
                            redirects: vec![],
                            pipes: vec![],
                            env_mutations: vec![],
                            raw: format!("network:tunnel {raw_spec}"),
                        },
                        raw: format!("network:tunnel {raw_spec}"),
                        created_at: chrono::Utc::now(),
                        path: None,
                        tunnel: Some(raw_spec.clone()),
                    };
                    PendingStore::new(&paths).add(&req)?;
                }
                return Err(Error::ApprovalRequired {
                    token: token.as_str().to_string(),
                    categories: categories.clone(),
                });
            }
            PolicyDecision::Block { rule_id, pattern } => {
                return Err(Error::Blocked {
                    rule_id: rule_id.clone(),
                    reason: pattern.clone(),
                });
            }
            PolicyDecision::Deny { reason } => {
                return Err(Error::Denied(reason.clone()));
            }
        }
    }

    // --- Allow path ---
    // Resolve target.
    let want = on_target.as_deref().unwrap_or(&project.default_target);
    let target = project
        .targets
        .iter()
        .find(|t| t.name() == want)
        .ok_or_else(|| Error::Usage(format!("no such target: {want}")))?;
    let _ = target; // suppress unused warning — used below via project re-load in supervisor

    let id = TunnelId::generate();
    let opened_at = chrono::Utc::now();
    let ttl_minutes = project.output.tunnel_ttl_minutes as i64;
    let expires_at = opened_at + Duration::minutes(ttl_minutes);

    // Write the record with placeholder pids (-1).
    // The child supervisor will overwrite both pids after it opens its ssh handle.
    let record = TunnelRecord {
        id: id.clone(),
        project: project_name.clone(),
        target: target.name().to_string(),
        spec: spec.clone(),
        ssh_pid: -1,
        supervisor_pid: -1,
        opened_at,
        expires_at,
    };
    let state_path = paths.tunnels_dir().join(format!("{}.toml", id.as_str()));
    TunnelStore::new(&paths).add(&record)?;

    // SAFETY-INVARIANT-4: audit write before any user-visible output.
    writer.append(&event::tunnel_open(
        &project_name,
        target.name(),
        &id,
        &spec,
        expires_at,
    ))?;

    // Spawn the detached supervisor child; it handles all ssh I/O.
    spawn_supervisor_process(&paths, &state_path)?;

    println!(
        "tunnel open id={id} on localhost:{lp} \u{2192} {rh}:{rp} (max {ttl} min)\n\
         tunnel traffic is opaque to safessh \u{2014} only `tunnels close {id}` audits closure",
        id = id.as_str(),
        lp = spec.local_port,
        rh = spec.remote_host,
        rp = spec.remote_port,
        ttl = ttl_minutes,
    );

    Ok(())
}

/// Spawn `safessh __tunnel-supervisor <record_path>` as a fully detached child.
///
/// stdin/stdout/stderr → /dev/null so the child cannot pollute the parent's
/// framed output. On Unix the child is placed in a new session (setsid) so
/// SIGINT to the calling shell never reaches it.
pub fn spawn_supervisor_process(paths: &Paths, record_path: &Path) -> Result<()> {
    let _ = paths; // available for future use (e.g. logging)
    use std::process::{Command as Stdc, Stdio};

    let exe = std::env::current_exe().map_err(Error::Io)?;
    let mut cmd = Stdc::new(exe);
    cmd.arg("__tunnel-supervisor")
        .arg(record_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is async-signal-safe; this pre_exec closure runs
        // in the child after fork() but before exec(), with no allocator use.
        unsafe {
            cmd.pre_exec(|| {
                // Detach from controlling terminal and the parent's process
                // group so SIGINT to the parent shell never reaches us.
                // SAFETY: see above.
                nix::unistd::setsid().ok();
                Ok(())
            });
        }
    }

    cmd.spawn()
        .map_err(|e| Error::Ssh(format!("spawn supervisor: {e}")))?;
    Ok(())
}

/// Child entrypoint: re-reads the TunnelRecord TOML, opens its own ssh
/// handle, updates pids in the store, installs SIGTERM handler, then calls
/// `supervisor::run_inline` to block until TTL / signal / ssh-exit.
pub async fn run_supervisor(record_path: std::path::PathBuf) -> Result<()> {
    let raw = std::fs::read_to_string(&record_path).map_err(Error::Io)?;
    let mut record: TunnelRecord =
        toml::from_str(&raw).map_err(|e| Error::Storage(e.to_string()))?;

    let paths = Paths::user().map_err(Error::Io)?;
    paths.ensure_dirs().map_err(Error::Io)?;

    let driver =
        Arc::new(OpenSshDriver::new(paths.cache.join("control-sockets"))?) as Arc<dyn SshDriver>;

    // Re-resolve target from the current project file.
    let project = ProjectStore::new(paths.clone()).load(&record.project)?;
    let target = project
        .targets
        .iter()
        .find(|t| t.name() == record.target)
        .ok_or_else(|| Error::Usage(format!("no such target: {}", record.target)))?;

    let handle = driver.open_tunnel(target, &record.spec).await?;

    // Update the state file with the real pids.
    record.ssh_pid = handle.ssh_pid();
    record.supervisor_pid = std::process::id() as i32;
    TunnelStore::new(&paths).add(&record)?;

    let ttl = std::time::Duration::from_secs(
        (record.expires_at - chrono::Utc::now())
            .num_seconds()
            .max(0) as u64,
    );

    let cancel = tokio_util::sync::CancellationToken::new();
    install_sigterm(cancel.clone());

    supervisor::run_inline(paths, record, handle, ttl, cancel).await?;
    Ok(())
}

/// Install a SIGTERM handler that cancels the given token (Unix only).
///
/// Spawned as a detached tokio task; does nothing on non-Unix platforms.
#[cfg(unix)]
fn install_sigterm(cancel: tokio_util::sync::CancellationToken) {
    tokio::spawn(async move {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            if s.recv().await.is_some() {
                cancel.cancel();
            }
        }
    });
}

#[cfg(not(unix))]
fn install_sigterm(_cancel: tokio_util::sync::CancellationToken) {}

/// Strip `--yolo` and `--on <target>` / `--on=<target>` from the
/// external-subcommand argv, which passes verbatim (clap doesn't parse it).
///
/// Returns `(remaining_args, yolo_seen, on_target_value)`.
fn strip_extras(args: Vec<String>) -> (Vec<String>, bool, Option<String>) {
    let mut out = Vec::with_capacity(args.len());
    let mut yolo = false;
    let mut on_target = None;
    let mut iter = args.into_iter();
    while let Some(a) = iter.next() {
        if a == "--yolo" {
            yolo = true;
            continue;
        }
        if a == "--on" {
            on_target = iter.next();
            continue;
        }
        if let Some(rest) = a.strip_prefix("--on=") {
            on_target = Some(rest.to_string());
            continue;
        }
        out.push(a);
    }
    (out, yolo, on_target)
}
