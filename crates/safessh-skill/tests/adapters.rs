use safessh_skill::adapters::{filename, format, Target};
use safessh_skill::CONTENT;

#[test]
fn claude_code_snapshot() {
    insta::assert_snapshot!("claude_code", format(Target::ClaudeCode, CONTENT));
}

#[test]
fn agents_md_snapshot() {
    insta::assert_snapshot!("agents_md", format(Target::AgentsMd, CONTENT));
}

#[test]
fn filenames_are_correct() {
    assert_eq!(filename(Target::ClaudeCode), "safessh.md");
    assert_eq!(filename(Target::AgentsMd), "AGENTS.md");
}

#[test]
fn claude_code_has_yaml_frontmatter() {
    let out = format(Target::ClaudeCode, CONTENT);
    assert!(out.starts_with("---\nname: safessh\ndescription: "));
    assert!(out.contains("\n---\n\n"));
    assert!(out.ends_with(CONTENT));
}

#[test]
fn agents_md_has_section_header() {
    let out = format(Target::AgentsMd, CONTENT);
    assert!(out.starts_with("## safessh\n\n"));
    assert!(out.ends_with("\n"));
    assert!(out.contains(CONTENT));
}

#[test]
fn content_covers_required_topics() {
    // The body must cover the six topics required by Task 16's acceptance
    // criteria. These checks are intentionally loose-string but anchored to
    // tokens the body should contain so that snapshot drift does not silently
    // drop a required section.
    let body = CONTENT;
    // 1. When to invoke
    assert!(body.contains("When to invoke"), "missing 'When to invoke'");
    // 2. Subcommand reference
    assert!(body.contains("Subcommands"), "missing 'Subcommands'");
    assert!(body.contains("exec"), "missing 'exec' subcommand");
    assert!(body.contains("project list"), "missing 'project list'");
    assert!(body.contains("project add"), "missing 'project add'");
    assert!(body.contains("approve"), "missing 'approve'");
    // 3. BLOCKED handling
    assert!(body.contains("BLOCKED:"), "missing 'BLOCKED:' guidance");
    assert!(
        body.contains("verbatim"),
        "missing 'verbatim' surface guidance"
    );
    assert!(
        body.contains("Do not retry"),
        "missing 'do not retry' guidance"
    );
    // 4. Output framing format
    assert!(body.contains("<stdout>"), "missing '<stdout>' framing");
    assert!(body.contains("<stderr>"), "missing '<stderr>' framing");
    assert!(
        body.contains("<exit code="),
        "missing '<exit code=' framing"
    );
    // 5. --yolo discouragement
    assert!(body.contains("--yolo"), "missing '--yolo' discussion");
    assert!(
        body.contains("Do not use") || body.contains("discouraged"),
        "missing '--yolo' discouragement"
    );
    // 6. Project list discovery
    assert!(
        body.contains("safessh project list"),
        "missing 'safessh project list' discovery"
    );
}
