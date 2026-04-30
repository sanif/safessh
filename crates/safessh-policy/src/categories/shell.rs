//! Shell command category matchers.
//!
//! Each `is_*` function inspects a [`ParsedCommand`] and returns `true` when
//! the command falls into that category. The aggregator
//! [`match_shell_categories`] returns the (unsorted) list of matched category
//! names; combining/sorting/deduping is the job of `categories::match_all`
//! (added in Task 11).
//!
//! v0.1 only inspects the first pipeline stage. `redirects` is empty in
//! practice (Task 9 left it as a no-op), so the disk-redirect check is a
//! placeholder until a later task wires up redirect parsing.

use crate::ast::ParsedCommand;

/// Aggregate every shell category that matches `cmd`.
///
/// Order follows the source code below; callers that need a stable / deduped
/// list should go through `categories::match_all` (Task 11) which sorts and
/// dedups across both shell and SQL categories.
pub fn match_shell_categories(cmd: &ParsedCommand) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if is_read_safe(cmd) {
        out.push("read:safe".into());
    }
    if is_destructive_filesystem(cmd) {
        out.push("destructive:filesystem".into());
    }
    if is_destructive_disk(cmd) {
        out.push("destructive:disk".into());
    }
    if is_privilege_escalation(cmd) {
        out.push("privilege:escalation".into());
    }
    if is_system_control(cmd) {
        out.push("system:control".into());
    }
    if is_network_listen(cmd) {
        out.push("network:listen".into());
    }
    if is_exec_opaque(cmd) {
        out.push("exec:opaque".into());
    }
    out.sort();
    out.dedup();
    out
}

/// Binaries whose default behaviour is read-only (no observable side effects
/// on the remote host beyond stdout/stderr).
const READ_SAFE_BINS: &[&str] = &[
    "ls", "cat", "head", "tail", "grep", "stat", "file", "wc", "sort", "uniq", "which", "whereis",
    "pwd", "id", "whoami", "uname", "date", "uptime", "df", "du", "free", "ps", "top", "echo",
    "printf",
];

/// True for read-only commands. `find` is special-cased: it counts as
/// read-safe only when neither `-delete` nor `-exec` appears in the original
/// argv.
pub fn is_read_safe(cmd: &ParsedCommand) -> bool {
    if cmd.binary == "find" {
        return !find_has_delete_or_exec(cmd);
    }
    READ_SAFE_BINS.contains(&cmd.binary.as_str())
}

/// True iff the original argv of a `find` invocation contains `-delete` or
/// `-exec` as a whole token.
///
/// The shell parser bundles short flags character-by-character (so `-delete`
/// becomes `-d -e -l -e -t -e` in `cmd.flags`). To recover the original
/// tokens we re-tokenize `cmd.raw` with `shell_words`. Tokenisation already
/// succeeded once (the parser ran on the same input), so a failure here is
/// vanishingly unlikely; if it does happen we conservatively report `true`
/// (treat as not-read-safe / destructive) to default-deny.
fn find_has_delete_or_exec(cmd: &ParsedCommand) -> bool {
    match shell_words::split(&cmd.raw) {
        Ok(tokens) => tokens.iter().any(|t| t == "-delete" || t == "-exec"),
        Err(_) => true,
    }
}

const DESTRUCTIVE_FS_BINS: &[&str] = &["rm", "rmdir", "unlink", "shred"];

/// True for filesystem-mutating commands (file/dir removal) and `find` when
/// it is invoked with `-delete` or `-exec`.
pub fn is_destructive_filesystem(cmd: &ParsedCommand) -> bool {
    if DESTRUCTIVE_FS_BINS.contains(&cmd.binary.as_str()) {
        return true;
    }
    if cmd.binary == "find" && find_has_delete_or_exec(cmd) {
        return true;
    }
    false
}

const DESTRUCTIVE_DISK_BINS: &[&str] = &["dd", "fdisk", "parted", "wipefs", "mkfs"];

