//! The provider's API, the board, an Ethereum RPC and a Bitcoin explorer, over HTTP.

use super::wire;
use crate::monitors::fast_bridge::{
    model::{Ad, ChainTx, Coin, Health, Outlook, PoolLock, ReverseSide, Terms, Want},
    ports::{Board, Chain, Fut, GasBalance, PoolLog, Provider},
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::time::Duration;

/// The most one answer may hold. The board's lists are a few kilobytes. Parsed JSON takes many times its bytes in
/// memory, so this is kept small: it is the bound on memory, not only on the read.
const MAX_BODY: usize = 256 << 10;

/// The most entries taken from one board list. The public board holds tens; past this it is a flood, not demand.
const MAX_ITEMS: usize = 5_000;

/// The most an address's transaction list may hold. An explorer lists up to 75, and one that moves a Tacit note is
/// about 6 KiB with its witness, so the list does not fit in `MAX_BODY`.
const MAX_HISTORY: usize = 1 << 20;

/// No redirects: a 3xx would send a request, and the pool account in it, to a host the operator never named.
pub fn create_http_client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(8)).redirect(reqwest::redirect::Policy::none()).user_agent("tacmon").build().expect("http client")
}

fn keep(body: &mut Vec<u8>, chunk: &[u8], max: usize) -> Result<()> {
    if body.len() + chunk.len() > max {
        bail!("the answer is over {max} bytes");
    }
    body.extend_from_slice(chunk);
    Ok(())
}

/// The JSON body, read in pieces so one that never ends is dropped at `max` and not held in memory.
async fn json_within(mut resp: reqwest::Response, max: usize) -> Result<Value> {
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        keep(&mut body, &chunk, max)?;
    }
    Ok(serde_json::from_slice(&body)?)
}

async fn json_body(resp: reqwest::Response) -> Result<Value> {
    json_within(resp, MAX_BODY).await
}

/// Only a 2xx is an answer. A redirect is not followed, and the body that comes with one is not read as the answer either.
fn succeeded(resp: reqwest::Response) -> Result<reqwest::Response> {
    if !resp.status().is_success() {
        bail!("answered {}", resp.status());
    }
    Ok(resp)
}

/// A JSON body from an answer that succeeded.
async fn get(http: &reqwest::Client, url: &str) -> Result<Value> {
    json_body(succeeded(http.get(url).send().await?)?).await
}

/// A JSON body from an error status too, with whether the status was a success: `/health` answers 503 with the
/// error in the body. Never from a redirect.
async fn get_or_error_body(http: &reqwest::Client, url: &str) -> Result<(bool, Value)> {
    let resp = http.get(url).send().await?;
    let status = resp.status();
    if status.is_redirection() || status.is_informational() {
        bail!("answered {status}");
    }
    Ok((status.is_success(), json_body(resp).await?))
}

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

impl HttpBoard {
    /// Every board is read and the lists are joined. One board failing fails the read: the lists of the rest would be
    /// drawn as the whole market, with nothing to say a part is missing.
    async fn list(&self, path: &str) -> Result<Vec<Value>> {
        if self.boards.is_empty() {
            bail!("no board is configured");
        }
        let mut out = Vec::new();
        for b in &self.boards {
            let mut v = get(&self.http, &format!("{b}/{path}")).await.with_context(|| format!("{b}/{path}"))?;
            let Some(Value::Array(items)) = v.get_mut(path).map(Value::take) else { bail!("{b}/{path} answered without a list") };
            // a flooded list is not read at all, for the same reason
            if items.len() > MAX_ITEMS {
                bail!("{b}/{path} lists more than {MAX_ITEMS}");
            }
            out.extend(items);
        }
        Ok(out)
    }
}

