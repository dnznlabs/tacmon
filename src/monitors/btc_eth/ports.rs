//! What the BTC and ETH monitor needs from the outside, one trait per capability. The feed and the state depend on
//! these and never on an adapter: a new transport is a new file in `adapters/` and nothing else.

use super::model::{Ad, Health, Info, LogEntry, ReverseSide, Swap};
pub use crate::monitors::common::ports::{Chain, Fut, QUEUE};
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

/// The provider itself: what it will take, what its books say, and whether it is well.
pub trait Provider: Send + Sync {
    fn info(&self) -> Fut<'_, Info>;
    fn health(&self) -> Fut<'_, Health>;
    /// The reverse side's terms, or that it is off.
    fn reverse(&self) -> Fut<'_, ReverseSide>;
}

/// Where providers advertise.
pub trait Board: Send + Sync {
    fn ads(&self) -> Fut<'_, Vec<Ad>>;
}

/// The provider's swap records, both directions.
pub trait SwapStore: Send + Sync {
    fn swaps(&self) -> Fut<'_, Vec<Swap>>;
}

/// The ETH an account holds outside the pool, in ETH: what pays the provider's own gas. Optional: asking sends the
/// account to whoever answers.
pub trait GasBalance: Send + Sync {
    fn eth<'a>(&'a self, account: &'a str) -> Fut<'a, f64>;
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
    /// `None`: no explorer is asked, and nothing of the provider's is sent to one.
    pub chain: Option<Arc<dyn Chain>>,
    pub log: Box<dyn LogFeed>,
}