/// True for raw-disk-touching commands (`dd`, `mkfs.*`, etc.) or any redirect
/// targeting a `/dev/sd*`, `/dev/nvme*`, or `/dev/disk*` block device.
///
/// The redirect check is currently a no-op because the parser leaves
/// `redirects` empty in v0.1; it is wired in for forward compatibility.
pub fn is_destructive_disk(cmd: &ParsedCommand) -> bool {
    if DESTRUCTIVE_DISK_BINS.contains(&cmd.binary.as_str()) {
        return true;
    }
    if cmd.binary.starts_with("mkfs.") {
        return true;
    }
    cmd.redirects.iter().any(|r| {
        r.contains("/dev/sd") || r.contains("/dev/nvme") || r.contains("/dev/disk")
    })
}

const PRIV_ESC_BINS: &[&str] = &["sudo", "su", "doas", "pkexec"];

/// True for the canonical privilege-escalation entry points.
pub fn is_privilege_escalation(cmd: &ParsedCommand) -> bool {
    PRIV_ESC_BINS.contains(&cmd.binary.as_str())
}

const SYSTEM_CONTROL_BINS: &[&str] = &["shutdown", "reboot", "halt", "poweroff"];
const SYSTEMCTL_DESTRUCTIVE_VERBS: &[&str] = &["stop", "disable", "mask", "kill"];

/// True for power-state commands and for `systemctl` invocations whose
/// subcommand is a destructive verb (`stop`/`disable`/`mask`/`kill`).
/// Read verbs like `status` or `is-active` are intentionally excluded.
pub fn is_system_control(cmd: &ParsedCommand) -> bool {
    if SYSTEM_CONTROL_BINS.contains(&cmd.binary.as_str()) {
        return true;
    }
    if cmd.binary == "systemctl" {
        return cmd
            .args
            .iter()
            .any(|a| SYSTEMCTL_DESTRUCTIVE_VERBS.contains(&a.as_str()));
    }
    false
}

/// True for commands that open a listening socket: `nc -l`, `ncat -l`,
/// `socat ... LISTEN ...`, or `python[3] -m http.server`.
pub fn is_network_listen(cmd: &ParsedCommand) -> bool {
    if cmd.binary == "nc" || cmd.binary == "ncat" {
        return cmd.flags.iter().any(|f| f == "-l");
    }
    if cmd.binary == "socat" {
        return cmd.args.iter().any(|a| a.contains("LISTEN"));
    }
    if cmd.binary == "python" || cmd.binary == "python3" {
        return cmd.flags.iter().any(|f| f == "-m")
            && cmd.args.iter().any(|a| a == "http.server");
    }
    false
}

/// Interpreters whose `-c` / `-e` payloads carry semantic structure we *do*
/// understand (SQL). They route through Task 11's `categories::sql` matchers
/// instead of opaque-shelling.
const KNOWN_SEMANTIC_INTERPRETERS: &[&str] = &["psql", "mysql", "sqlite3"];

/// True for arbitrary-code execution where we cannot statically inspect the
/// payload: `sh -c '...'`, `python -c '...'`, `perl -e '...'`, `eval ...`.
///
/// Excludes [`KNOWN_SEMANTIC_INTERPRETERS`] (psql/mysql/sqlite3) — those have
/// dedicated SQL-aware matchers in Task 11.
pub fn is_exec_opaque(cmd: &ParsedCommand) -> bool {
    if KNOWN_SEMANTIC_INTERPRETERS.contains(&cmd.binary.as_str()) {
        return false;
    }
    const OPAQUE_SHELLS: &[&str] = &["sh", "bash", "zsh", "fish", "ksh", "dash"];
    if OPAQUE_SHELLS.contains(&cmd.binary.as_str())
        && cmd.flags.iter().any(|f| f == "-c")
    {
        return true;
    }
    if (cmd.binary == "python" || cmd.binary == "python3")
        && cmd.flags.iter().any(|f| f == "-c")
    {
        return true;
    }
    if (cmd.binary == "perl" || cmd.binary == "ruby")
        && cmd.flags.iter().any(|f| f == "-e")
    {
        return true;
    }
    if cmd.binary == "eval" {
        return true;
    }
    false
}
