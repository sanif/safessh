use chrono::{Duration, Utc};
use safessh_core::tunnel::{TunnelId, TunnelRecord, TunnelSpec};
use safessh_storage::paths::Paths;
use safessh_storage::tunnels::TunnelStore;
use tempfile::tempdir;

fn paths_in(td: &tempfile::TempDir) -> Paths {
    // Construct Paths directly — avoids the process-global SAFESSH_HOME env
    // var race when cargo runs tests in parallel within the same binary.
    let root = td.path();
    let p = Paths {
        config: root.join("config"),
        state: root.join("state"),
        cache: root.join("cache"),
    };
    p.ensure_dirs().unwrap();
    p
}

fn rec(id: TunnelId, pid: i32) -> TunnelRecord {
    TunnelRecord {
        id,
        project: "prod".into(),
        target: "default".into(),
        spec: TunnelSpec::parse("5432:db:5432").unwrap(),
        ssh_pid: pid,
        supervisor_pid: pid,
        opened_at: Utc::now(),
        expires_at: Utc::now() + Duration::minutes(30),
    }
}

#[test]
fn add_and_list_round_trip() {
    let td = tempdir().unwrap();
    let p = paths_in(&td);
    let store = TunnelStore::new(&p);
    let r = rec(TunnelId::generate(), 1);
    store.add(&r).unwrap();
    let listed = store.list_all().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id.as_str(), r.id.as_str());
}

#[test]
fn list_sorts_by_opened_at() {
    let td = tempdir().unwrap();
    let p = paths_in(&td);
    let store = TunnelStore::new(&p);
    let mut a = rec(TunnelId::generate(), 1);
    let mut b = rec(TunnelId::generate(), 2);
    a.opened_at = Utc::now();
    b.opened_at = a.opened_at + Duration::seconds(10);
    store.add(&b).unwrap();
    store.add(&a).unwrap();
    let listed = store.list_all().unwrap();
    assert_eq!(listed[0].id.as_str(), a.id.as_str());
    assert_eq!(listed[1].id.as_str(), b.id.as_str());
}

#[test]
fn get_returns_none_when_missing() {
    let td = tempdir().unwrap();
    let p = paths_in(&td);
    let store = TunnelStore::new(&p);
    assert!(store.get(&TunnelId::generate()).unwrap().is_none());
}

#[test]
fn remove_deletes_and_returns_record() {
    let td = tempdir().unwrap();
    let p = paths_in(&td);
    let store = TunnelStore::new(&p);
    let r = rec(TunnelId::generate(), 1);
    store.add(&r).unwrap();
    let removed = store.remove(&r.id).unwrap();
    assert_eq!(removed.id.as_str(), r.id.as_str());
    assert!(store.list_all().unwrap().is_empty());
    assert!(store.remove(&r.id).is_err());
}

#[test]
fn reap_dead_removes_records_with_dead_pids() {
    let td = tempdir().unwrap();
    let p = paths_in(&td);
    let store = TunnelStore::new(&p);
    // PID 2_000_000_000 is essentially guaranteed not to exist on Unix.
    let dead = rec(TunnelId::generate(), 2_000_000_000);
    let live = rec(TunnelId::generate(), std::process::id() as i32);
    store.add(&dead).unwrap();
    store.add(&live).unwrap();
    let reaped = store.reap_dead().unwrap();
    assert_eq!(reaped.len(), 1);
    assert_eq!(reaped[0].as_str(), dead.id.as_str());
    let remaining = store.list_all().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id.as_str(), live.id.as_str());
}
