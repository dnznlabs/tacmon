//! Configuration: a TOML file, every field optional. The shell owns the theme; each monitor owns a section, and
//! `[shared]` holds what monitors would otherwise each be told the same.

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
    pub shared: SharedConfig,
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

/// The outside services every monitor may be pointed at. Each is empty by default, and a monitor uses one only
/// where its own setting of the same name is empty: a monitor's own section wins. Setting one here sends what each
/// enabled monitor would send to it, so it turns that read on for all of them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SharedConfig {
    /// Ethereum JSON-RPC URL.
    pub eth_rpc: String,
    /// The pool contract's address.
    pub pool_contract: String,
    /// The site an Ethereum transaction's page is on. Nothing is ever requested from it.
    pub eth_explorer: String,
    /// An Esplora API for Bitcoin.
    pub btc_explorer: String,
}

impl SharedConfig {
    /// Fills each of a monitor's settings that is empty with the shared one.
    pub fn fill(&self, eth_rpc: &mut String, pool_contract: &mut String, eth_explorer: &mut String, btc_explorer: &mut String) {
        for (own, shared) in
            [(eth_rpc, &self.eth_rpc), (pool_contract, &self.pool_contract), (eth_explorer, &self.eth_explorer), (btc_explorer, &self.btc_explorer)]
        {
            if own.is_empty() {
                own.clone_from(shared);
            }
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            splash: true,
            theme: ThemeConfig::default(),
            shared: SharedConfig::default(),
            fast_bridge: FastBridgeConfig::default(),
            btc_eth: BtcEthConfig::default(),
        }
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
        let cfg: Self = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok((cfg.resolved(), Some(path)))
    }

    /// The configuration the monitors run on: every monitor's empty setting that `[shared]` has, filled from it.
    pub fn resolved(mut self) -> Self {
        let (s, f, b) = (&self.shared, &mut self.fast_bridge, &mut self.btc_eth);
        s.fill(&mut f.eth_rpc, &mut f.pool_contract, &mut f.eth_explorer, &mut f.btc_explorer);
        s.fill(&mut b.eth_rpc, &mut b.pool_contract, &mut b.eth_explorer, &mut b.btc_explorer);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_monitor_uses_a_shared_setting_only_where_its_own_is_empty() {
        let text = r#"
            [shared]
            btc_explorer = "https://shared.example/api"
            eth_rpc = "https://rpc.example"

            [fast_bridge]
            btc_explorer = "https://own.example/api"
        "#;

        let cfg = toml::from_str::<Config>(text).expect("a config").resolved();

        assert_eq!((cfg.fast_bridge.btc_explorer.as_str(), cfg.btc_eth.btc_explorer.as_str()), ("https://own.example/api", "https://shared.example/api"));
        assert_eq!((cfg.fast_bridge.eth_rpc.as_str(), cfg.btc_eth.eth_rpc.as_str()), ("https://rpc.example", "https://rpc.example"));
        // what neither names stays off
        assert!(cfg.fast_bridge.pool_contract.is_empty() && cfg.btc_eth.eth_explorer.is_empty());
    }

    #[test]
    fn with_nothing_shared_every_outside_read_stays_off_and_an_unknown_shared_setting_is_refused() {
        let unset = Config::default().resolved();

        let unknown = toml::from_str::<Config>("[shared]\nboards = []\n");

        assert!(unset.fast_bridge.btc_explorer.is_empty() && unset.btc_eth.eth_rpc.is_empty() && unset.shared.eth_rpc.is_empty());
        assert!(unknown.is_err());
    }
}
