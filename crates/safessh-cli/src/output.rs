//! Framed output writer for `safessh exec`.
//!
//! The agent (or human) consuming `safessh exec` reads back a strict,
//! machine-parseable wrapper around stdout/stderr/exit-code so it never has to
//! disambiguate `safessh`'s own diagnostics from the remote process's bytes.
//! The format is byte-stable; downstream tools (e.g. the Claude Code skill)
//! parse it verbatim.

use std::io::Write;

/// Write the framed exec result to stdout.
///
/// Format (newline-terminated, byte-exact):
///
/// ```text
/// <stdout>
/// <bytes>
/// </stdout>
/// <stderr>
/// <bytes>
/// </stderr>
/// <exit code="N" duration="<ms>ms"/>
/// ```
///
/// When `truncated` is `true`, the exit tag gains a ` truncated="true"`
/// attribute before the closing slash.
pub fn write_framed(stdout: &[u8], stderr: &[u8], exit: i32, duration_ms: u64, truncated: bool) {
    let out = std::io::stdout();
    let mut h = out.lock();
    let _ = writeln!(h, "<stdout>");
    let _ = h.write_all(stdout);
    let _ = writeln!(h, "</stdout>");
    let _ = writeln!(h, "<stderr>");
    let _ = h.write_all(stderr);
    let _ = writeln!(h, "</stderr>");
    let trunc = if truncated {
        r#" truncated="true""#
    } else {
        ""
    };
    let _ = writeln!(
        h,
        r#"<exit code="{exit}" duration="{duration_ms}ms"{trunc}/>"#
    );
}
