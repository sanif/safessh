//! Integration tests for `safessh_policy::categories::shell`.
//!
//! Each of the seven categories has at least 3 positive cases and 1 negative
//! case, plus dedicated tests for the spec-mandated edge cases (find -delete,
//! psql -c excluded from exec:opaque, systemctl status not system:control,
//! etc.) and for the `match_shell_categories` aggregator.

use safessh_core::types::ParsedCommand;
use safessh_policy::categories::shell::{
    is_destructive_disk, is_destructive_filesystem, is_exec_opaque, is_network_listen,
    is_privilege_escalation, is_read_safe, is_system_control, match_shell_categories,
};
use safessh_policy::parser::parse;

/// Parse `s` into a single [`ParsedCommand`]. Tests panic on parse failure
/// because every input here is hand-crafted and known good.
fn p(s: &str) -> ParsedCommand {
    parse(s)
        .expect("test input must parse")
        .pop()
        .expect("at least one command")
}

// ----- read:safe -------------------------------------------------------------

#[test]
fn read_safe_positives() {
    assert!(is_read_safe(&p("ls -la /etc")));
    assert!(is_read_safe(&p("cat /etc/hostname")));
    assert!(is_read_safe(&p("grep -i pattern file.txt")));
}

#[test]
fn read_safe_find_without_delete_or_exec() {
    // Spec: `is_read_safe(parse("find / -name foo")) == true`.
    assert!(is_read_safe(&p("find / -name foo")));
}

#[test]
fn read_safe_negative() {
    assert!(!is_read_safe(&p("rm -rf /etc")));
}

#[test]
fn read_safe_find_with_delete_is_not_read_safe() {
    assert!(!is_read_safe(&p("find / -name foo -delete")));
}

#[test]
fn read_safe_find_with_exec_is_not_read_safe() {
    assert!(!is_read_safe(&p("find / -name foo -exec rm {} ;")));
}

// ----- destructive:filesystem -----------------------------------------------

#[test]
fn destructive_fs_positives() {
    assert!(is_destructive_filesystem(&p("rm -rf /var/log")));
    assert!(is_destructive_filesystem(&p("rmdir foo")));
    assert!(is_destructive_filesystem(&p("shred secrets.txt")));
}

#[test]
fn destructive_fs_find_with_delete() {
    // Spec: `is_destructive_filesystem(parse("find / -name foo -delete")) == true`.
    assert!(is_destructive_filesystem(&p("find / -name foo -delete")));
}

#[test]
fn destructive_fs_negative() {
    assert!(!is_destructive_filesystem(&p("ls /var")));
}

// ----- destructive:disk ------------------------------------------------------

#[test]
fn destructive_disk_positives() {
    assert!(is_destructive_disk(&p("dd if=/dev/zero of=/dev/sda")));
    assert!(is_destructive_disk(&p("fdisk /dev/sda")));
    assert!(is_destructive_disk(&p("wipefs -a /dev/sdb")));
}

#[test]
fn destructive_disk_mkfs_variants() {
    assert!(is_destructive_disk(&p("mkfs.ext4 /dev/sda1")));
    assert!(is_destructive_disk(&p("mkfs.xfs /dev/nvme0n1p1")));
}

#[test]
fn destructive_disk_negative() {
    assert!(!is_destructive_disk(&p("ls /dev/sda")));
}

// ----- privilege:escalation --------------------------------------------------

#[test]
fn priv_esc_positives() {
    assert!(is_privilege_escalation(&p("sudo ls")));
    assert!(is_privilege_escalation(&p("su -")));
    assert!(is_privilege_escalation(&p("doas vi /etc/hosts")));
}

#[test]
fn priv_esc_pkexec() {
    assert!(is_privilege_escalation(&p(
        "pkexec systemctl restart nginx"
    )));
}

#[test]
fn priv_esc_negative() {
    assert!(!is_privilege_escalation(&p("ls /etc")));
}

// ----- system:control --------------------------------------------------------

#[test]
fn system_control_positives() {
    assert!(is_system_control(&p("shutdown -h now")));
    assert!(is_system_control(&p("reboot")));
    assert!(is_system_control(&p("poweroff")));
}

