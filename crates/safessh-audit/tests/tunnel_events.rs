use chrono::{TimeZone, Utc};
use safessh_audit::event;
use safessh_core::tunnel::{TunnelCloseReason, TunnelId, TunnelSpec};

#[test]
fn tunnel_open_payload() {
    let id = TunnelId::from_str("tunabcde");
    let spec = TunnelSpec::parse("5432:db.internal:5432").unwrap();
    let expires = Utc.with_ymd_and_hms(2026, 5, 2, 12, 30, 0).unwrap();
    let evt = event::tunnel_open("prod", "default", &id, &spec, expires);
    assert_eq!(evt.event_type, "tunnel_open");
    assert_eq!(evt.project.as_deref(), Some("prod"));
    let data = &evt.data;
    assert_eq!(data["id"], "tunabcde");
    assert_eq!(data["target"], "default");
    assert_eq!(data["local_port"], 5432);
    assert_eq!(data["remote_host"], "db.internal");
    assert_eq!(data["remote_port"], 5432);
    assert_eq!(
        data["opacity_warning"],
        "tunnel traffic is opaque to safessh"
    );
    assert_eq!(data["expires_at"], "2026-05-02T12:30:00Z");
}

#[test]
fn tunnel_close_reason_kebab_case() {
    let id = TunnelId::from_str("tunabcde");
    let evt = event::tunnel_close("prod", &id, TunnelCloseReason::TtlExpired, 1800);
    assert_eq!(evt.event_type, "tunnel_close");
    assert_eq!(evt.data["id"], "tunabcde");
    assert_eq!(evt.data["reason"], "ttl-expired");
    assert_eq!(evt.data["duration_secs"], 1800);
}

#[test]
fn tunnel_close_user_close() {
    let id = TunnelId::from_str("tunabcde");
    let evt = event::tunnel_close("prod", &id, TunnelCloseReason::UserClose, 12);
    assert_eq!(evt.data["reason"], "user-close");
    assert_eq!(evt.data["duration_secs"], 12);
}
