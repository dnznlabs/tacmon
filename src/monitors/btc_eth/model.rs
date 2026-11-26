//! What a provider of Bitcoin for ETH is, as this monitor sees it. No I/O and no wire formats here.
//! Two units, and a name always says which: sats on Bitcoin, and pool ETH in units of 1e-8 ETH.

pub use crate::monitors::common::model::{Asset, ChainTx, Health, Level, Outlook, PoolLock, PoolScan, effective_bps, fee_on, rate, total};

/// Sats in a bitcoin, and pool units in an ETH: the pool counts ETH to 8 places.
pub const UNIT: u128 = 100_000_000;

/// Which way a swap goes, as the user sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    /// The user's sats in, the provider's pool ETH out.
    #[default]
    Forward,
    /// The user's pool ETH in, the provider's sats out, from the reverse side's own address.
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
            Direction::Forward => "BTC→ETH",
            Direction::Reverse => "ETH→BTC",
        }
    }
}

fn grouped(whole: u128) -> String {
    let whole = whole.to_string();
    let mut out = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "1,250,000": sats, whole.
pub fn sats(s: u128) -> String {
    grouped(s)
}

/// "0.07125": pool units as ETH, to five places and never rounded up: a balance is not shown larger than it is.
pub fn eth(units: u128) -> String {
    format!("{}.{:05}", grouped(units / UNIT), (units % UNIT) / 1000)
}

/// The same for a figure that can be under zero (a net after costs).
pub fn eth_signed(units: i128) -> String {
    format!("{}{}", if units < 0 { "−" } else { "" }, eth(units.unsigned_abs()))
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

#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    Started(Identity),
    Capacity(Capacity),
    /// The reverse side started, selling from this Bitcoin address.
    ReverseOn(String),
    /// The price the provider started with, in ETH per BTC. `None`: it started with none.
    StartPrice(Option<f64>),
    /// The provider said it has no price and quotes nothing (`false`), or that it has one again.
    Priced(bool),
    /// The provider's books counted a swap that ended: what it earned over the market.
    Counted {
        id: String,
        fee: Fee,
    },
}

/// A swap's fee as the provider's books count it: sats for BTC→ETH, pool ETH for ETH→BTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fee {
    Sats(u128),
    Eth(u128),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    pub bitcoin_address: String,
    pub claim_pub: String,
    pub pool_account: String,
    pub dry_run: bool,
}

/// The provider's pool ETH as its log last put it, in pool units.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capacity {
    pub held: u128,
    pub promised: u128,
    pub open: u64,
    pub holds: u64,
}

/// One side's terms. BTC→ETH counts in sats (the amount a user locks); ETH→BTC in pool ETH units.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Terms {
    /// The asset on Bitcoin and the one in the pool. `None` where the answer does not say.
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
    /// The Bitcoin side and the pool side of the swap. Which one the user pays is the direction's.
    pub sats: u128,
    pub eth: u128,
    pub lock_txid: String,
    /// The Bitcoin transactions the record names, in the order they happen.
    pub linked: Vec<(Role, String)>,
    /// What the record itself says of some of them, by id: the provider's own, which it built and sent.
    pub said: Vec<(String, Said)>,
    /// The block the user's lock was seen in, where the record says.
    pub lock_height: Option<u64>,
    /// BTC→ETH: the leaf of the provider's lock of ETH in the pool, as 64 hex digits. Empty until the record names it.
    pub pool_leaf: String,
}

/// What a swap record says of a transaction the provider sent. It is the provider's word, and needs no explorer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Said {
    /// Its fee in sats and its size in virtual bytes.
    pub fee: Option<u64>,
    pub vsize: Option<u64>,
    /// When the provider sent it, or sent its latest replacement.
    pub at: Option<u64>,
    /// Times the provider replaced it with one paying more.
    pub bumps: u64,
}

impl Said {
    /// Its fee rate in thousandths of a sat per virtual byte.
    pub fn rate_milli(&self) -> Option<u64> {
        let (fee, vsize) = (self.fee?, self.vsize.filter(|v| *v > 0)?);
        Some(fee.saturating_mul(1000) / vsize)
    }
}

/// The quotes the provider has given, by the files it keeps of them: how many, and when the newest was written.
/// A provider on the board that is never asked, or cannot be reached, gives none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Quotes {
    pub count: usize,
    pub last: Option<u64>,
}

/// What a Bitcoin transaction is to a swap, named as the provider's record names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// BTC→ETH: the user's lock of its sats, then the provider's take of them. The take pays its own fee out of the lock.
    UserLock,
    Take,
    /// ETH→BTC: the provider's lock of sats for the user (after a commit, where the record names one), and its ways back.
    LockCommit,
    Lock,
    Refund,
    TakeRefund,
    /// BTC→ETH, on Ethereum: the provider's lock of pool ETH for the user. A relay sends it, not the pool account.
    PoolLock,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::UserLock => "user's lock",
            Role::Take => "take",
            Role::LockCommit => "lock commit",
            Role::Lock => "lock",
            Role::Refund => "refund",
            Role::TakeRefund => "take refund",
            Role::PoolLock => "pool lock",
        }
    }
    /// Whether the swap's sats ride on this transaction. A commit only pays for the one that does.
    pub fn carries_sats(self) -> bool {
        !matches!(self, Role::LockCommit | Role::PoolLock)
    }
}

