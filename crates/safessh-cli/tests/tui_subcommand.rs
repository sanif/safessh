//! `safessh tui` integration test — only the non-TTY refusal path is
//! testable headlessly; opening the TUI requires a real terminal.

use assert_cmd::Command;

#[test]
fn tui_refuses_without_tty() {
    // assert_cmd's Command::output() runs the child with piped stdin/stdout,
    // so atty::is(Stdin) is false. The TUI should refuse with exit 1.
    let out = Command::cargo_bin("safessh")
        .unwrap()
        .args(["tui"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("tui requires a TTY"),
        "expected TTY refusal: {stderr}"
    );
}
