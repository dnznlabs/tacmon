//! The provider's API, the board and an Ethereum RPC, over HTTP.

use super::wire;
use crate::monitors::fast_bridge::{
    model::{Ad, Health, Terms, Want},
    ports::{Board, Fut, GasBalance, Provider},
};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::time::Duration;

/// The most one answer may hold. The board's lists are a few kilobytes; this is room to grow, not a target.
const MAX_BODY: usize = 4 << 20;

/// No redirects: a 3xx would send a request, and the pool account in it, to a host the operator never named.
pub fn create_http_client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(8)).redirect(reqwest::redirect::Policy::none()).user_agent("tacmon").build().expect("http client")
}

fn keep(body: &mut Vec<u8>, chunk: &[u8]) -> Result<()> {
    if body.len() + chunk.len() > MAX_BODY {
        bail!("the answer is over {MAX_BODY} bytes");
    }
    body.extend_from_slice(chunk);
    Ok(())
}

/// The JSON body, read in pieces so one that never ends is dropped at `MAX_BODY` and not held in memory.
async fn json_body(mut resp: reqwest::Response) -> Result<Value> {
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        keep(&mut body, &chunk)?;
    }
    Ok(serde_json::from_slice(&body)?)
}

/// A JSON body from an answer that succeeded.
async fn get(http: &reqwest::Client, url: &str) -> Result<Value> {
    json_body(http.get(url).send().await?.error_for_status()?).await
}

/// A JSON body whatever the status: `/health` answers 503 with the error in the body.
async fn get_any_status(http: &reqwest::Client, url: &str) -> Result<Value> {
    json_body(http.get(url).send().await?).await
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
        Box::pin(async move { Ok(wire::health(&get_any_status(&self.http, &format!("{}/health", self.base)).await?)) })
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
    /// Every board is read and the lists are joined. One that fails leaves the others' answers standing; all failing is an error.
    async fn list(&self, path: &str) -> Result<Vec<Value>> {
        let mut out = None::<Vec<Value>>;
        for b in &self.boards {
            let Ok(v) = get(&self.http, &format!("{b}/{path}")).await else { continue };
            let Some(items) = v.get(path).and_then(Value::as_array) else { continue };
            out.get_or_insert_with(Vec::new).extend(items.iter().cloned());
        }
        out.ok_or_else(|| anyhow!("no board answered /{path}"))
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
            let v = json_body(self.http.post(&self.rpc).json(&body).send().await?.error_for_status()?).await?;
            let hex = v.get("result").and_then(Value::as_str).context("the RPC answered without a result")?;
            Ok(u128::from_str_radix(hex.trim_start_matches("0x"), 16)? as f64 / 1e18)
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
        let base = serve_once(format!("307 Temporary Redirect\r\nLocation: http://{target}/info"), "").await;

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

    #[test]
    fn a_body_over_the_limit_is_refused_before_it_is_kept() {
        let mut body = vec![0; MAX_BODY - 1];

        let (fits, over) = (keep(&mut body, &[0]).is_ok(), keep(&mut body, &[0]).is_err());

        assert_eq!((fits, over, body.len()), (true, true, MAX_BODY));
    }
}
