//! What every monitor's ports are made of.

use super::model::{ChainTx, Coin, Outlook, PoolLock};
use anyhow::Result;
use std::{future::Future, pin::Pin};

/// How many entries or events may wait between a reader and the screen. A reader that fills it waits: memory does
/// not grow with how fast a source produces.
pub const QUEUE: usize = 1024;

/// Boxed so the ports stay object-safe: the adapter behind one is chosen at run time.
pub type Fut<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// The Bitcoin chain, through an explorer. Optional: asking sends the address or the transaction id to whoever answers.
pub trait Chain: Send + Sync {
    fn unspent<'a>(&'a self, address: &'a str) -> Fut<'a, Vec<Coin>>;
    /// The address's latest transactions, unconfirmed ones included. Not all of them: an explorer lists the recent ones.
    fn history<'a>(&'a self, address: &'a str) -> Fut<'a, Vec<ChainTx>>;
    /// One transaction by id. `None`: the explorer answers and does not know it.
    fn tx<'a>(&'a self, txid: &'a str) -> Fut<'a, Option<ChainTx>>;
    /// Whether an output of a transaction has been spent.
    fn spent<'a>(&'a self, txid: &'a str, vout: u32) -> Fut<'a, bool>;
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
