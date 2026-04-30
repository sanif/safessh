use safessh_audit::event;
use safessh_audit::jsonl::AuditWriter;
use safessh_core::types::ParsedCommand;
use safessh_storage::paths::Paths;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// `Paths::user()` reads `SAFESSH_HOME` from the process env, which is shared
/// across cargo's parallel test threads. Serialize the env-mutation segment
/// so two tests can't observe each other's directories.
fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn temp_paths() -> (tempfile::TempDir, Paths, MutexGuard<'static, ()>) {
    let guard = env_lock();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("SAFESSH_HOME", dir.path());
    let p = Paths::user().unwrap();
    p.ensure_dirs().unwrap();
    (dir, p, guard)
}

fn parsed(raw: &str, args: Vec<String>) -> ParsedCommand {
    ParsedCommand {
        binary: "echo".into(),
        flags: vec![],
        args,
        redirects: vec![],
        pipes: vec![],
        env_mutations: vec![],
        raw: raw.into(),
    }
}

#[test]
fn append_writes_one_line() {
    let (_d, paths, _g) = temp_paths();
    let w = AuditWriter::open(&paths).unwrap();
    let p = ParsedCommand {
        binary: "ls".into(),
        flags: vec![],
        args: vec!["/etc".into()],
        redirects: vec![],
        pipes: vec![],
        env_mutations: vec![],
        raw: "ls /etc".into(),
    };
    w.append(&event::exec_attempt("prod", &p, "allow")).unwrap();
    let raw = std::fs::read_to_string(paths.audit_log()).unwrap();
    assert_eq!(raw.lines().count(), 1);
    assert!(raw.contains("exec_attempt"));
}

#[test]
fn redacts_aws_key_in_audit() {
    let (_d, paths, _g) = temp_paths();
    let w = AuditWriter::open(&paths).unwrap();
    let p = parsed(
        "echo AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE",
        vec!["AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE".into()],
    );
    w.append(&event::exec_attempt("prod", &p, "allow")).unwrap();
    let raw = std::fs::read_to_string(paths.audit_log()).unwrap();
    assert!(
        !raw.contains("AKIAIOSFODNN7EXAMPLE"),
        "audit file leaked the AWS key: {raw}"
    );
    assert!(raw.contains("REDACTED"), "expected REDACTED marker: {raw}");
}
