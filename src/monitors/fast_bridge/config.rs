use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FastBridgeConfig {
    /// Off until set: a monitor needs a service of the operator's to watch, and reads nothing before it is asked to.
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
    /// The pool contract's address. With `eth_rpc` set too, its log is read for the locks this provider's recent
    /// swaps put into the pool. Empty: not read.
    pub pool_contract: String,
    /// The site an Ethereum transaction's page is on (`https://etherscan.io`), for the link the Transactions page
    /// copies. Nothing is ever requested from it. Empty: the hash is copied bare.
    pub eth_explorer: String,
    /// An Esplora API (`https://mempool.space/api`) to read the coins and transactions at the provider's Bitcoin
    /// addresses, and the status of its recent swaps' transactions, from. Empty: not read, and neither the addresses
    /// nor the transaction ids are sent anywhere.
    pub btc_explorer: String,
    /// The reverse side's Bitcoin address, for before the side is on: the log names it once it is.
    pub reverse_address: String,
    /// A command that prints what is at the reverse address, read with the provider's key:
    /// `["/path/to/tacmon/scripts/helpers/fast-bridge", "status"]`. Empty: not run. tacmon reads its output, never the key.
    pub status_cmd: Vec<String>,
    /// A command that says, with the provider's key, whether a transaction paid the reverse key silently:
    /// `["/path/to/tacmon/scripts/helpers/fast-bridge", "normalize"]`. tacmon adds `--txid <id>` and nothing else, so
    /// it is the tool's dry run and sends nothing. Run once for each note that left the main address for an address
    /// that is neither of the provider's. Empty: not run, and such a send is never flagged.
    pub normalize_cmd: Vec<String>,
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
    pub chain_secs: u64,
    pub status_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Advisor {
    /// Fees (hundredths of a percent) compared on the Advisor page.
    pub fee_ladder_bps: Vec<u64>,
    /// A swap's deadline turns red when it is closer than this.
    pub deadline_warn_secs: u64,
    /// A Bitcoin transaction unconfirmed for longer than this is marked as stuck.
    pub tx_stuck_secs: u64,
}

impl Default for FastBridgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            lp_url: "http://127.0.0.1:8787".into(),
            boards: vec!["https://tacit-fast-board.onrender.com".into()],
            data_dir: "/var/lib/tacit-fastbridge-lp".into(),
            log_cmd: ["journalctl", "-u", "tacit-fastbridge-lp", "-o", "cat", "-f", "-n", "2000"].map(String::from).to_vec(),
            eth_rpc: String::new(),
            pool_account: String::new(),
            pool_contract: String::new(),
            eth_explorer: String::new(),
            btc_explorer: String::new(),
            reverse_address: String::new(),
            status_cmd: Vec::new(),
            normalize_cmd: Vec::new(),
            refresh: Refresh::default(),
            advisor: Advisor::default(),
        }
    }
}

impl Default for Refresh {
    fn default() -> Self {
        Self { lp_secs: 5, board_secs: 20, files_secs: 5, eth_secs: 60, chain_secs: 120, status_secs: 300 }
    }
}

impl Default for Advisor {
    fn default() -> Self {
        Self { fee_ladder_bps: vec![10, 30, 50, 100, 150, 200, 300, 500], deadline_warn_secs: 3600, tx_stuck_secs: 1800 }
    }
}
