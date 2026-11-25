//! The monitors this build knows. Adding one is a directory here, a section in `Config`, and a line below.

pub mod fast_bridge;

use crate::{config::Config, shell::monitor::Monitor};

/// Every monitor as (its name, its section in the configuration, what it watches), for the help shown when none is on.
pub const KNOWN: &[(&str, &str, &str)] = &[("fast-bridge", "fast_bridge", "a fast bridge liquidity provider on this machine")];

/// The monitors the configuration turns on: each is off until its section says `enabled = true`. `demo` runs every
/// monitor, enabled or not, on its in-memory adapters: nothing is read from this machine or the network.
pub fn create_monitors(cfg: &Config, demo: bool) -> Vec<Box<dyn Monitor>> {
    let mut out: Vec<Box<dyn Monitor>> = Vec::new();
    if demo || cfg.fast_bridge.enabled {
        out.push(Box::new(fast_bridge::create_fast_bridge(&cfg.fast_bridge, demo)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_monitor_runs_until_one_is_enabled_and_the_demo_runs_them_all() {
        let (unset, mut on) = (Config::default(), Config::default());
        on.fast_bridge.enabled = true;

        let (off, demo, enabled) = (create_monitors(&unset, false).len(), create_monitors(&unset, true).len(), create_monitors(&on, false).len());

        assert_eq!((off, demo, enabled), (0, KNOWN.len(), 1));
    }
}