impl Board for HttpBoard {
    fn wants(&self) -> Fut<'_, Vec<Want>> {
        Box::pin(async move { Ok(self.list("wants").await?.iter().map(wire::want).collect()) })
    }
    fn ads(&self) -> Fut<'_, Vec<Ad>> {
        Box::pin(async move { Ok(self.list("ads").await?.iter().filter_map(wire::ad).collect()) })
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
            let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "eth_getBalance", "params": [account, "latest"] });
            let v = json_body(succeeded(self.http.post(&self.rpc).json(&body).send().await?)?).await?;
            let hex = v.get("result").and_then(Value::as_str).context("the RPC answered without a result")?;
            Ok(u128::from_str_radix(hex.trim_start_matches("0x"), 16)? as f64 / 1e18)
        })
    }
}

/// One JSON-RPC call's result. An answer that carries an error, or no result, is a failed read.
async fn rpc_call(http: &reqwest::Client, rpc: &str, method: &str, params: Value, max: usize) -> Result<Value> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let mut v = json_within(succeeded(http.post(rpc).json(&body).send().await?)?, max).await?;
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
        bail!("the RPC refused: {}", e.get("message").and_then(Value::as_str).unwrap_or("no reason given").chars().take(120).collect::<String>());
    }
    v.get_mut("result").map(Value::take).filter(|r| !r.is_null()).context("the RPC answered without a result")
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
        Box::pin(async move { wire::quantity(&rpc_call(&self.http, &self.rpc, "eth_blockNumber", json!([]), MAX_BODY).await?).context("not a block number") })
    }
    fn locks(&self, from: u64, to: u64) -> Fut<'_, Vec<PoolLock>> {
        Box::pin(async move {
            let filter =
                json!([{ "address": self.pool, "topics": [wire::LOCK_LEAVES_TOPIC], "fromBlock": format!("{from:#x}"), "toBlock": format!("{to:#x}") }]);
            // a day of the pool's locks is some 20 KiB: an answer over the bound fails, since part of a log is not the log
            let logs = rpc_call(&self.http, &self.rpc, "eth_getLogs", filter, MAX_HISTORY).await?;
            wire::pool_locks(&logs, &self.pool).context("the RPC answered without a list")
        })
    }
}

pub struct HttpChain {
    http: reqwest::Client,
    base: String,
}

pub fn create_http_chain(http: reqwest::Client, base: &str) -> HttpChain {
    HttpChain { http, base: base.trim_end_matches('/').to_string() }
}

impl Chain for HttpChain {
    fn unspent<'a>(&'a self, address: &'a str) -> Fut<'a, Vec<Coin>> {
        Box::pin(async move {
            // the address comes from the log, and goes into a URL path
            let address = wire::btc_address(address).context("not a Bitcoin address")?;
            // an address with more outputs than one answer may hold fails here: part of a list is not the list
            wire::coins(&get(&self.http, &format!("{}/address/{address}/utxo", self.base)).await?).context("the explorer answered without a list")
        })
    }
    fn history<'a>(&'a self, address: &'a str) -> Fut<'a, Vec<ChainTx>> {
        Box::pin(async move {
            let address = wire::btc_address(address).context("not a Bitcoin address")?;
            let resp = succeeded(self.http.get(format!("{}/address/{address}/txs", self.base)).send().await?)?;
            wire::history(&json_within(resp, MAX_HISTORY).await?, &address).context("the explorer answered without a list")
        })
    }
    fn tx<'a>(&'a self, txid: &'a str) -> Fut<'a, Option<ChainTx>> {
        Box::pin(async move {
            // the id comes from a swap record, which carries what a counterparty supplied, and goes into a URL path
            let id = wire::txid_text(txid).context("not a transaction id")?;
            let resp = self.http.get(format!("{}/tx/{id}", self.base)).send().await?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            let tx = wire::chain_tx(&json_body(succeeded(resp)?).await?, None).context("the explorer answered without a transaction")?;
            // an answer about another transaction is not an answer about this one
            if tx.txid != id {
                bail!("the explorer answered with another transaction");
            }
            Ok(Some(tx))
        })
    }
    fn spent<'a>(&'a self, txid: &'a str, vout: u32) -> Fut<'a, bool> {
        Box::pin(async move {
            let id = wire::txid_text(txid).context("not a transaction id")?;
            let v = get(&self.http, &format!("{}/tx/{id}/outspend/{vout}", self.base)).await?;
            v.get("spent").and_then(Value::as_bool).context("the explorer answered without saying")
        })
    }
    fn outlook(&self) -> Fut<'_, Outlook> {
        Box::pin(async move {
            let (tip, fees) = (format!("{}/blocks/tip/height", self.base), format!("{}/fee-estimates", self.base));
            let (tip, fees) = tokio::try_join!(get(&self.http, &tip), get(&self.http, &fees))?;
            wire::outlook(&tip, &fees).context("the explorer answered without a height or a fee estimate")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::timeout,
    };

    /// Answers one request with `head` and `body`, and gives the address to ask.
    async fn serve_once(head: String, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let addr = listener.local_addr().expect("an address");
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.expect("a request");
            let _ = sock.read(&mut [0; 2048]).await;
            let _ = sock.write_all(format!("HTTP/1.1 {head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
        });
        format!("http://{addr}")
    }

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
    async fn an_address_that_is_not_one_is_never_put_in_a_url() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let base = format!("http://{}", listener.local_addr().expect("an address"));

        let read = create_http_chain(create_http_client(), &base).unspent("../../tx/push?x=").await;

        assert!(read.is_err());
        assert!(timeout(Duration::from_millis(200), listener.accept()).await.is_err(), "the explorer was asked");
    }

