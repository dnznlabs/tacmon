//! What the fast bridge monitor needs from the outside, one trait per capability. The feed and the state
//! depend on these and never on an adapter: a new transport is a new file in `adapters/` and nothing else.

use super::model::{Ad, Health, LogEntry, Swap, Terms, Want};
use anyhow::Result;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::mpsc::UnboundedReceiver;

/// Boxed so the ports stay object-safe: the adapter behind one is chosen at run time.
pub type Fut<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// The provider itself: what it will take, and whether it is well.
pub trait Provider: Send + Sync {
    fn terms(&self) -> Fut<'_, Terms>;
    fn health(&self) -> Fut<'_, Health>;
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

/// The provider's log, oldest first, then live. The stream ends when the adapter can give no more.
pub trait LogFeed: Send {
    /// Called once, inside the runtime.
    fn start(self: Box<Self>) -> UnboundedReceiver<LogEntry>;
}

pub struct Ports {
    pub provider: Arc<dyn Provider>,
    pub board: Arc<dyn Board>,
    pub swaps: Arc<dyn SwapStore>,
    /// `None`: no ETH balance is read or shown.
    pub gas: Option<Arc<dyn GasBalance>>,
    pub log: Box<dyn LogFeed>,
}
