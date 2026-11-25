//! What the fast bridge monitor needs from the outside, one trait per capability. The feed and the state
//! depend on these and never on an adapter: a new transport is a new file in `adapters/` and nothing else.

use super::model::{Ad, ChainTx, Coin, Health, LogEntry, Outlook, PoolLock, ReverseSide, ReverseStock, Swap, Terms, Want};
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

/// The Bitcoin chain, through an explorer. Optional: asking sends the address or the transaction id to whoever answers.
pub trait Chain: Send + Sync {
    fn unspent<'a>(&'a self, address: &'a str) -> Fut<'a, Vec<Coin>>;
    /// The address's latest transactions, unconfirmed ones included. Not all of them: an explorer lists the recent ones.
    fn history<'a>(&'a self, address: &'a str) -> Fut<'a, Vec<ChainTx>>;
    /// One transaction by id. `None`: the explorer answers and does not know it.
    fn tx<'a>(&'a self, txid: &'a str) -> Fut<'a, Option<ChainTx>>;
    fn outlook(&self) -> Fut<'_, Outlook>;
}

/// The pool contract's log on Ethereum. Optional: asking sends the pool contract's address, which every provider
/// shares, and nothing of this provider's.
pub trait PoolLog: Send + Sync {
    /// The newest block's number.
    fn tip(&self) -> Fut<'_, u64>;
    /// Every lock put into the pool in blocks `from` to `to`, both included, oldest first. `None`: the node answered
    /// and refused, which for a log is nearly always the range. A request that got no answer is an error.
    fn locks(&self, from: u64, to: u64) -> Fut<'_, Option<Vec<PoolLock>>>;
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
    /// `None`: no address or transaction is looked up: the Maintenance page shows no coins, the Transactions page no status.
    pub chain: Option<Arc<dyn Chain>>,
    /// `None`: the pool's log is not read, and the Transactions page shows no Ethereum rows.
    pub pool: Option<Arc<dyn PoolLog>>,
    /// `None`: what needs the key is not shown.
    pub inventory: Option<Arc<dyn Inventory>>,
    pub log: Box<dyn LogFeed>,
}