    #[tokio::test]
    async fn an_id_that_is_not_one_is_never_put_in_a_url() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let base = format!("http://{}", listener.local_addr().expect("an address"));

        let read = create_http_chain(create_http_client(), &base).tx("../address/x/txs").await;

        assert!(read.is_err());
        assert!(timeout(Duration::from_millis(200), listener.accept()).await.is_err(), "the explorer was asked");
    }

    #[tokio::test]
    async fn a_transaction_the_explorer_does_not_know_is_not_a_failed_read_and_another_one_is_not_the_answer() {
        let id = "ab".repeat(32);
        let other: &'static str = Box::leak(format!(r#"{{"txid":"{}","fee":1,"weight":4,"status":{{"confirmed":true}}}}"#, "cd".repeat(32)).into_boxed_str());
        let (unknown, wrong, failing) = (
            serve_once("404 Not Found".into(), "Transaction not found").await,
            serve_once("200 OK".into(), other).await,
            serve_once("500 Oops".into(), "{}").await,
        );
        let ask = |base: String| {
            let id = id.clone();
            async move { create_http_chain(create_http_client(), &base).tx(&id).await }
        };

        let (unknown, wrong, failing) = (ask(unknown).await, ask(wrong).await, ask(failing).await);

        assert_eq!(unknown.ok(), Some(None));
        assert!(wrong.is_err() && failing.is_err());
    }

    #[test]
    fn a_body_over_the_limit_is_refused_before_it_is_kept() {
        let mut body = vec![0; MAX_BODY - 1];

        let (fits, over) = (keep(&mut body, &[0], MAX_BODY).is_ok(), keep(&mut body, &[0], MAX_BODY).is_err());

        assert_eq!((fits, over, body.len()), (true, true, MAX_BODY));
    }

    #[tokio::test]
    async fn an_rpc_that_refuses_is_a_failed_read_and_a_pool_that_is_not_an_address_is_never_asked_about() {
        let refused = serve_once("200 OK".into(), r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"range too wide"},"result":[]}"#).await;
        let answered = serve_once("200 OK".into(), r#"{"jsonrpc":"2.0","id":1,"result":"0x2a"}"#).await;
        let pool = format!("0x{}", "ab".repeat(20));

        let locks = create_rpc_pool_log(create_http_client(), &refused, &pool).expect("an adapter").locks(1, 2).await;
        let tip = create_rpc_pool_log(create_http_client(), &answered, &pool).expect("an adapter").tip().await;
        let unnamed = create_rpc_pool_log(create_http_client(), &answered, "the pool");

        assert!(locks.is_err_and(|e| e.to_string().contains("range too wide")));
        assert_eq!((tip.ok(), unnamed.is_none()), (Some(42), true));
    }
}
