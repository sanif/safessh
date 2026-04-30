//! ControlMaster argv helpers and socket-directory bootstrap.
//!
//! OpenSSH's connection multiplexing uses a Unix socket whose path is
//! supplied via `-o ControlPath=...`. The socket directory must be
//! private (mode 0o700) on Unix because anyone with write access to it
//! could hijack the multiplexed control channel and execute commands as
//! the user without re-authenticating.
//!
//! `argv_options` returns the three opts that turn on auto-multiplexing
//! with a 60-second persistence window — long enough to amortize
//! authentication across a burst of agent calls, short enough to avoid
//! pinning forgotten sockets.

use std::path::Path;

/// Build the `-o` flag pairs that enable ControlMaster auto-multiplexing
/// against a socket inside `control_dir`.
///
/// `%C` is OpenSSH's per-connection token (a hash of host/port/user) so
/// distinct targets get distinct sockets within the same directory.
pub fn argv_options(control_dir: &Path) -> Vec<String> {
    let socket = control_dir.join("%C");
    vec![
        "-o".into(),
        "ControlMaster=auto".into(),
        "-o".into(),
        format!("ControlPath={}", socket.display()),
        "-o".into(),
        "ControlPersist=60s".into(),
    ]
}

/// Create `control_dir` if missing and (on Unix) clamp its mode to 0o700.
///
/// Idempotent: re-tightens the permissions every call so a directory
/// reused across runs cannot loosen over time.
pub fn ensure_dir(control_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(control_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(control_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
