//! Project TOML schema and CRUD store.

use crate::atomic;
use crate::paths::Paths;
use safessh_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub default_target: String,
    pub targets: Vec<Target>,
    #[serde(default)]
    pub policy: Policy,
    #[serde(default)]
    pub approvals: Approvals,
    #[serde(default)]
    pub output: OutputCaps,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Target {
    Inline {
        name: String,
        host: String,
        #[serde(default = "default_port")]
        port: u16,
        user: String,
        #[serde(default)]
        identity_file: Option<PathBuf>,
        #[serde(default)]
        proxy_jump: Option<String>,
        #[serde(default)]
        keychain_secret: Option<String>,
    },
    SshConfigAlias {
        name: String,
        ssh_config_alias: String,
    },
}

fn default_port() -> u16 {
    22
}

impl Target {
    pub fn name(&self) -> &str {
        match self {
            Target::SshConfigAlias { name, .. } | Target::Inline { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub require_approval: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approvals {
    #[serde(default = "default_timed_minutes")]
    pub timed_default_minutes: u32,
    #[serde(default)]
    pub yolo: bool,
}

fn default_timed_minutes() -> u32 {
    30
}

impl Default for Approvals {
    fn default() -> Self {
        Self {
            timed_default_minutes: 30,
            yolo: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputCaps {
    #[serde(default = "default_stdout_cap")]
    pub stdout_cap_bytes: u64,
    #[serde(default = "default_stderr_cap")]
    pub stderr_cap_bytes: u64,
    #[serde(default = "default_file_read_cap")]
    pub file_read_cap_bytes: u64,
    #[serde(default = "default_tunnel_ttl")]
    pub tunnel_ttl_minutes: u32,
}

fn default_stdout_cap() -> u64 {
    1_048_576
}
fn default_stderr_cap() -> u64 {
    262_144
}
fn default_file_read_cap() -> u64 {
    5_242_880
}
fn default_tunnel_ttl() -> u32 {
    30
}

impl Default for OutputCaps {
    fn default() -> Self {
        Self {
            stdout_cap_bytes: default_stdout_cap(),
            stderr_cap_bytes: default_stderr_cap(),
            file_read_cap_bytes: default_file_read_cap(),
            tunnel_ttl_minutes: default_tunnel_ttl(),
        }
    }
}

pub struct ProjectStore {
    paths: Paths,
}

impl ProjectStore {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    /// Borrow the underlying [`Paths`] so callers can pass the same handle
    /// to other storage modules (e.g. `ssh_config::SshConfigSnapshot::load`)
    /// without re-walking env vars.
    pub fn paths_ref(&self) -> &Paths {
        &self.paths
    }

    pub fn save(&self, project: &Project) -> Result<()> {
        let path = self
            .paths
            .projects_dir()
            .join(format!("{}.toml", project.name));
        let toml = toml::to_string_pretty(project).map_err(|e| Error::Serde(e.to_string()))?;
        atomic::write_string(&path, &toml)?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> Result<Project> {
        let path = self.paths.projects_dir().join(format!("{id}.toml"));
        let raw =
            std::fs::read_to_string(&path).map_err(|_| Error::ProjectNotFound(id.to_string()))?;
        toml::from_str(&raw).map_err(|e| Error::Storage(format!("{}: {e}", path.display())))
    }

    pub fn list(&self) -> Result<Vec<String>> {
        let dir = self.paths.projects_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if let Some(name) = entry.path().file_stem().and_then(|s| s.to_str()) {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("toml") {
                    out.push(name.to_string());
                }
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn remove(&self, id: &str) -> Result<()> {
        let path = self.paths.projects_dir().join(format!("{id}.toml"));
        std::fs::remove_file(&path).map_err(|_| Error::ProjectNotFound(id.to_string()))?;
        Ok(())
    }
}
