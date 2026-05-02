//! Shared test helpers for `safessh-cli` integration tests.
//!
//! Each test gets a fresh `tempfile::TempDir` rooted at `SAFESSH_HOME` plus
//! a `safessh` subprocess builder pre-configured with that env var. Tests
//! that need richer environments (e.g. an `ssh` wrapper on PATH) build
//! their own helpers — this module is intentionally minimal.

#![allow(dead_code)]

use assert_cmd::Command;
use std::path::Path;

pub struct TestEnv {
    home: tempfile::TempDir,
}

impl TestEnv {
    pub fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
        }
    }

    pub fn home(&self) -> &Path {
        self.home.path()
    }

    /// Build a `safessh` subprocess with `SAFESSH_HOME` set to this env's
    /// tempdir and `EDITOR=true` so `project edit` is a no-op. Also points
    /// `SSH_CONFIG_PATH` at `<home>/ssh_config` so the per-test fixture
    /// written by [`Self::write_ssh_config`] is what the subprocess reads
    /// (and tests don't pollute the real `~/.ssh/config`). When that file
    /// doesn't exist, the loader returns an empty snapshot (no error).
    pub fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("safessh").unwrap();
        c.env("SAFESSH_HOME", self.home.path());
        c.env("EDITOR", "true");
        c.env("SSH_CONFIG_PATH", self.ssh_config_path());
        c
    }

    /// Path of the per-test ssh-config fixture (whether or not it exists).
    pub fn ssh_config_path(&self) -> std::path::PathBuf {
        self.home.path().join("ssh_config")
    }

    /// Write the per-test ssh-config fixture. Subprocesses spawned via
    /// [`Self::cmd`] will read it because `SSH_CONFIG_PATH` is wired in.
    pub fn write_ssh_config(&self, body: &str) {
        std::fs::write(self.ssh_config_path(), body).unwrap();
    }

    /// Write a project TOML to `<home>/config/projects/<name>.toml`.
    pub fn write_project(&self, name: &str, body: &str) {
        let projects = self.home.path().join("config/projects");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::write(projects.join(format!("{name}.toml")), body).unwrap();
    }

    /// Read back a project's TOML for assertion purposes.
    pub fn read_project(&self, name: &str) -> String {
        let path = self
            .home
            .path()
            .join("config/projects")
            .join(format!("{name}.toml"));
        std::fs::read_to_string(path).unwrap()
    }
}
