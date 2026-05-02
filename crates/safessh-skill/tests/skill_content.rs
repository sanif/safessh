const SKILL: &str = include_str!("../src/content/safessh.md");

#[test]
fn skill_describes_forward_subcommand() {
    assert!(
        SKILL.contains("safessh <project> [--on <target>] forward"),
        "skill is missing the `forward` syntax line"
    );
}

#[test]
fn skill_includes_opacity_warning() {
    assert!(
        SKILL.contains("tunnel traffic is opaque to safessh"),
        "skill does not warn the LLM about tunnel opacity"
    );
}

#[test]
fn skill_documents_tunnels_management() {
    assert!(SKILL.contains("safessh tunnels list"));
    assert!(SKILL.contains("safessh tunnels close"));
}
