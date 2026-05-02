use safessh_storage::project::{FileDecision, FileRule, Policy};

#[test]
fn v02_policy_parses_with_empty_file_rules() {
    let toml_src = r#"
allow = ["read:safe", "file:read"]
require_approval = ["file:write"]
deny = []
"#;
    let p: Policy = toml::from_str(toml_src).unwrap();
    assert_eq!(p.file_rules.len(), 0);
    assert_eq!(p.allow, vec!["read:safe".to_string(), "file:read".into()]);
}

#[test]
fn parses_file_rules_table() {
    let toml_src = r#"
allow = []
require_approval = []
deny = []

[[file_rules]]
category = "file:read"
paths = ["/etc/nginx/*", "/var/log/nginx/**"]
decision = "allow"

[[file_rules]]
category = "file:write"
paths = ["/tmp/staging/*"]
decision = "approve"

[[file_rules]]
category = "file:read"
paths = ["/etc/shadow"]
decision = "deny"

[[file_rules]]
category = "file:write"
paths = ["/etc/sudoers.d/**"]
decision = "block"
"#;
    let p: Policy = toml::from_str(toml_src).unwrap();
    assert_eq!(p.file_rules.len(), 4);
    assert!(matches!(p.file_rules[0].decision, FileDecision::Allow));
    assert!(matches!(p.file_rules[1].decision, FileDecision::Approve));
    assert!(matches!(p.file_rules[2].decision, FileDecision::Deny));
    assert!(matches!(p.file_rules[3].decision, FileDecision::Block));
    assert_eq!(p.file_rules[0].paths, vec!["/etc/nginx/*".to_string(), "/var/log/nginx/**".into()]);
}

#[test]
fn rejects_unknown_decision_value() {
    let toml_src = r#"
allow = []
require_approval = []
deny = []
[[file_rules]]
category = "file:read"
paths = ["/x"]
decision = "maybe"
"#;
    let err = toml::from_str::<Policy>(toml_src).unwrap_err().to_string();
    assert!(err.contains("decision"), "error did not mention `decision`: {err}");
}

#[test]
fn round_trips() {
    let p = Policy {
        allow: vec!["read:safe".into()],
        require_approval: vec!["file:write".into()],
        deny: vec![],
        file_rules: vec![FileRule {
            category: "file:read".into(),
            paths: vec!["/etc/nginx/*".into()],
            decision: FileDecision::Allow,
        }],
    };
    let s = toml::to_string(&p).unwrap();
    let back: Policy = toml::from_str(&s).unwrap();
    assert_eq!(back.file_rules.len(), 1);
    assert_eq!(back.file_rules[0].paths, p.file_rules[0].paths);
}
