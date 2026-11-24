//! The adapters behind the fast bridge monitor's ports, and the one place that chooses between them.

mod http;
mod local;
pub mod memory;
mod wire;

use super::{config::FastBridgeConfig, ports::Ports};
use std::sync::Arc;

/// The real thing: the provider on this machine, the public board, and an RPC only when one is configured.
pub fn create_ports(cfg: &FastBridgeConfig) -> Ports {
    let client = http::create_http_client();
    Ports {
        provider: Arc::new(http::create_http_provider(client.clone(), &cfg.lp_url)),
        board: Arc::new(http::create_http_board(client.clone(), &cfg.boards)),
        swaps: Arc::new(local::create_file_swaps(&cfg.data_dir)),
        gas: (!cfg.eth_rpc.is_empty()).then(|| Arc::new(http::create_rpc_gas(client, &cfg.eth_rpc)) as _),
        log: Box::new(local::create_command_log(&cfg.log_cmd)),
    }
}
