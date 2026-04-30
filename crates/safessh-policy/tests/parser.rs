//! Integration tests for `safessh_policy::parser`.
//!
//! Covers the acceptance criteria for v0.1 Task 9 plus property-based fuzz of
//! arbitrary 0..200-char inputs to enforce SAFETY-INVARIANT-1: parse must
//! never panic and must return either `Ok` or `ParseError::Opaque`.

use proptest::prelude::*;
use safessh_policy::parser::{parse, ParseError};

#[test]
fn parses_rm_rf_with_bundled_flags() {
    let v = parse("rm -rf /var/log").expect("valid command");
    assert_eq!(v.len(), 1);
    let cmd = &v[0];
    assert_eq!(cmd.binary, "rm");
    assert_eq!(cmd.flags, vec!["-r".to_string(), "-f".to_string()]);
    assert_eq!(cmd.args, vec!["/var/log".to_string()]);
    assert_eq!(cmd.raw, "rm -rf /var/log");
    assert!(cmd.pipes.is_empty());
    assert!(cmd.redirects.is_empty());
    assert!(cmd.env_mutations.is_empty());
}

#[test]
fn parses_ls_la() {
    let v = parse("ls -la /etc").expect("valid command");
    assert_eq!(v.len(), 1);
    let cmd = &v[0];
    assert_eq!(cmd.binary, "ls");
    assert_eq!(cmd.flags, vec!["-l".to_string(), "-a".to_string()]);
    assert_eq!(cmd.args, vec!["/etc".to_string()]);
}

#[test]
fn long_flags_stay_whole() {
    let v = parse("rm --recursive --force /tmp/x").expect("valid command");
    let cmd = &v[0];
    assert_eq!(cmd.binary, "rm");
    assert_eq!(
        cmd.flags,
        vec!["--recursive".to_string(), "--force".to_string()]
    );
    assert_eq!(cmd.args, vec!["/tmp/x".to_string()]);
}

#[test]
fn mixed_long_and_short_flags() {
    let v = parse("grep -i --color=auto pattern file.txt").expect("valid command");
    let cmd = &v[0];
    assert_eq!(cmd.binary, "grep");
    assert_eq!(
        cmd.flags,
        vec!["-i".to_string(), "--color=auto".to_string()]
    );
    assert_eq!(
        cmd.args,
        vec!["pattern".to_string(), "file.txt".to_string()]
    );
}

#[test]
fn quoted_args_preserve_spaces() {
    let v = parse(r#"echo "hello world""#).expect("valid command");
    let cmd = &v[0];
    assert_eq!(cmd.binary, "echo");
    assert_eq!(cmd.args, vec!["hello world".to_string()]);
}

#[test]
fn opaque_on_unterminated_quote() {
    let r = parse("rm -rf '/var");
    assert!(
        matches!(r, Err(ParseError::Opaque(_))),
        "expected Opaque, got {r:?}"
    );
}

#[test]
fn opaque_on_unterminated_double_quote() {
    let r = parse(r#"echo "hello"#);
    assert!(
        matches!(r, Err(ParseError::Opaque(_))),
        "expected Opaque, got {r:?}"
    );
}

#[test]
fn opaque_on_empty_string() {
    let r = parse("");
    assert!(
        matches!(r, Err(ParseError::Opaque(_))),
        "expected Opaque, got {r:?}"
    );
}

#[test]
fn opaque_on_whitespace_only() {
    let r = parse("   \t\n  ");
    assert!(
        matches!(r, Err(ParseError::Opaque(_))),
        "expected Opaque, got {r:?}"
    );
}

#[test]
fn bare_dash_is_positional_arg() {
    // `cat -` reads from stdin; the bare `-` should be an arg, not a flag.
    let v = parse("cat -").expect("valid command");
    let cmd = &v[0];
    assert_eq!(cmd.binary, "cat");
    assert!(cmd.flags.is_empty());
    assert_eq!(cmd.args, vec!["-".to_string()]);
}

#[test]
fn raw_field_preserved() {
    let raw = "ls -la /etc";
    let v = parse(raw).expect("valid command");
    assert_eq!(v[0].raw, raw);
}

proptest! {
    /// SAFETY-INVARIANT-1: any input up to 200 chars must yield `Ok` or
    /// `ParseError::Opaque` — never a panic and never any other error
    /// variant. The single-variant `ParseError` enum makes this trivially
    /// true at the type level, but we still drive the code path to catch
    /// panics inside conch-parser or shell-words.
    #[test]
    fn never_panics_on_arbitrary_input(s in "\\PC{0,200}") {
        let _ = parse(&s);
    }

    /// Stress the bundled-flag splitter on arbitrary-looking flag strings.
    #[test]
    fn never_panics_on_flag_like_input(s in "-{1,3}[a-zA-Z0-9]{0,20}") {
        let input = format!("cmd {s}");
        let _ = parse(&input);
    }
}
