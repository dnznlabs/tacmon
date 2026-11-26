//! What every monitor's ports are made of.

use super::model::{ChainTx, Coin, Outlook};
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