impl Swap {
    /// The provider has been paid. In reverse that is its claim of the pool lock, before the user takes the Bitcoin.
    pub fn paid(&self) -> bool {
        match self.direction {
            Direction::Forward => self.phase == "done",
            Direction::Reverse => matches!(self.phase.as_str(), "claimed" | "done"),
        }
    }
    /// Finished one way or another: nothing of it is still at stake.
    pub fn closed(&self) -> bool {
        match self.direction {
            Direction::Forward => matches!(self.phase.as_str(), "done" | "refunded" | "rejected" | "aborted" | "lost"),
            Direction::Reverse => matches!(self.phase.as_str(), "done" | "refunded" | "counter-took" | "aborted"),
        }
    }
    /// Ended with the provider out of pocket, as the provider itself names it.
    pub fn lost(&self) -> bool {
        self.phase == "lost"
    }
}

/// The provider's reverse side: pool ETH in, sats out, from an address of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ReverseSide {
    /// The provider answers, and the side is switched off.
    #[default]
    Off,
    /// `terms.available` is the most pool ETH it can take now, which the sats at its address bound.
    On { terms: Terms, open: Option<u64>, max_open: u64 },
}

/// A provider's ad on the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ad {
    pub name: String,
    pub claim_pub: String,
    pub terms: Terms,
    /// The ad has a reverse member, and that member's terms where they can be read.
    pub reverse: bool,
    pub reverse_terms: Option<Terms>,
}

/// `/info`: the BTC→ETH terms, and the provider's books where its upkeep has made them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Info {
    pub terms: Terms,
    pub books: Option<Books>,
}

/// What the provider's upkeep reports of itself. Every figure is the provider's own count, since `since`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Books {
    pub since: u64,
    pub swaps: u64,
    pub forward: u64,
    pub reverse: u64,
    pub refunds: u64,
    pub fees_sats: u128,
    pub fees_eth: u128,
    pub cost_sats: u128,
    pub cost_eth: u128,
    pub loss_sats: u128,
    pub loss_eth: u128,
    /// Fees less costs and losses, all as pool ETH at the price of each swap. `None` where it cannot be read.
    pub net_eth: Option<i128>,
    pub reinvested_eth: u128,
    pub owed_eth: u128,
    pub stock: Option<Stock>,
    pub notes: Option<Notes>,
    pub target: Option<Target>,
    /// How far the provider leans its one rate to draw the side it is short of, in hundredths of a percent.
    pub lean_bps: Option<i64>,
    pub buyback: Option<Buyback>,
}

/// What the provider held when its upkeep last looked, and what it was worth then against when it first looked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stock {
    pub eth: u128,
    /// The sats at the reverse address. `None` while the reverse side is off: nothing reads them.
    pub sats: Option<u128>,
    pub value_eth: u128,
    pub at: u64,
    pub first_value_eth: u128,
    pub first_at: u64,
}

impl Stock {
    /// ETH per BTC as the provider valued its sats at that look. `None` without sats to value.
    pub fn price(&self) -> Option<f64> {
        let sats = self.sats.filter(|s| *s > 0)?;
        Some(self.value_eth.checked_sub(self.eth)? as f64 / sats as f64)
    }
}

/// The provider's free ETH notes in the pool. One lock draws from the largest sixteen, so `usable` is the most a
/// single swap can be given, whatever `total` says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes {
    pub count: u64,
    pub total: u128,
    pub usable: u128,
    pub dust: u64,
    pub largest: u128,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    pub eth: u128,
    pub btc_as_eth: u128,
    pub reached: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Buyback {
    pub on: bool,
    pub routed_eth: u128,
    pub sends: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sats_are_grouped_and_eth_is_cut_to_five_places_never_rounded_up() {
        let (s, e, net) = (1_250_000, 7_125_999, -150_000);

        let out = (sats(s), eth(e), eth(1_234 * UNIT), eth_signed(net));

        assert_eq!(out, ("1,250,000".to_string(), "0.07125".to_string(), "1,234.00000".to_string(), "−0.00150".to_string()));
    }

    #[test]
    fn a_stock_gives_the_price_its_sats_were_valued_at_and_none_without_sats() {
        // 0.5 ETH and 1,000,000 sats worth 0.31 ETH at 31 ETH per BTC
        let stock = Stock { eth: 50_000_000, sats: Some(1_000_000), value_eth: 81_000_000, ..Default::default() };

        let (priced, empty, off) = (stock.price(), Stock { sats: Some(0), ..stock.clone() }.price(), Stock { sats: None, ..stock.clone() }.price());

        assert_eq!((priced, empty, off), (Some(31.0), None, None));
    }

    #[test]
    fn a_swap_is_paid_and_closed_by_its_direction_and_a_lost_one_is_closed() {
        let swap = |direction, phase: &str| Swap { direction, phase: phase.into(), ..Default::default() };

        let read =
            [swap(Direction::Forward, "taking"), swap(Direction::Forward, "lost"), swap(Direction::Reverse, "claimed"), swap(Direction::Reverse, "done")]
                .map(|s| (s.paid(), s.closed(), s.lost()));

        assert_eq!(read, [(false, false, false), (false, true, true), (true, false, false), (true, true, false)]);
    }
}
