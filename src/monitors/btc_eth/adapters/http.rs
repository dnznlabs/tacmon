//! The provider's API, the board and an Ethereum RPC, over HTTP. The Bitcoin explorer is the shared one.

use super::wire;
pub use crate::monitors::common::http::{create_http_chain, create_http_client, create_rpc_pool_log};
use crate::monitors::{
    btc_eth::{
        model::{Ad, Health, Info, ReverseSide},
        ports::{Board, Fut, GasBalance, Provider},
    },
    common::http::{board_list, eth_balance, get, get_or_error_body, json_body, succeeded},
};
use anyhow::Context;

pub struct HttpProvider {
    http: reqwest::Client,
    base: String,
}

pub fn create_http_provider(http: reqwest::Client, base: &str) -> HttpProvider {
    HttpProvider { http, base: base.trim_end_matches('/').to_string() }
}

impl Provider for HttpProvider {
    fn info(&self) -> Fut<'_, Info> {
        Box::pin(async move { wire::info(&get(&self.http, &format!("{}/info", self.base)).await?).context("/info answered without terms") })
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
        Box::pin(async move {
            // the account comes from the log or the config, and goes into a request body
            let account = wire::eth_address(account).context("not an Ethereum address")?;
            eth_balance(&self.http, &self.rpc, &account).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::common::http::testing::serve_once;
    use std::time::Duration;
    use tokio::{net::TcpListener, time::timeout};

    const INFO: &str = r#"{"minAmount":"50000","maxAmount":"100000","feeBps":30,"flatFee":"1000","available":"90000","skewBps":4,"stats":{"swaps":2}}"#;

    #[tokio::test]
    async fn info_carries_the_terms_and_the_books_and_a_failed_status_is_a_failed_read() {
        let (answering, failing) = (serve_once("200 OK".into(), INFO).await, serve_once("503 Service Unavailable".into(), INFO).await);

        let (read, failed) =
            (create_http_provider(create_http_client(), &answering).info().await, create_http_provider(create_http_client(), &failing).info().await);

        let info = read.expect("an answer");
        assert_eq!((info.terms.max, info.terms.available, info.books.map(|b| (b.swaps, b.lean_bps))), (100_000, Some(90_000), Some((2, Some(4)))));
        assert!(failed.is_err());
    }

    #[tokio::test]
    async fn a_redirect_is_never_followed_or_read_as_terms_or_as_health() {
        let elsewhere = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let head = format!("307 Temporary Redirect\r\nLocation: http://{}/info", elsewhere.local_addr().expect("an address"));
        let (info, health) = (serve_once(head.clone(), INFO).await, serve_once(head, r#"{"ok":true}"#).await);

        let (terms, well) =
            (create_http_provider(create_http_client(), &info).info().await, create_http_provider(create_http_client(), &health).health().await);

        assert!(terms.is_err() && well.is_err());
        assert!(timeout(Duration::from_millis(200), elsewhere.accept()).await.is_err(), "the redirect target was contacted");
    }

    #[tokio::test]
    async fn health_is_read_from_a_failed_status_and_is_never_well_under_one() {
        let said = serve_once("503 Service Unavailable".into(), r#"{"ok":false,"error":"tick failed"}"#).await;
        let lying = serve_once("500 Internal Server Error".into(), r#"{"ok":true}"#).await;

        let (said, lying) =
            (create_http_provider(create_http_client(), &said).health().await, create_http_provider(create_http_client(), &lying).health().await);

        assert_eq!(said.expect("an answer"), Health { ok: false, error: "tick failed".into() });
        assert_eq!(lying.expect("an answer"), Health { ok: false, error: "/health answered an error status".into() });
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
    async fn an_account_that_is_not_one_is_never_sent_to_the_rpc() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let rpc = format!("http://{}", listener.local_addr().expect("an address"));

        let read = create_rpc_gas(create_http_client(), &rpc).eth(r#"0x","latest"],"x":["#).await;

        assert!(read.is_err());
        assert!(timeout(Duration::from_millis(200), listener.accept()).await.is_err(), "the RPC was asked");
    }
}