#[test]
fn system_control_systemctl_stop() {
    // Spec: `is_system_control(parse("systemctl stop nginx")) == true`.
    assert!(is_system_control(&p("systemctl stop nginx")));
    assert!(is_system_control(&p("systemctl disable nginx")));
    assert!(is_system_control(&p("systemctl mask nginx")));
    assert!(is_system_control(&p("systemctl kill nginx")));
}

#[test]
fn system_control_systemctl_status_is_not() {
    // Spec: `is_system_control(parse("systemctl status nginx")) == false`.
    assert!(!is_system_control(&p("systemctl status nginx")));
    assert!(!is_system_control(&p("systemctl is-active nginx")));
}

#[test]
fn system_control_negative() {
    assert!(!is_system_control(&p("ls /var")));
}

// ----- network:listen --------------------------------------------------------

#[test]
fn network_listen_positives() {
    assert!(is_network_listen(&p("nc -l 8080")));
    assert!(is_network_listen(&p("ncat -l 9000")));
    assert!(is_network_listen(&p(
        "socat TCP-LISTEN:8080,fork EXEC:/bin/cat"
    )));
}

#[test]
fn network_listen_python_http_server() {
    assert!(is_network_listen(&p("python -m http.server")));
    assert!(is_network_listen(&p("python3 -m http.server 8000")));
}

#[test]
fn network_listen_negative() {
    // `nc` without `-l` is a client connect, not a listener.
    assert!(!is_network_listen(&p("nc example.com 80")));
    // `python -m something_else` is not http.server.
    assert!(!is_network_listen(&p("python -m pip install foo")));
}

// ----- exec:opaque -----------------------------------------------------------

#[test]
fn exec_opaque_positives() {
    assert!(is_exec_opaque(&p("bash -c 'rm -rf /'")));
    assert!(is_exec_opaque(&p("sh -c uptime")));
    assert!(is_exec_opaque(&p("perl -e 'print 1'")));
}

#[test]
fn exec_opaque_python_dash_c() {
    // Spec: `is_exec_opaque(parse("python -c print(1)")) == true`.
    assert!(is_exec_opaque(&p("python -c print(1)")));
    assert!(is_exec_opaque(&p("python3 -c print(2)")));
}

#[test]
fn exec_opaque_eval() {
    assert!(is_exec_opaque(&p("eval some_var")));
}

#[test]
fn exec_opaque_excludes_psql() {
    // Spec: `is_exec_opaque(parse("psql -c SELECT 1")) == false`. Routes via
    // Task 11's `categories::sql` matchers instead.
    assert!(!is_exec_opaque(&p("psql -c SELECT 1")));
    assert!(!is_exec_opaque(&p("mysql -e SELECT 1")));
    assert!(!is_exec_opaque(&p("sqlite3 db.sqlite")));
}

#[test]
fn exec_opaque_negative() {
    assert!(!is_exec_opaque(&p("ls -la")));
    // `bash` with no `-c` is not opaque-exec from a static-analysis POV.
    assert!(!is_exec_opaque(&p("bash script.sh")));
}

// ----- aggregator ------------------------------------------------------------

#[test]
fn aggregator_returns_sorted_deduped_list() {
    let cats = match_shell_categories(&p("rm -rf /var/log"));
    assert_eq!(cats, vec!["destructive:filesystem".to_string()]);

    // sudo by itself counts as privilege:escalation.
    let cats = match_shell_categories(&p("sudo ls"));
    assert_eq!(cats, vec!["privilege:escalation".to_string()]);

    // ls is read:safe.
    let cats = match_shell_categories(&p("ls /etc"));
    assert_eq!(cats, vec!["read:safe".to_string()]);

    // No matches → empty vec.
    let cats = match_shell_categories(&p("git status"));
    assert!(cats.is_empty());
}

#[test]
fn aggregator_is_sorted() {
    // Whichever combination of categories matches, the output must be in
    // lexicographic order. Verified by checking a few inputs.
    for cmd in [
        "rm -rf /var/log",
        "ls /etc",
        "sudo systemctl stop nginx",
        "shutdown -h now",
        "find / -name foo -delete",
    ] {
        let cats = match_shell_categories(&p(cmd));
        let mut sorted = cats.clone();
        sorted.sort();
        assert_eq!(cats, sorted, "categories for {cmd:?} not sorted: {cats:?}");
    }
}
