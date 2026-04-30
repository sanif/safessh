use safessh_skill::adapters::Target;
use safessh_skill::install;

#[test]
fn install_claude_code_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join(".claude/skills/safessh.md");
    install::install_to(Target::ClaudeCode, &dest).unwrap();
    let content = std::fs::read_to_string(&dest).unwrap();
    assert!(content.starts_with("---\nname: safessh"));
}

#[test]
fn install_agents_md_appends_section() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("AGENTS.md");
    std::fs::write(&path, "# Existing\n\nstuff\n").unwrap();
    install::install_to(Target::AgentsMd, &path).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("# Existing"));
    assert!(content.contains("## safessh"));
}

#[test]
fn uninstall_agents_md_removes_section() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("AGENTS.md");
    install::install_to(Target::AgentsMd, &path).unwrap();
    install::uninstall_at(Target::AgentsMd, &path).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(!content.contains("## safessh"));
}
