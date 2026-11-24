use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FastBridgeConfig {
    pub enabled: bool,
    /// The provider's local HTTP API (`/info`, `/health`).
    pub lp_url: String,
    /// Boards to read waiting bridgers (`/wants`) and providers (`/ads`) from.
    pub boards: Vec<String>,
    /// The provider's `dataDir`. Swap records are read from `swaps/`, never written.
    pub data_dir: PathBuf,
    /// Command whose stdout is the provider's JSON log, one object per line.
    pub log_cmd: Vec<String>,
    /// Ethereum JSON-RPC URL for the pool account's ETH balance. Empty: not read, and the account is sent nowhere.
    pub eth_rpc: String,
    /// The pool account, when it should not be learned from the log.
    pub pool_account: String,
    pub refresh: Refresh,
    pub advisor: Advisor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Refresh {
    pub lp_secs: u64,
    pub board_secs: u64,
    pub files_secs: u64,
    pub eth_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Advisor {
    /// Fees (hundredths of a percent) compared on the Advisor page.
    pub fee_ladder_bps: Vec<u64>,
    /// A swap's deadline turns red when it is closer than this.
    pub deadline_warn_secs: u64,
}

impl Default for FastBridgeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            lp_url: "http://127.0.0.1:8787".into(),
            boards: vec!["https://tacit-fast-board.onrender.com".into()],
            data_dir: "/var/lib/tacit-fastbridge-lp".into(),
            log_cmd: ["journalctl", "-u", "tacit-fastbridge-lp", "-o", "cat", "-f", "-n", "2000"].map(String::from).to_vec(),
            eth_rpc: String::new(),
            pool_account: String::new(),
            refresh: Refresh::default(),
            advisor: Advisor::default(),
        }
    }
}

impl Default for Refresh {
    fn default() -> Self {
        Self { lp_secs: 5, board_secs: 20, files_secs: 5, eth_secs: 60 }
    }
}

impl Default for Advisor {
    fn default() -> Self {
        Self { fee_ladder_bps: vec![10, 30, 50, 100, 150, 200, 300, 500], deadline_warn_secs: 3600 }
    }
}
