use safessh_core::types::{
    AllowSource, ApprovalToken, AuditEvent, ParsedCommand, PolicyDecision, ProjectId, Target,
};

#[test]
fn project_id_rejects_invalid() {
    assert!(ProjectId::new("ok-name").is_ok());
    assert!(ProjectId::new("bad/name").is_err());
    assert!(ProjectId::new("bad name").is_err());
    assert!(ProjectId::new("bad\\name").is_err());
    assert!(ProjectId::new(".hidden").is_err());
    assert!(ProjectId::new("").is_err());
}

#[test]
fn project_id_round_trips_through_serde() {
    let pid = ProjectId::new("my-project").unwrap();
    let json = serde_json::to_string(&pid).unwrap();
    let back: ProjectId = serde_json::from_str(&json).unwrap();
    assert_eq!(pid, back);
}

#[test]
fn approval_token_alphabet_excludes_ambiguous_chars() {
    for _ in 0..1000 {
        let t = ApprovalToken::generate();
        let s = t.as_str();
        assert_eq!(s.len(), 6);
        for c in s.chars() {
            assert!(
                c != '0' && c != '1' && c != 'l' && c != 'o',
                "bad char: {c}"
            );
            assert!(c.is_ascii_alphanumeric());
        }
    }
}

#[test]
fn approval_token_round_trips() {
    let t = ApprovalToken::generate();
    let json = serde_json::to_string(&t).unwrap();
    let back: ApprovalToken = serde_json::from_str(&json).unwrap();
    assert_eq!(t, back);
}

#[test]
fn target_round_trips_ssh_alias() {
    let t = Target::SshConfigAlias {
        name: "prod-db".into(),
        ssh_config_alias: "db.prod".into(),
    };
    let json = serde_json::to_string(&t).unwrap();
    let back: Target = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name(), "prod-db");
}

#[test]
fn target_round_trips_inline_with_default_port() {
    let t = Target::Inline {
        name: "node1".into(),
        host: "10.0.0.1".into(),
        port: 22,
        user: "ubuntu".into(),
        identity_file: None,
        proxy_jump: None,
        keychain_secret: None,
    };
    let json = serde_json::to_string(&t).unwrap();
    let back: Target = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name(), "node1");
}

#[test]
fn target_inline_default_port_when_missing() {
    let json = r#"{
        "name": "node1",
        "host": "10.0.0.1",
        "user": "ubuntu",
        "identity_file": null,
        "proxy_jump": null,
        "keychain_secret": null
    }"#;
    let t: Target = serde_json::from_str(json).unwrap();
    match t {
        Target::Inline { port, .. } => assert_eq!(port, 22),
        _ => panic!("expected Inline variant"),
    }
}

#[test]
fn parsed_command_round_trips() {
    let cmd = ParsedCommand {
        binary: "ls".into(),
        flags: vec!["-la".into()],
        args: vec!["/tmp".into()],
        redirects: vec![],
        pipes: vec![ParsedCommand {
            binary: "grep".into(),
            flags: vec![],
            args: vec!["foo".into()],
            redirects: vec![],
            pipes: vec![],
            env_mutations: vec![],
            raw: "grep foo".into(),
        }],
        env_mutations: vec![("FOO".into(), "bar".into())],
        raw: "ls -la /tmp | grep foo".into(),
    };
    let json = serde_json::to_string(&cmd).unwrap();
    let back: ParsedCommand = serde_json::from_str(&json).unwrap();
    assert_eq!(cmd, back);
}

