//! `safessh policy show <category|project>` — print either the binaries
//! associated with a known shell category, a description for SQL categories,
//! or the policy fields of a saved project.
//!
//! Category data is duplicated (small static lists) to avoid widening the
//! visibility of `safessh-policy::categories::shell` constants. If those
//! constants drift, this module needs a corresponding update.

use safessh_core::error::{Error, Result};
use safessh_storage::paths::Paths;
use safessh_storage::project::ProjectStore;

const READ_SAFE_BINS: &[&str] = &[
    "ls",
    "cat",
    "head",
    "tail",
    "grep",
    "stat",
    "file",
    "wc",
    "sort",
    "uniq",
    "which",
    "whereis",
    "pwd",
    "id",
    "whoami",
    "uname",
    "date",
    "uptime",
    "df",
    "du",
    "free",
    "ps",
    "top",
    "echo",
    "printf",
    "find (without -delete/-exec)",
];

const DESTRUCTIVE_FS_BINS: &[&str] = &[
    "rm",
    "rmdir",
    "unlink",
    "shred",
    "find -delete",
    "find -exec",
];

const DESTRUCTIVE_DISK_BINS: &[&str] = &["dd", "fdisk", "parted", "wipefs", "mkfs", "mkfs.*"];

const PRIV_ESC_BINS: &[&str] = &["sudo", "su", "doas", "pkexec"];

const SYSTEM_CONTROL_BINS: &[&str] = &[
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "systemctl stop|disable|mask|kill",
];

const NETWORK_LISTEN_BINS: &[&str] = &[
    "nc -l",
    "ncat -l",
    "socat ... LISTEN",
    "python -m http.server",
    "python3 -m http.server",
];

const EXEC_OPAQUE_BINS: &[&str] = &[
    "sh -c",
    "bash -c",
    "zsh -c",
    "fish -c",
    "ksh -c",
    "dash -c",
    "python -c",
    "python3 -c",
    "perl -e",
    "ruby -e",
    "eval",
];

/// Look up a shell category by name and return the matching binaries (or a
/// human-readable description for SQL categories).
fn shell_category(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "read:safe" => READ_SAFE_BINS,
        "destructive:filesystem" => DESTRUCTIVE_FS_BINS,
        "destructive:disk" => DESTRUCTIVE_DISK_BINS,
        "privilege:escalation" => PRIV_ESC_BINS,
        "system:control" => SYSTEM_CONTROL_BINS,
        "network:listen" => NETWORK_LISTEN_BINS,
        "exec:opaque" => EXEC_OPAQUE_BINS,
        _ => return None,
    })
}

/// Recognised SQL categories handled by `safessh-policy::categories::sql`.
fn sql_category_description(name: &str) -> Option<&'static str> {
    Some(match name {
        "destructive:db" => {
            "SQL-aware: detected from psql -c / mysql -e / sqlite3 argv (DROP, TRUNCATE, ALTER ..., etc.)"
        }
        "db:read" => "SQL-aware: detected from psql -c / mysql -e / sqlite3 argv (SELECT, SHOW, ...)",
        "db:write" => {
            "SQL-aware: detected from psql -c / mysql -e / sqlite3 argv (INSERT, UPDATE, DELETE, ...)"
        }
        _ => return None,
    })
}

pub fn run(what: String) -> Result<()> {
    // Step 1: try category match (shell or SQL).
    if let Some(bins) = shell_category(&what) {
        println!("Category: {what}");
        println!("Matches binaries / patterns:");
        for b in bins {
            println!("  - {b}");
        }
        return Ok(());
    }
    if let Some(desc) = sql_category_description(&what) {
        println!("Category: {what}");
        println!("{desc}");
        return Ok(());
    }

    // Step 2: try project lookup.
    let paths = Paths::user().map_err(Error::Io)?;
    paths.ensure_dirs().map_err(Error::Io)?;
    let store = ProjectStore::new(paths);
    match store.load(&what) {
        Ok(project) => {
            println!("Project: {}", project.name);
            println!("Policy:");
            println!("  allow:");
            for c in &project.policy.allow {
                println!("    - {c}");
            }
            println!("  require_approval:");
            for c in &project.policy.require_approval {
                println!("    - {c}");
            }
            println!("  deny:");
            for c in &project.policy.deny {
                println!("    - {c}");
            }
            Ok(())
        }
        Err(_) => Err(Error::Usage(format!("unknown category or project: {what}"))),
    }
}
