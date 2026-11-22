//! The user's gamma configuration (`<config dir>/gamma/config.toml`) and data folder.

use anyhow::Result;
use gamma_core::project::RemoteCfg;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserConfig {
    #[serde(default, rename = "remote")]
    pub remotes: Vec<RemoteCfg>,
    #[serde(default)]
    pub updates: Updates,
    #[serde(default)]
    pub agent: AgentCfg,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Updates {
    /// `stable` or `beta`.
    #[serde(default = "stable")]
    pub channel: String,
    /// Check for updates automatically (the only network traffic gamma starts by itself).
    #[serde(default = "yes")]
    pub check: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Updates { channel: stable(), check: true }
    }
}

fn stable() -> String {
    "stable".into()
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AgentCfg {
    /// `claude` or `codex`.
    #[serde(default)]
    pub provider: Option<String>,
    /// Extra arguments for the agent program.
    #[serde(default)]
    pub args: Vec<String>,
}

pub fn config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("GAMMA_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("gamma")
}

pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("GAMMA_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("gamma")
}

impl UserConfig {
    pub fn path() -> PathBuf {
        config_dir().join("config.toml")
    }
    pub fn load() -> UserConfig {
        std::fs::read_to_string(Self::path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self) -> Result<()> {
        gamma_core::fsutil::atomic_write(&Self::path(), toml::to_string_pretty(self)?.as_bytes())
    }
}
