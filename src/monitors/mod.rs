//! The monitors this build knows. Adding one is a directory here, a section in `Config`, and a line below.

pub mod fast_bridge;

use crate::{config::Config, shell::monitor::Monitor};

/// `demo` runs every monitor on its in-memory adapters: nothing is read from this machine or the network.
pub fn create_monitors(cfg: &Config, demo: bool) -> Vec<Box<dyn Monitor>> {
    let mut out: Vec<Box<dyn Monitor>> = Vec::new();
    if cfg.fast_bridge.enabled {
        out.push(Box::new(fast_bridge::create_fast_bridge(&cfg.fast_bridge, demo)));
    }
    out
}
