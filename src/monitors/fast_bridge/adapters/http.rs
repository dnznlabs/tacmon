//! The provider's API, the board, an Ethereum RPC and a Bitcoin explorer, over HTTP.

use super::wire;
use crate::monitors::common::http::{MAX_BODY, MAX_HISTORY, board_list, eth_balance, get, get_or_error_body, json_body, rpc_call, succeeded};
pub use crate::monitors::common::http::{create_http_chain, create_http_client};
use crate::monitors::fast_bridge::{
    model::{Ad, Health, PoolLock, ReverseSide, Terms, Want},
    ports::{Board, Fut, GasBalance, PoolLog, Provider},
};
use anyhow::Context;
use serde_json::json;

pub struct HttpProvider {
    http: reqwest::Client,
    base: String,
}

pub fn create_http_provider(http: reqwest::Client, base: &str) -> HttpProvider {
    HttpProvider { http, base: base.trim_end_matches('/').to_string() }
}

impl Provider for HttpProvider {
    fn terms(&self) -> Fut<'_, Terms> {
        Box::pin(async move { wire::terms(&get(&self.http, &format!("{}/info", self.base)).await?).context("/info answered without terms") })
    }
    fn health(&self) -> Fut<'_, Health> {
        Box::pin(async move {
            let (succeeded, body) = get_or_error_body(&self.http, &format!("{}/health", self.base)).await?;
            let h = wire::health(&body);
            // a failed status is never healthy, whatever its body says
            Ok(Health { ok: h.ok && succeeded, error: if h.error.is_empty() && !succeeded { "/health answered an error status".into() } else { h.error } })
        })
    }
    fn reverse(&self) -> Fut<'_, ReverseSide> {
        Box::pin(async move {
            let resp = self.http.get(format!("{}/reverse/info", self.base)).send().await?;
            // the route is only there while the side is on; any other failure is a failed read, not "off"
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(ReverseSide::Off);
            }
            wire::reverse(&json_body(succeeded(resp)?).await?).context("/reverse/info answered without terms")
        })
    }
}

pub struct HttpBoard {
    http: reqwest::Client,
    boards: Vec<String>,
}

pub fn create_http_board(http: reqwest::Client, boards: &[String]) -> HttpBoard {
    HttpBoard { http, boards: boards.iter().map(|b| b.trim_end_matches('/').to_string()).collect() }
}

impl Board for HttpBoard {
    fn wants(&self) -> Fut<'_, Vec<Want>> {
        Box::pin(async move { Ok(board_list(&self.http, &self.boards, "wants").await?.iter().map(wire::want).collect()) })
    }
    fn ads(&self) -> Fut<'_, Vec<Ad>> {
        Box::pin(async move { Ok(board_list(&self.http, &self.boards, "ads").await?.iter().filter_map(wire::ad).collect()) })
    }
}

pub struct RpcGas {
    http: reqwest::Client,
    rpc: String,
}

pub fn create_rpc_gas(http: reqwest::Client, rpc: &str) -> RpcGas {
    RpcGas { http, rpc: rpc.to_string() }
}

impl GasBalance for RpcGas {
    fn eth<'a>(&'a self, account: &'a str) -> Fut<'a, f64> {
        Box::pin(eth_balance(&self.http, &self.rpc, account))
    }
}

pub struct RpcPoolLog {
    http: reqwest::Client,
    rpc: String,
    /// Lower case, checked when the adapter was made.
    pool: String,
}

/// `None` when `pool` is not an Ethereum address: nothing is asked about a contract that cannot be named.
pub fn create_rpc_pool_log(http: reqwest::Client, rpc: &str, pool: &str) -> Option<RpcPoolLog> {
    Some(RpcPoolLog { http, rpc: rpc.to_string(), pool: wire::eth_address(pool)? })
}

