use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BtcEthConfig {
    /// Off until set: a monitor needs a service of the operator's to watch, and reads nothing before it is asked to.
    pub enabled: bool,
    /// The provider's local HTTP API (`/info`, `/health`, `/reverse/info`).
    pub lp_url: String,
    /// Boards to read the providers of the pair (`/ads`) from.
    pub boards: Vec<String>,
    /// The provider's `dataDir`. Swap records are read from `swaps/` and `reverse/swaps/`, never written, and
    /// nothing else in it is opened: the key file beside them is never read.
    pub data_dir: PathBuf,
    /// Command whose stdout is the provider's JSON log, one object per line.
    pub log_cmd: Vec<String>,
    /// Ethereum JSON-RPC URL for the pool account's own ETH balance, which pays its gas. Empty: not read, and the
    /// account is sent nowhere.
    pub eth_rpc: String,
    /// The pool contract's address. With `eth_rpc` set too, its log is read for the locks this provider's recent
    /// swaps put into the pool. Empty: not read.
    pub pool_contract: String,
    /// The site an Ethereum transaction's page is on (`https://etherscan.io`), for the link the Tx page copies.
    /// Nothing is ever requested from it. Empty: the hash is copied bare.
    pub eth_explorer: String,
    /// A Bitcoin explorer's API (Esplora: mempool.space/api, blockstream.info/api) for the status of the swaps'
    /// transactions and of the provider's addresses. Empty: not asked, and the Transactions page lists what the swap
    /// records name without a status. Asking sends the provider's addresses and transaction ids to whoever answers.
    pub btc_explorer: String,
    /// The pool account, when it should not be learned from the log.
    pub pool_account: String,
    /// The reverse side's Bitcoin address, for before that side is on: the log names it once it is, and until then
    /// only a tool with the key can (Tacit's setup prints it). The address to fund to turn ETH→BTC on.
    pub reverse_address: String,
    /// Fees (hundredths of a percent) compared on the Advisor page, beside the provider's own.
    pub fee_ladder_bps: Vec<u64>,
    /// A swap's deadline turns red when it is closer than this.
    pub deadline_warn_secs: u64,
    /// A Bitcoin transaction unconfirmed for longer than this is marked as stuck.
    pub tx_stuck_secs: u64,
    pub refresh: Refresh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Refresh {
    pub lp_secs: u64,
    pub board_secs: u64,
    pub files_secs: u64,
    pub eth_secs: u64,
    /// The explorer is never asked more often than every 30 s, whatever this says.
    pub chain_secs: u64,
}

impl Default for BtcEthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            lp_url: "http://127.0.0.1:8788".into(),
            boards: vec!["https://tacit-fast-board.onrender.com".into()],
            data_dir: "/var/lib/tacit-fastbridge-lp-btc".into(),
            log_cmd: ["journalctl", "-u", "tacit-fastbridge-lp-btc", "-o", "cat", "-f", "-n", "2000"].map(String::from).to_vec(),
            eth_rpc: String::new(),
            pool_contract: String::new(),
            eth_explorer: String::new(),
            btc_explorer: String::new(),
            pool_account: String::new(),
            reverse_address: String::new(),
            fee_ladder_bps: vec![5, 10, 20, 30, 50, 100, 150, 200],
            deadline_warn_secs: 3600,
            tx_stuck_secs: 1800,
            refresh: Refresh::default(),
        }
    }
}

impl Default for Refresh {
    fn default() -> Self {
        Self { lp_secs: 5, board_secs: 20, files_secs: 5, eth_secs: 60, chain_secs: 120 }
    }
}
