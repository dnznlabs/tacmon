//! What a fast bridge provider is, as this monitor sees it. No I/O and no wire formats here.
//! Amounts are TAC base units (8 decimal places) unless a name says otherwise.

pub use crate::monitors::common::model::{
    Asset, ChainTx, Coin, DUST, Flag, HIGH_FEE_BPS, Health, Level, Outlook, PoolLock, PoolScan, Step, effective_bps, fee_on, flag, rate, total,
};

pub const UNIT: u128 = 100_000_000;
/// Which way a swap goes, as the bridger sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    /// TAC on Bitcoin in, pool TAC out.
    #[default]
    Forward,
    /// Pool TAC in, TAC on Bitcoin out, from the reverse side's own address.
    Reverse,
}

impl Direction {
    /// The happy path, in order. Anything else a record says is off it.
    pub fn phases(self) -> [&'static str; 6] {
        match self {
            Direction::Forward => ["accepted", "locking", "locked", "offered", "taking", "done"],
            Direction::Reverse => ["built", "broadcast", "offered", "claiming", "claimed", "done"],
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Direction::Forward => "BTC→POOL",
            Direction::Reverse => "POOL→BTC",
        }
    }
}

/// "1,234.56"
pub fn tac(u: u128) -> String {
    let whole = (u / UNIT).to_string();
    let cents = (u % UNIT) / (UNIT / 100);
    let mut out = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("{out}.{cents:02}")
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    /// "HH:MM:SS", UTC, as the provider writes it.
    pub time: String,
    /// Seconds since 1970 when the provider wrote it, where the line says.
    pub at: Option<u64>,
    pub level: Level,
    pub msg: String,
    pub detail: String,
    /// State the line carries, beyond being a line in the feed.
    pub signal: Option<Signal>,
}

impl LogEntry {
    /// A line that is not the provider's own (a reader's trouble, a stack trace).
    pub fn plain(level: Level, msg: impl Into<String>) -> Self {
        Self { time: "--:--:--".into(), at: None, level, msg: msg.into(), detail: String::new(), signal: None }
    }
}

#[derive(Debug, Clone)]
pub enum Signal {
    Started(Identity),
    Capacity(Capacity),
    /// The reverse side started, selling from this Bitcoin address.
    ReverseOn(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    pub bitcoin_address: String,
    pub claim_pub: String,
    pub pool_account: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capacity {
    pub held: u128,
    pub promised: u128,
    pub open: u64,
    pub holds: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Terms {
    /// The asset taken on Bitcoin and the one paid in the pool. `None` where the answer does not say.
    pub btc_asset: Option<Asset>,
    pub evm_asset: Option<Asset>,
    pub min: u128,
    pub max: u128,
    pub fee_bps: u64,
    pub flat: u128,
    /// The most it can take now. `None` when the provider does not say.
    pub available: Option<u128>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Swap {
    pub direction: Direction,
    pub id: String,
    pub phase: String,
    pub created: u64,
    /// The Ethereum deadline. 0 where the record does not carry one yet.
    pub deadline: u64,
    /// What the bridger pays and what it receives: TAC on Bitcoin for pool TAC forward, the other way round in reverse.
    pub pays: u128,
    pub gets: u128,
    pub lock_txid: String,
    /// The provider's take of the bridger's lock: where the TAC it was paid on Bitcoin landed. Empty until taken.
    pub take_txid: String,
    /// Every Bitcoin transaction the record names, with what it is to the swap. Each id is 64 hex digits.
    pub linked: Vec<(Role, String)>,
    /// Forward: the leaf of the provider's lock in the pool, 64 hex digits, as the pool's log names it. Empty until
    /// the provider has built the lock.
    pub pool_leaf: String,
}

/// What a Bitcoin transaction is to a swap, named as the provider's record names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Forward: the bridger's lock of its TAC, then the provider's take of it (a commit and the take itself).
    BridgerLock,
    TakeCommit,
    Take,
    /// Reverse: the provider's lock of a note for the bridger (a commit and the lock itself), and its ways back.
    LockCommit,
    Lock,
    Refund,
    TakeRefund,
    /// Forward, on Ethereum: the provider's lock of pool TAC for the bridger. A relay sends it, not the pool account.
    PoolLock,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::BridgerLock => "bridger's lock",
            Role::TakeCommit => "take commit",
            Role::Take => "take",
            Role::LockCommit => "lock commit",
            Role::Lock => "lock",
            Role::Refund => "refund",
            Role::TakeRefund => "take refund",
            Role::PoolLock => "pool lock",
        }
    }
    /// Whether the swap's TAC on Bitcoin rides on this transaction. The others only pay for one that does.
    pub fn carries_tac(self) -> bool {
        matches!(self, Role::BridgerLock | Role::Take | Role::Lock | Role::Refund)
    }
}

impl Swap {
    pub fn fee(&self) -> u128 {
        self.pays.saturating_sub(self.gets)
    }
    /// The provider has been paid. In reverse that is its claim of the pool lock, before the bridger takes the note.
    pub fn paid(&self) -> bool {
        match self.direction {
            Direction::Forward => self.phase == "done",
            Direction::Reverse => matches!(self.phase.as_str(), "claimed" | "done"),
        }
    }
    /// Finished one way or another: nothing of it is still at stake.
    pub fn closed(&self) -> bool {
        match self.direction {
            Direction::Forward => matches!(self.phase.as_str(), "done" | "refunded" | "rejected" | "aborted"),
            Direction::Reverse => matches!(self.phase.as_str(), "done" | "refunded" | "counter-took" | "aborted"),
        }
    }
}

/// The provider's reverse side: pool TAC in, TAC on Bitcoin out, from an address of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ReverseSide {
    /// The provider answers, and the side is switched off.
    #[default]
    Off,
    /// `terms.available` is the most pool TAC it can take now, which the TAC at its address bounds.
    On { terms: Terms, open: Option<u64>, max_open: u64 },
}

/// What is at the reverse address, as a tool that holds the key reports it: the TAC needs the key to read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReverseStock {
    pub address: String,
    /// Confirmed TAC notes the side can sell, and the TAC in them.
    pub notes: u64,
    pub tac: u128,
    /// Confirmed plain sats. `None` when the tool could not read them.
    pub sats: Option<u64>,
}

/// A bridge no provider online could take when it was posted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Want {
    /// The asset the bridger holds on Bitcoin.
    pub btc_asset: Option<Asset>,
    pub amount: u128,
    pub max_fee_bps: Option<u64>,
    pub until: u64,
}

/// A provider's ad on the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ad {
    pub name: String,
    pub claim_pub: String,
    pub terms: Terms,
    pub any_tac: bool,
    /// The ad has a reverse member, and that member's terms where they can be read.
    pub reverse: bool,
    pub reverse_terms: Option<Terms>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tac_groups_thousands_and_keeps_two_places() {
        let amounts = [24_900_000_000, 123_456_789_012_345];

        let out = amounts.map(tac);

        assert_eq!(out, ["249.00", "1,234,567.89"]);
    }
}