impl PoolLog for RpcPoolLog {
    fn tip(&self) -> Fut<'_, u64> {
        Box::pin(async move {
            let tip = rpc_call(&self.http, &self.rpc, "eth_blockNumber", json!([]), MAX_BODY).await?.context("the RPC refused")?;
            wire::quantity(&tip).context("not a block number")
        })
    }
    fn locks(&self, from: u64, to: u64) -> Fut<'_, Option<Vec<PoolLock>>> {
        Box::pin(async move {
            let filter =
                json!([{ "address": self.pool, "topics": [wire::LOCK_LEAVES_TOPIC], "fromBlock": format!("{from:#x}"), "toBlock": format!("{to:#x}") }]);
            // a day of the pool's locks is some 20 KiB: an answer over the bound fails, since part of a log is not the log
            let Some(logs) = rpc_call(&self.http, &self.rpc, "eth_getLogs", filter, MAX_HISTORY).await? else { return Ok(None) };
            Ok(Some(wire::pool_locks(&logs, &self.pool).context("the RPC answered without a list")?))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::common::http::{MAX_ITEMS, testing::serve_once};
    use std::time::Duration;
    use tokio::{net::TcpListener, time::timeout};

    #[tokio::test]
    async fn a_redirect_is_an_error_and_is_not_followed() {
        let elsewhere = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let target = elsewhere.local_addr().expect("an address");
        // the body is terms a reader would accept: only the status says this is not the answer
        let head = format!("307 Temporary Redirect\r\nLocation: http://{target}/info");
        let base = serve_once(head, r#"{"minAmount":"1","maxAmount":"2","feeBps":1}"#).await;

        let terms = create_http_provider(create_http_client(), &base).terms().await;

        assert!(terms.is_err());
        assert!(timeout(Duration::from_millis(200), elsewhere.accept()).await.is_err(), "the redirect target was contacted");
    }

    #[tokio::test]
    async fn a_failed_status_is_a_failed_read_not_zero_terms() {
        let base = serve_once("404 Not Found".into(), "{}").await;

        let terms = create_http_provider(create_http_client(), &base).terms().await;

        assert!(terms.is_err());
    }

    #[tokio::test]
    async fn health_is_read_from_the_body_of_a_failed_status() {
        let base = serve_once("503 Service Unavailable".into(), r#"{"ok":false,"error":"tick failed"}"#).await;

        let health = create_http_provider(create_http_client(), &base).health().await.expect("an answer");

        assert_eq!(health, Health { ok: false, error: "tick failed".into() });
    }

    #[tokio::test]
    async fn a_failed_status_is_never_healthy_whatever_its_body_says() {
        let base = serve_once("500 Internal Server Error".into(), r#"{"ok":true}"#).await;

        let health = create_http_provider(create_http_client(), &base).health().await.expect("an answer");

        assert_eq!(health, Health { ok: false, error: "/health answered an error status".into() });
    }

    #[tokio::test]
    async fn health_is_not_read_from_the_body_of_a_redirect() {
        let base = serve_once("302 Found\r\nLocation: http://127.0.0.1:1/".into(), r#"{"ok":true}"#).await;

        let health = create_http_provider(create_http_client(), &base).health().await;

        assert!(health.is_err());
    }

    #[tokio::test]
    async fn one_board_of_two_failing_is_a_failed_read_not_a_smaller_market() {
        let answering = serve_once("200 OK".into(), r#"{"wants":[{"amount":"5"}]}"#).await;
        let failing = serve_once("500 Internal Server Error".into(), "{}").await;

        let wants = create_http_board(create_http_client(), &[answering, failing]).wants().await;

        assert!(wants.is_err());
    }

    #[tokio::test]
    async fn a_flooded_board_list_is_a_failed_read() {
        let body: &'static str = Box::leak(format!(r#"{{"wants":[{}]}}"#, vec![r#"{"amount":"1"}"#; MAX_ITEMS + 1].join(",")).into_boxed_str());
        let base = serve_once("200 OK".into(), body).await;

        let wants = create_http_board(create_http_client(), &[base]).wants().await;

        assert!(wants.is_err());
    }

    #[tokio::test]
    async fn reverse_off_is_only_a_not_found_and_any_other_failure_is_a_failed_read() {
        let off = serve_once("404 Not Found".into(), r#"{"error":"not found"}"#).await;
        let failing = serve_once("500 Internal Server Error".into(), "{}").await;

        let (off, failing) =
            (create_http_provider(create_http_client(), &off).reverse().await, create_http_provider(create_http_client(), &failing).reverse().await);

        assert_eq!(off.ok(), Some(ReverseSide::Off));
        assert!(failing.is_err());
    }

    #[tokio::test]
    async fn a_node_that_refuses_is_told_from_one_that_fails_whatever_status_it_refuses_with() {
        let error = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"range too wide"},"result":[]}"#;
        let pool = format!("0x{}", "ab".repeat(20));
        let ask = |head: &'static str, body: &'static str| {
            let pool = pool.clone();
            async move { create_rpc_pool_log(create_http_client(), &serve_once(head.into(), body).await, &pool).expect("an adapter").locks(1, 2).await }
        };

        let (refused, forbidden, down, moved, read) = (
            ask("200 OK", error).await,
            ask("403 Forbidden", error).await,
            ask("502 Bad Gateway", "<html>").await,
            ask("307 Temporary Redirect\r\nLocation: http://127.0.0.1:9/", error).await,
            ask("200 OK", r#"{"jsonrpc":"2.0","id":1,"result":[]}"#).await,
        );

        assert!(matches!((refused, forbidden, read), (Ok(None), Ok(None), Ok(Some(l))) if l.is_empty()));
        assert!(down.is_err() && moved.is_err());
    }

    #[tokio::test]
    async fn the_tip_is_read_and_a_pool_that_is_not_an_address_is_never_asked_about() {
        let answered = serve_once("200 OK".into(), r#"{"jsonrpc":"2.0","id":1,"result":"0x2a"}"#).await;
        let pool = format!("0x{}", "ab".repeat(20));

        let tip = create_rpc_pool_log(create_http_client(), &answered, &pool).expect("an adapter").tip().await;
        let unnamed = create_rpc_pool_log(create_http_client(), &answered, "the pool");

        assert_eq!((tip.ok(), unnamed.is_none()), (Some(42), true));
    }
}
