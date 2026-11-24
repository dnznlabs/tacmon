//! The provider's API, the board and an Ethereum RPC, over HTTP.

use super::wire;
use crate::monitors::fast_bridge::{
    model::{Ad, Health, Terms, Want},
    ports::{Board, Fut, GasBalance, Provider},
};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::time::Duration;

/// The most one answer may hold. The board's lists are a few kilobytes. Parsed JSON takes many times its bytes in
/// memory, so this is kept small: it is the bound on memory, not only on the read.
const MAX_BODY: usize = 256 << 10;

/// The most entries taken from one board list. The public board holds tens; past this it is a flood, not demand.
const MAX_ITEMS: usize = 5_000;

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
            let Ok(mut v) = get(&self.http, &format!("{b}/{path}")).await else { continue };
            // a flooded list is not read at all: part of it would be drawn as the whole market
            let Some(Value::Array(items)) = v.get_mut(path).map(Value::take).filter(|l| l.as_array().is_some_and(|l| l.len() <= MAX_ITEMS)) else { continue };
            out.get_or_insert_with(Vec::new).extend(items);
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
            let v = json_body(succeeded(self.http.post(&self.rpc).json(&body).send().await?)?).await?;
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
    async fn a_flooded_board_list_is_a_failed_read() {
        let body: &'static str = Box::leak(format!(r#"{{"wants":[{}]}}"#, vec![r#"{"amount":"1"}"#; MAX_ITEMS + 1].join(",")).into_boxed_str());
        let base = serve_once("200 OK".into(), body).await;

        let wants = create_http_board(create_http_client(), &[base]).wants().await;

        assert!(wants.is_err());
    }

    #[test]
    fn a_body_over_the_limit_is_refused_before_it_is_kept() {
        let mut body = vec![0; MAX_BODY - 1];

        let (fits, over) = (keep(&mut body, &[0]).is_ok(), keep(&mut body, &[0]).is_err());

        assert_eq!((fits, over, body.len()), (true, true, MAX_BODY));
    }
}
