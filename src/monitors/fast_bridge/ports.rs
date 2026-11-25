//! What the fast bridge monitor needs from the outside, one trait per capability. The feed and the state
//! depend on these and never on an adapter: a new transport is a new file in `adapters/` and nothing else.

use super::model::{Ad, Coin, Health, LogEntry, ReverseSide, ReverseStock, Swap, Terms, Want};
use anyhow::Result;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::mpsc::Receiver;

/// How many entries or events may wait between a reader and the screen. A reader that fills it waits: memory does
/// not grow with how fast a source produces.
pub const QUEUE: usize = 1024;

/// Boxed so the ports stay object-safe: the adapter behind one is chosen at run time.
pub type Fut<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// The provider itself: what it will take, and whether it is well.
pub trait Provider: Send + Sync {
    fn terms(&self) -> Fut<'_, Terms>;
    fn health(&self) -> Fut<'_, Health>;
    /// The reverse side's terms, or that it is off.
    fn reverse(&self) -> Fut<'_, ReverseSide>;
}

/// Where providers advertise and unmet bridgers wait.
pub trait Board: Send + Sync {
    fn wants(&self) -> Fut<'_, Vec<Want>>;
    fn ads(&self) -> Fut<'_, Vec<Ad>>;
}

/// The provider's swap records.
pub trait SwapStore: Send + Sync {
    fn swaps(&self) -> Fut<'_, Vec<Swap>>;
}

/// The ETH an account holds, in ETH. Optional: asking sends the account to whoever answers.
pub trait GasBalance: Send + Sync {
    fn eth<'a>(&'a self, account: &'a str) -> Fut<'a, f64>;
}

/// What is unspent at a Bitcoin address. Optional: asking sends the address to whoever answers.
pub trait Chain: Send + Sync {
    fn unspent<'a>(&'a self, address: &'a str) -> Fut<'a, Vec<Coin>>;
}

/// What only the provider's key can read. Optional: behind it is a tool that holds the key, never this process.
pub trait Inventory: Send + Sync {
    fn reverse_stock(&self) -> Fut<'_, ReverseStock>;
}

/// The provider's log, oldest first, then live. The stream ends when the adapter can give no more.
pub trait LogFeed: Send {
    /// Called once, inside the runtime.
    fn start(self: Box<Self>) -> Receiver<LogEntry>;
}

pub struct Ports {
    pub provider: Arc<dyn Provider>,
    pub board: Arc<dyn Board>,
    pub swaps: Arc<dyn SwapStore>,
    /// `None`: no ETH balance is read or shown.
    pub gas: Option<Arc<dyn GasBalance>>,
    /// `None`: no address is looked up and the Maintenance page shows no coins.
    pub chain: Option<Arc<dyn Chain>>,
    /// `None`: what needs the key is not shown.
    pub inventory: Option<Arc<dyn Inventory>>,
    pub log: Box<dyn LogFeed>,
}
