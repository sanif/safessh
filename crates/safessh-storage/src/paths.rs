//! XDG-style path resolution with `SAFESSH_HOME` env override.

use directories::ProjectDirs;
use std::path::PathBuf;

pub struct Paths {
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

impl Paths {
    /// Returns the standard user paths, honoring `SAFESSH_HOME` for tests.
    pub fn user() -> std::io::Result<Self> {
        if let Ok(root) = std::env::var("SAFESSH_HOME") {
            let root = PathBuf::from(root);
            return Ok(Self {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            });
        }
        let dirs = ProjectDirs::from("dev", "safessh", "safessh")
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no project dirs"))?;
        Ok(Self {
            config: dirs.config_dir().to_path_buf(),
            state: dirs.data_local_dir().to_path_buf(),
            cache: dirs.cache_dir().to_path_buf(),
        })
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.config.join("projects")
    }

    pub fn policies_dir(&self) -> PathBuf {
        self.config.join("policies")
    }

    pub fn approvals_dir(&self) -> PathBuf {
        self.state.join("approvals")
    }

    pub fn audit_log(&self) -> PathBuf {
        self.state.join("audit.log")
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    pub fn ssh_config_snapshot(&self) -> PathBuf {
        self.cache.join("ssh-config-snapshot.toml")
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config)?;
        std::fs::create_dir_all(&self.state)?;
        std::fs::create_dir_all(&self.cache)?;
        std::fs::create_dir_all(self.projects_dir())?;
        std::fs::create_dir_all(self.policies_dir())?;
        std::fs::create_dir_all(self.approvals_dir().join("pending"))?;
        std::fs::create_dir_all(self.approvals_dir().join("timed"))?;
        std::fs::create_dir_all(self.approvals_dir().join("always"))?;
        std::fs::create_dir_all(self.approvals_dir().join("blocked"))?;
        Ok(())
    }
}
