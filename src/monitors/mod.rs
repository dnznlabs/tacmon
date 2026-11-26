//! The monitors this build knows. Adding one is a directory here, a section in `Config`, and a line below.

pub mod btc_eth;
pub mod common;
pub mod fast_bridge;

use crate::{config::Config, shell::monitor::Monitor};

/// Every monitor as (its name, its section in the configuration, what it watches), for the help shown when none is on.
pub const KNOWN: &[(&str, &str, &str)] = &[
    ("fast-bridge", "fast_bridge", "a fast bridge liquidity provider on this machine"),
    ("btc-eth", "btc_eth", "a provider of Bitcoin for ETH on this machine"),
];

/// The monitors the configuration turns on: each is off until its section says `enabled = true`. `demo` runs every
/// monitor, enabled or not, on its in-memory adapters: nothing is read from this machine or the network.
pub fn create_monitors(cfg: &Config, demo: bool) -> Vec<Box<dyn Monitor>> {
    let mut out: Vec<Box<dyn Monitor>> = Vec::new();
    if demo || cfg.fast_bridge.enabled {
        out.push(Box::new(fast_bridge::create_fast_bridge(&cfg.fast_bridge, demo)));
    }
    if demo || cfg.btc_eth.enabled {
        out.push(Box::new(btc_eth::create_btc_eth(&cfg.btc_eth, demo)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_monitor_runs_until_one_is_enabled_and_the_demo_runs_them_all() {
        let (unset, mut on) = (Config::default(), Config::default());
        on.btc_eth.enabled = true;

        let (off, demo, enabled) = (create_monitors(&unset, false), create_monitors(&unset, true), create_monitors(&on, false));

        assert_eq!((off.len(), demo.len(), enabled.iter().map(|m| m.id()).collect::<Vec<_>>()), (0, KNOWN.len(), vec!["btc-eth"]));
        // every monitor is in the help that says how to turn one on, under the name it answers to
        assert_eq!(demo.iter().map(|m| m.id()).collect::<Vec<_>>(), KNOWN.iter().map(|k| k.0).collect::<Vec<_>>());
    }
}
