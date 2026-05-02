use safessh_tui::screens::audit::summarize_for_test;
use serde_json::json;

#[test]
fn open_summary_contains_spec_and_opaque_tag() {
    let v = json!({
        "event_type": "tunnel_open",
        "data": {
            "id": "abcdefgh",
            "target": "default",
            "local_port": 5432,
            "remote_host": "db.internal",
            "remote_port": 5432,
            "expires_at": "2026-05-02T13:00:00Z",
            "opacity_warning": "tunnel traffic is opaque to safessh",
        }
    });
    let s = summarize_for_test("tunnel_open", &v);
    assert!(s.contains("5432"));
    assert!(s.contains("db.internal"));
    assert!(s.contains("[opaque]"), "summary missing [opaque] tag: {s}");
}

#[test]
fn close_summary_carries_reason_and_duration() {
    let v = json!({
        "event_type": "tunnel_close",
        "data": {
            "id": "abcdefgh",
            "reason": "ttl-expired",
            "duration_secs": 1800,
        }
    });
    let s = summarize_for_test("tunnel_close", &v);
    assert!(s.contains("ttl-expired"));
    assert!(s.contains("30 min") || s.contains("1800s"));
}
