use chrono::Utc;
use safessh_core::tunnel::{TunnelCloseReason, TunnelId, TunnelRecord, TunnelSpec};

#[test]
fn tunnel_id_generates_eight_chars() {
    for _ in 0..50 {
        let id = TunnelId::generate();
        let s = id.as_str();
        assert_eq!(s.len(), 8);
        assert!(s.chars().all(|c| !"01lo".contains(c)));
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}

#[test]
fn parses_full_spec() {
    let s = TunnelSpec::parse("5432:db.internal:5432").unwrap();
    assert_eq!(s.local_port, 5432);
    assert_eq!(s.remote_host, "db.internal");
    assert_eq!(s.remote_port, 5432);
}

#[test]
fn rejects_two_part_spec() {
    let err = TunnelSpec::parse("5432:db.internal").unwrap_err();
    assert!(err.to_string().contains("local_port:remote_host:remote_port"));
}

#[test]
fn rejects_zero_port() {
    assert!(TunnelSpec::parse("0:db:5432").is_err());
    assert!(TunnelSpec::parse("5432:db:0").is_err());
}

#[test]
fn rejects_overflow_port() {
    assert!(TunnelSpec::parse("80000:db:80").is_err());
    assert!(TunnelSpec::parse("80:db:99999").is_err());
}

#[test]
fn close_reason_serde_kebab_case() {
    let r: TunnelCloseReason = serde_json::from_str("\"ttl-expired\"").unwrap();
    assert!(matches!(r, TunnelCloseReason::TtlExpired));
    let s = serde_json::to_string(&TunnelCloseReason::UserClose).unwrap();
    assert_eq!(s, "\"user-close\"");
}

#[test]
fn record_roundtrips_via_toml() {
    let id = TunnelId::generate();
    let rec = TunnelRecord {
        id: id.clone(),
        project: "prod".into(),
        target: "default".into(),
        spec: TunnelSpec::parse("5432:db.internal:5432").unwrap(),
        ssh_pid: 4242,
        supervisor_pid: 4243,
        opened_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::minutes(30),
    };
    let s = toml::to_string(&rec).unwrap();
    let back: TunnelRecord = toml::from_str(&s).unwrap();
    assert_eq!(back.id.as_str(), id.as_str());
    assert_eq!(back.target, "default");
    assert_eq!(back.spec.local_port, 5432);
}
