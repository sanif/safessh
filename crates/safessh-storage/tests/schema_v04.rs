use chrono::Utc;
use safessh_core::types::ParsedCommand;
use safessh_storage::approvals::{PatternRule, PendingRequest};

#[test]
fn pending_request_v03_has_none_tunnel() {
    let toml_src = r#"
token = "abc123"
project = "prod"
categories = ["network:tunnel"]
raw = "forward 5432:db:5432"
created_at = "2026-05-02T12:00:00Z"

[parsed]
binary = "network:tunnel"
flags = []
args = ["5432:db:5432"]
redirects = []
pipes = []
env_mutations = []
raw = "network:tunnel 5432:db:5432"
"#;
    let p: PendingRequest = toml::from_str(toml_src).unwrap();
    assert!(p.tunnel.is_none());
    assert!(p.path.is_none());
}

#[test]
fn pending_request_v04_round_trip_with_tunnel() {
    let p = PendingRequest {
        token: "abc123".into(),
        project: "prod".into(),
        categories: vec!["network:tunnel".into()],
        parsed: ParsedCommand {
            binary: "network:tunnel".into(),
            flags: vec![],
            args: vec!["5432:db:5432".into()],
            redirects: vec![],
            pipes: vec![],
            env_mutations: vec![],
            raw: "network:tunnel 5432:db:5432".into(),
        },
        raw: "network:tunnel 5432:db:5432".into(),
        created_at: Utc::now(),
        path: None,
        tunnel: Some("5432:db:5432".into()),
    };
    let s = toml::to_string(&p).unwrap();
    let back: PendingRequest = toml::from_str(&s).unwrap();
    assert_eq!(back.tunnel.as_deref(), Some("5432:db:5432"));
}

#[test]
fn pattern_rule_v03_has_none_category() {
    let toml_src = r#"
rule_id = "r1"
binary = "ls"
flags = ["-l"]
categories = ["read:safe"]
created_at = "2026-05-02T12:00:00Z"
"#;
    let r: PatternRule = toml::from_str(toml_src).unwrap();
    assert!(r.category.is_none());
}

#[test]
fn pattern_rule_v04_with_category() {
    let r = PatternRule {
        rule_id: "r1".into(),
        binary: "@network:tunnel".into(),
        flags: vec![],
        args_pattern: None,
        categories: vec!["network:tunnel".into()],
        category: Some("network:tunnel".into()),
        created_at: Utc::now(),
    };
    let s = toml::to_string(&r).unwrap();
    let back: PatternRule = toml::from_str(&s).unwrap();
    assert_eq!(back.category.as_deref(), Some("network:tunnel"));
}
