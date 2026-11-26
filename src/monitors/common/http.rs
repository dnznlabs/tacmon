//! HTTP as every monitor reads it: no redirects, only a 2xx is an answer, and no answer is held past its bound.

use super::{
    model::{ChainTx, Coin, Outlook},
    ports::{Chain, Fut},
    wire,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::time::Duration;

/// The most one answer may hold. The board's lists are a few kilobytes. Parsed JSON takes many times its bytes in
/// memory, so this is kept small: it is the bound on memory, not only on the read.
pub const MAX_BODY: usize = 256 << 10;

/// The most entries taken from one board list. The public board holds tens; past this it is a flood, not demand.
pub const MAX_ITEMS: usize = 5_000;

/// The most an address's transaction list, or a contract's log, may hold. An explorer lists up to 75 transactions,
/// and one that moves a Tacit note is about 6 KiB with its witness, so the list does not fit in `MAX_BODY`.
pub const MAX_HISTORY: usize = 1 << 20;

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
pub async fn json_within(mut resp: reqwest::Response, max: usize) -> Result<Value> {
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        keep(&mut body, &chunk, max)?;
    }
    Ok(serde_json::from_slice(&body)?)
}

pub async fn json_body(resp: reqwest::Response) -> Result<Value> {
    json_within(resp, MAX_BODY).await
}

/// Only a 2xx is an answer. A redirect is not followed, and the body that comes with one is not read as the answer either.
pub fn succeeded(resp: reqwest::Response) -> Result<reqwest::Response> {
    if !resp.status().is_success() {
        bail!("answered {}", resp.status());
    }
    Ok(resp)
}

/// A JSON body from an answer that succeeded.
pub async fn get(http: &reqwest::Client, url: &str) -> Result<Value> {
    json_body(succeeded(http.get(url).send().await?)?).await
}

/// A JSON body from an error status too, with whether the status was a success: `/health` answers 503 with the
/// error in the body. Never from a redirect.
pub async fn get_or_error_body(http: &reqwest::Client, url: &str) -> Result<(bool, Value)> {
    let resp = http.get(url).send().await?;
    let status = resp.status();
    if status.is_redirection() || status.is_informational() {
        bail!("answered {status}");
    }
    Ok((status.is_success(), json_body(resp).await?))
}

/// The list every board in `boards` keeps under `path` ("ads", "wants"), joined. One board failing fails the read:
/// the lists of the rest would be drawn as the whole market, with nothing to say a part is missing.
pub async fn board_list(http: &reqwest::Client, boards: &[String], path: &str) -> Result<Vec<Value>> {
    if boards.is_empty() {
        bail!("no board is configured");
    }
    let mut out = Vec::new();
    for b in boards {
        let mut v = get(http, &format!("{b}/{path}")).await.with_context(|| format!("{b}/{path}"))?;
        let Some(Value::Array(items)) = v.get_mut(path).map(Value::take) else { bail!("{b}/{path} answered without a list") };
        // a flooded list is not read at all, for the same reason
        if items.len() > MAX_ITEMS {
            bail!("{b}/{path} lists more than {MAX_ITEMS}");
        }
        out.extend(items);
    }
    Ok(out)
}

/// The ETH an account holds, as an Ethereum node reports it.
pub async fn eth_balance(http: &reqwest::Client, rpc: &str, account: &str) -> Result<f64> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "eth_getBalance", "params": [account, "latest"] });
    let v = json_body(succeeded(http.post(rpc).json(&body).send().await?)?).await?;
    let hex = v.get("result").and_then(Value::as_str).context("the RPC answered without a result")?;
    Ok(u128::from_str_radix(hex.trim_start_matches("0x"), 16)? as f64 / 1e18)
}

/// One JSON-RPC call's result, or `None` when the node answered with an error of its own. Nodes refuse with a 200,
/// a 400 or a 403, so the body of a failed status is read for one; a redirect's never is.
pub async fn rpc_call(http: &reqwest::Client, rpc: &str, method: &str, params: Value, max: usize) -> Result<Option<Value>> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let resp = http.post(rpc).json(&body).send().await?;
    let status = resp.status();
    if status.is_redirection() {
        bail!("answered {status}");
    }
    let answer = json_within(resp, max).await;
    if answer.as_ref().is_ok_and(|v| v.get("error").is_some_and(|e| !e.is_null())) {
        return Ok(None);
    }
    if !status.is_success() {
        bail!("answered {status}");
    }
    Ok(Some(answer?.get_mut("result").map(Value::take).filter(|r| !r.is_null()).context("the RPC answered without a result")?))
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
pub mod testing {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    /// Answers one request with `head` and `body`, and gives the address to ask.
    pub async fn serve_once(head: String, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let addr = listener.local_addr().expect("an address");
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.expect("a request");
            let _ = sock.read(&mut [0; 2048]).await;
            let _ = sock.write_all(format!("HTTP/1.1 {head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
        });
        format!("http://{addr}")
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::serve_once, *};
    use tokio::{net::TcpListener, time::timeout};

    #[test]
    fn a_body_over_the_limit_is_refused_before_it_is_kept() {
        let mut body = vec![0; MAX_BODY - 1];

        let (fits, over) = (keep(&mut body, &[0], MAX_BODY).is_ok(), keep(&mut body, &[0], MAX_BODY).is_err());

        assert_eq!((fits, over, body.len()), (true, true, MAX_BODY));
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
}
