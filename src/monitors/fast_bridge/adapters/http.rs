//! The provider's API, the board and an Ethereum RPC, over HTTP.

use super::wire;
use crate::monitors::fast_bridge::{
    model::{Ad, Health, Terms, Want},
    ports::{Board, Fut, GasBalance, Provider},
};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::time::Duration;

pub fn create_http_client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(8)).user_agent("tacmon").build().expect("http client")
}

/// A JSON body whatever the status: `/health` answers 503 with the error in the body.
async fn get(http: &reqwest::Client, url: &str) -> Result<Value> {
    Ok(http.get(url).send().await?.json().await?)
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
        Box::pin(async move { Ok(wire::terms(&get(&self.http, &format!("{}/info", self.base)).await?)) })
    }
    fn health(&self) -> Fut<'_, Health> {
        Box::pin(async move { Ok(wire::health(&get(&self.http, &format!("{}/health", self.base)).await?)) })
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
        Box::pin(async move { Ok(self.list("ads").await?.iter().map(wire::ad).collect()) })
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
            let v: Value = self.http.post(&self.rpc).json(&body).send().await?.json().await?;
            let hex = v.get("result").and_then(Value::as_str).context("the RPC answered without a result")?;
            Ok(u128::from_str_radix(hex.trim_start_matches("0x"), 16)? as f64 / 1e18)
        })
    }
}