#[test]
fn policy_decision_round_trips_all_variants() {
    let allow = PolicyDecision::Allow {
        matched_rule: Some("rule-1".into()),
        source: AllowSource::DefaultPolicy,
    };
    let json = serde_json::to_string(&allow).unwrap();
    let _: PolicyDecision = serde_json::from_str(&json).unwrap();

    let req = PolicyDecision::RequireApproval {
        token: ApprovalToken::generate(),
        categories: vec!["fs.write".into()],
        reason: "writes to disk".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    let _: PolicyDecision = serde_json::from_str(&json).unwrap();

    let block = PolicyDecision::Block {
        rule_id: "rule-2".into(),
        pattern: "rm -rf".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let _: PolicyDecision = serde_json::from_str(&json).unwrap();

    let deny = PolicyDecision::Deny {
        reason: "no".into(),
    };
    let json = serde_json::to_string(&deny).unwrap();
    let _: PolicyDecision = serde_json::from_str(&json).unwrap();
}

#[test]
fn allow_source_round_trips() {
    let s = AllowSource::TimedRule {
        rule_id: "x".into(),
        expires_at: chrono::Utc::now(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let _: AllowSource = serde_json::from_str(&json).unwrap();
}

#[test]
fn audit_event_round_trips() {
    let mut ev = AuditEvent::new("exec.start");
    ev.project = Some("my-project".into());
    ev.data = serde_json::json!({"command": "ls"});
    ev.error_class = Some("none".into());
    let json = serde_json::to_string(&ev).unwrap();
    let back: AuditEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back.schema_version, 1);
    assert_eq!(back.event_type, "exec.start");
    assert_eq!(back.project.as_deref(), Some("my-project"));
}

#[test]
fn audit_event_default_schema_version_is_one() {
    let ev = AuditEvent::new("foo");
    assert_eq!(ev.schema_version, 1);
}

#[test]
fn error_exit_codes_match_spec() {
    use safessh_core::error::Error;
    assert_eq!(Error::Config("x".into()).exit_code(), 1);
    assert_eq!(Error::ProjectNotFound("x".into()).exit_code(), 1);
    assert_eq!(Error::Usage("x".into()).exit_code(), 2);
    assert_eq!(
        Error::ApprovalRequired {
            token: "abc".into(),
            categories: vec![],
        }
        .exit_code(),
        10
    );
    assert_eq!(
        Error::Blocked {
            rule_id: "r".into(),
            reason: "x".into(),
        }
        .exit_code(),
        11
    );
    assert_eq!(Error::Denied("x".into()).exit_code(), 12);
    assert_eq!(Error::YoloRefused.exit_code(), 13);
    assert_eq!(Error::Ssh("x".into()).exit_code(), 20);
    assert_eq!(Error::Connection("x".into()).exit_code(), 21);
    assert_eq!(Error::OutputCapped { limit_bytes: 100 }.exit_code(), 30);
    assert_eq!(Error::Storage("x".into()).exit_code(), 40);
    assert_eq!(Error::AuditWriteFailed("x".into()).exit_code(), 50);
}

#[test]
fn error_classes_are_stable_strings() {
    use safessh_core::error::Error;
    assert_eq!(Error::Config("x".into()).error_class(), "config");
    assert_eq!(
        Error::ProjectNotFound("x".into()).error_class(),
        "project_not_found"
    );
    assert_eq!(Error::Usage("x".into()).error_class(), "usage");
    assert_eq!(
        Error::ApprovalRequired {
            token: "abc".into(),
            categories: vec![],
        }
        .error_class(),
        "approval_required"
    );
    assert_eq!(
        Error::Blocked {
            rule_id: "r".into(),
            reason: "x".into(),
        }
        .error_class(),
        "blocked"
    );
    assert_eq!(Error::Denied("x".into()).error_class(), "denied");
    assert_eq!(Error::YoloRefused.error_class(), "yolo_refused");
    assert_eq!(Error::Ssh("x".into()).error_class(), "ssh_failure");
    assert_eq!(
        Error::Connection("x".into()).error_class(),
        "connection_failure"
    );
    assert_eq!(
        Error::OutputCapped { limit_bytes: 100 }.error_class(),
        "output_capped"
    );
    assert_eq!(Error::Storage("x".into()).error_class(), "storage");
    assert_eq!(
        Error::AuditWriteFailed("x".into()).error_class(),
        "audit_write_failed"
    );
}
