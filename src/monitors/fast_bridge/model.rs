//! What a fast bridge provider is, as this monitor sees it. No I/O and no wire formats here.
//! Amounts are TAC base units (8 decimal places) unless a name says otherwise.

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

/// The whole fee on `amount` at `bps` plus `flat`.
/// Board amounts are anyone's to post, so nothing here may wrap: a fee too large to hold is the largest there is.
pub fn fee_on(amount: u128, bps: u64, flat: u128) -> u128 {
    amount.checked_mul(u128::from(bps)).map_or(u128::MAX, |share| (share / 10_000).saturating_add(flat))
}

/// A sum that stops at the top instead of wrapping round to a small number.
pub fn total(amounts: impl Iterator<Item = u128>) -> u128 {
    amounts.fold(0, u128::saturating_add)
}

/// That fee as a share of the amount, in hundredths of a percent. Rounded up: a cap is met or it is not.
pub fn effective_bps(amount: u128, bps: u64, flat: u128) -> u64 {
    if amount == 0 {
        return u64::MAX;
    }
    fee_on(amount, bps, flat).checked_mul(10_000).and_then(|f| u64::try_from(f.div_ceil(amount)).ok()).unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Alert,
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

/// What is bridged, as the protocol names it: 32 bytes. The board carries every pair any provider serves, so an
/// amount means nothing until its asset is known to be the one being watched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset(pub [u8; 32]);

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
pub struct Health {
    pub ok: bool,
    pub error: String,
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

/// The size of the output a Tacit note rides on. Anything larger is plain sats.
pub const DUST: u64 = 546;

/// An unspent output at a Bitcoin address, as an explorer reports it. `txid` is 64 hex digits: the reader drops anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coin {
    pub txid: String,
    pub vout: u32,
    pub sats: u64,
    pub confirmed: bool,
}

impl Coin {
    pub fn outpoint(&self) -> String {
        format!("{}:{}", self.txid, self.vout)
    }
    /// An output a note may ride on. Whether one does, and for how much TAC, takes the key to tell.
    pub fn note_sized(&self) -> bool {
        self.sats <= DUST
    }
}

/// A transaction as an explorer reports it. `txid` is 64 hex digits: the reader drops anything else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChainTx {
    pub txid: String,
    /// What it pays to be mined, in sats, and its size in virtual bytes.
    pub fee: u64,
    pub vsize: u64,
    /// The block it is in. `None`: not confirmed.
    pub height: Option<u64>,
    /// When that block was mined.
    pub time: Option<u64>,
    /// The sats in its outputs to, and in its inputs from, the address it was listed for. Zero for one looked up by id.
    pub received: u64,
    pub sent: u64,
    /// One of those outputs is the size a Tacit note rides on. Whether one does, and for how much, takes the key to tell.
    pub note: bool,
}

impl ChainTx {
    /// Its fee rate in thousandths of a sat per virtual byte.
    pub fn rate_milli(&self) -> Option<u64> {
        (self.vsize > 0).then(|| self.fee.saturating_mul(1000) / self.vsize)
    }
}

/// The chain as an explorer sees it now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outlook {
    /// The newest block's height.
    pub tip: u64,
    /// The fee rate expected to confirm within about an hour (six blocks), in thousandths of a sat per virtual byte.
    pub hour_milli: u64,
}

/// "1.16": thousandths of a sat per virtual byte, as sats.
pub fn rate(milli: u64) -> String {
    format!("{}.{:02}", milli / 1000, (milli % 1000) / 10)
}

/// A lock put into the pool, as the pool's own log on Ethereum reports it. `leaf` is 64 hex digits and `tx` is
/// `0x` and 64: the reader drops anything else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoolLock {
    pub leaf: String,
    pub tx: String,
    pub block: u64,
    /// When that block was made, where the node says.
    pub time: Option<u64>,
}

/// What has been read of the pool's log so far.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PoolScan {
    /// The newest block, and the oldest one the read reaches back to: a lock made before `since` is not in `locks`.
    pub tip: u64,
    pub since: u64,
    /// Every lock anyone put into the pool in those blocks, oldest first. The log does not say whose a lock is:
    /// a swap record's leaf does.
    pub locks: Vec<PoolLock>,
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

    #[test]
    fn fee_is_share_plus_flat() {
        // 1.5% + 0.1 TAC on 252.89340101 TAC leaves the bridger 249 TAC
        let (pays, bps, flat) = (25_289_340_101, 150, 10_000_000);

        let gets = pays - fee_on(pays, bps, flat);

        assert_eq!(gets, 24_900_000_000);
    }

    #[test]
    fn amounts_too_large_to_hold_stop_at_the_top_and_never_wrap() {
        let huge = u128::MAX / 2;

        let (fee, eff, sum) = (fee_on(huge, 10_000, huge), effective_bps(100 * UNIT, u64::MAX, u128::MAX), total([huge, huge, huge].into_iter()));

        assert_eq!((fee, eff, sum), (u128::MAX, u64::MAX, u128::MAX));
    }

    #[test]
    fn effective_fee_counts_the_flat_part() {
        let (amount, bps, flat) = (100 * UNIT, 30, 10_000_000);

        let eff = effective_bps(amount, bps, flat);

        assert_eq!(eff, 40);
    }
}
