use chrono::{Duration, Utc};
use safessh_core::tunnel::{TunnelCloseReason, TunnelId, TunnelRecord, TunnelSpec};
use safessh_ssh::mock::MockSshDriver;
use safessh_storage::paths::Paths;
use safessh_storage::tunnels::TunnelStore;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

fn paths_in(td: &tempfile::TempDir) -> Paths {
    let root = td.path();
    let p = Paths {
        config: root.join("config"),
        state: root.join("state"),
        cache: root.join("cache"),
    };
    p.ensure_dirs().unwrap();
    p
}

fn record(id: TunnelId, ttl_ms: i64, paths: &Paths) -> TunnelRecord {
    let now = Utc::now();
    let r = TunnelRecord {
        id,
        project: "prod".into(),
        target: "default".into(),
        spec: TunnelSpec::parse("5432:db:5432").unwrap(),
        ssh_pid: 4242,
        supervisor_pid: std::process::id() as i32,
        opened_at: now,
        expires_at: now + Duration::milliseconds(ttl_ms),
    };
    TunnelStore::new(paths).add(&r).unwrap();
    r
}

#[tokio::test]
async fn ttl_expired_closes_tunnel_and_writes_audit() {
    let td = tempdir().unwrap();
    let paths = paths_in(&td);
    let drv = MockSshDriver::default();
    let target = safessh_storage::project::Target::Inline {
        name: "default".into(),
        host: "h".into(),
        port: 22,
        user: "u".into(),
        identity_file: None,
        proxy_jump: None,
        keychain_secret: None,
    };
    let spec = TunnelSpec::parse("5432:db:5432").unwrap();
    let handle = safessh_ssh::driver::SshDriver::open_tunnel(&drv, &target, &spec)
        .await
        .unwrap();
    let rec = record(TunnelId::generate(), 50, &paths);
    let cancel = CancellationToken::new();

    let reason = safessh_cli::supervisor::run_inline(
        paths.clone(),
        rec.clone(),
        handle,
        std::time::Duration::from_millis(50),
        cancel,
    )
    .await
    .unwrap();
    assert_eq!(reason, TunnelCloseReason::TtlExpired);

    // State file removed
    assert!(TunnelStore::new(&paths).get(&rec.id).unwrap().is_none());
    // Audit log contains tunnel_close
    let log = std::fs::read_to_string(paths.audit_log()).unwrap();
    assert!(log.contains("\"event_type\":\"tunnel_close\""));
    assert!(log.contains("\"reason\":\"ttl-expired\""));
}

#[tokio::test]
async fn user_close_via_cancel_token() {
    let td = tempdir().unwrap();
    let paths = paths_in(&td);
    let drv = MockSshDriver::default();
    let target = safessh_storage::project::Target::Inline {
        name: "default".into(),
        host: "h".into(),
        port: 22,
        user: "u".into(),
        identity_file: None,
        proxy_jump: None,
        keychain_secret: None,
    };
    let spec = TunnelSpec::parse("5432:db:5432").unwrap();
    let handle = safessh_ssh::driver::SshDriver::open_tunnel(&drv, &target, &spec)
        .await
        .unwrap();
    let rec = record(TunnelId::generate(), 60_000, &paths);
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    let task = tokio::spawn(safessh_cli::supervisor::run_inline(
        paths.clone(),
        rec.clone(),
        handle,
        std::time::Duration::from_secs(60),
        cancel_clone,
    ));
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    cancel.cancel();
    let reason = task.await.unwrap().unwrap();
    assert_eq!(reason, TunnelCloseReason::UserClose);
}
