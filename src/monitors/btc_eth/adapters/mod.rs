//! The adapters behind the BTC and ETH monitor's ports, and the one place that chooses between them.

mod http;
mod local;
pub mod memory;
mod wire;

use super::{config::BtcEthConfig, ports::Ports};
use std::sync::Arc;

/// The real thing: the provider on this machine, the public board, and an RPC or an explorer only when one is configured.
pub fn create_ports(cfg: &BtcEthConfig) -> Ports {
    let client = http::create_http_client();
    Ports {
        provider: Arc::new(http::create_http_provider(client.clone(), &cfg.lp_url)),
        board: Arc::new(http::create_http_board(client.clone(), &cfg.boards)),
        swaps: Arc::new(local::create_file_swaps(&cfg.data_dir)),
        gas: (!cfg.eth_rpc.is_empty()).then(|| Arc::new(http::create_rpc_gas(client.clone(), &cfg.eth_rpc)) as _),
        // both must be set: the RPC to ask, and the contract to ask about
        pool: (!cfg.eth_rpc.is_empty())
            .then(|| http::create_rpc_pool_log(client.clone(), &cfg.eth_rpc, &cfg.pool_contract))
            .flatten()
            .map(|p| Arc::new(p) as _),
        chain: (!cfg.btc_explorer.is_empty()).then(|| Arc::new(http::create_http_chain(client, &cfg.btc_explorer)) as _),
        log: Box::new(local::create_command_log(&cfg.log_cmd)),
    }
}
