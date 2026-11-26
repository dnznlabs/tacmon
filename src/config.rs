//! Configuration: a TOML file, every field optional. The shell owns the theme; each monitor owns a section.

use crate::monitors::{btc_eth::BtcEthConfig, fast_bridge::FastBridgeConfig};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The animated start screen. `--no-splash` turns it off for one run.
    pub splash: bool,
    pub theme: ThemeConfig,
    pub fast_bridge: FastBridgeConfig,
    pub btc_eth: BtcEthConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    /// Any ratatui colour: a name ("cyan", "lightmagenta"), "#rrggbb", or a 256-colour index ("242").
    pub accent: String,
    pub good: String,
    pub warn: String,
    pub bad: String,
    pub dim: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { splash: true, theme: ThemeConfig::default(), fast_bridge: FastBridgeConfig::default(), btc_eth: BtcEthConfig::default() }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self { accent: "cyan".into(), good: "green".into(), warn: "yellow".into(), bad: "red".into(), dim: "242".into() }
    }
}

impl Config {
    /// `$XDG_CONFIG_HOME/tacmon/config.toml`, or `~/.config/...`.
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("tacmon").join("config.toml"))
    }

    /// An explicit path must exist; the default path is used only when it does. With the configuration comes the
    /// file it was read from, `None` when there was none to read.
    pub fn load(explicit: Option<&Path>) -> Result<(Self, Option<PathBuf>)> {
        let path = match explicit {
            Some(p) => Some(p.to_path_buf()),
            None => Self::default_path().filter(|p| p.exists()),
        };
        let Some(path) = path else { return Ok((Self::default(), None)) };
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let cfg = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok((cfg, Some(path)))
    }
}
