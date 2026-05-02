use safessh_storage::policies::preset_file_rules;
use safessh_storage::project::FileDecision;

#[test]
fn preset_contains_required_read_paths() {
    let rules = preset_file_rules();
    let read_paths: Vec<&String> = rules
        .iter()
        .filter(|r| r.category == "file:read")
        .flat_map(|r| r.paths.iter())
        .collect();
    for required in [
        "/etc/shadow",
        "/etc/sudoers",
        "/etc/sudoers.d/**",
        "/root/.ssh/**",
        "/home/*/.ssh/**",
        "**/.env*",
        "**/id_rsa*",
        "**/id_ed25519*",
        "**/id_ecdsa*",
    ] {
        assert!(
            read_paths.iter().any(|p| p.as_str() == required),
            "preset missing file:read path: {required}"
        );
    }
}

#[test]
fn preset_contains_required_write_paths() {
    let rules = preset_file_rules();
    let write_paths: Vec<&String> = rules
        .iter()
        .filter(|r| r.category == "file:write")
        .flat_map(|r| r.paths.iter())
        .collect();
    for required in [
        "/etc/shadow",
        "/etc/sudoers",
        "/etc/sudoers.d/**",
        "/root/.ssh/**",
        "/home/*/.ssh/**",
    ] {
        assert!(
            write_paths.iter().any(|p| p.as_str() == required),
            "preset missing file:write path: {required}"
        );
    }
}

#[test]
fn every_preset_rule_is_deny() {
    let rules = preset_file_rules();
    for rule in rules {
        assert_eq!(
            rule.decision,
            FileDecision::Deny,
            "non-deny preset entry: {rule:?}"
        );
    }
    assert!(!rules.is_empty(), "preset list unexpectedly empty");
}
