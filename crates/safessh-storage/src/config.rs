//! Global config TOML schema and load/save helpers.

use crate::atomic;
use crate::paths::Paths;
use safessh_core::error::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_timed")]
    pub default_timed_minutes: u32,
    #[serde(default = "default_tunnel_ttl")]
    pub tunnel_ttl_minutes: u32,
    #[serde(default)]
    pub disable_yolo: bool,
    #[serde(default)]
    pub redaction_patterns: Vec<RedactionPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionPattern {
    pub name: String,
    pub regex: String,
}

fn default_timed() -> u32 {
    30
}
fn default_tunnel_ttl() -> u32 {
    30
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_timed_minutes: 30,
            tunnel_ttl_minutes: 30,
            disable_yolo: false,
            redaction_patterns: vec![],
        }
    }
}

pub fn load(paths: &Paths) -> Result<Config> {
    let path = paths.config_file();
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(&path)?;
    toml::from_str(&raw).map_err(|e| Error::Storage(format!("{}: {e}", path.display())))
}

pub fn save(paths: &Paths, config: &Config) -> Result<()> {
    let toml = toml::to_string_pretty(config).map_err(|e| Error::Serde(e.to_string()))?;
    atomic::write_string(&paths.config_file(), &toml)?;
    Ok(())
}
