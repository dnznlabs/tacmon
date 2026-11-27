//! The types and the arithmetic no monitor should have a version of its own of. Amounts are whole base units of
//! whatever the monitor counts in: nothing here knows which.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Alert,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Health {
    pub ok: bool,
    pub error: String,
}

/// What is swapped, as the protocol names it: 32 bytes. The board carries every pair any provider serves, so an
/// amount means nothing until its asset is known to be the one being watched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset(pub [u8; 32]);

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

/// The app marks a swap as a high fee when the whole fee is over this share of the amount (its `warnFeeBps`).
pub const HIGH_FEE_BPS: u64 = 500;

/// Where the app's high-fee mark falls on the sizes a provider takes. The flat part weighs most on a small swap, so
/// the mark can start part way up the size range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Never,
    Always,
    /// Sizes under this are marked; this size and larger are not.
    Under(u128),
}

/// The app's own rule (`fee * 10000 > amount * warnFeeBps`), on one swap.
pub fn flagged(amount: u128, bps: u64, flat: u128) -> bool {
    fee_on(amount, bps, flat).checked_mul(10_000).is_none_or(|f| f > amount.saturating_mul(u128::from(HIGH_FEE_BPS)))
}

/// The mark across `min..=max`. The fee's share only falls as a swap grows, so the smallest size decides `Never`,
/// the largest `Always`, and the edge between them is found by halving. The app's rounding makes the edge ragged by
/// a base unit or so: rounded up to a whole `step`, it reads as the size it is.
pub fn flag(min: u128, max: u128, bps: u64, flat: u128, step: u128) -> Flag {
    if min > max || !flagged(min, bps, flat) {
        return Flag::Never;
    }
    if flagged(max, bps, flat) {
        return Flag::Always;
    }
    let (mut low, mut high) = (min, max);
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if flagged(mid, bps, flat) { low = mid } else { high = mid }
    }
    let step = step.max(1);
    Flag::Under(high.div_ceil(step).saturating_mul(step).min(max))
}

/// One thing a Maintenance page says to do, and the command that does it where there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub level: Level,
    pub text: String,
    /// The command, one line per part, joined with ` \` so it pastes as one.
    pub cmd: Vec<String>,
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
    /// Its note-sized outputs to other addresses, where it also spends a note-sized coin of the address it was listed
    /// for: the shape of TAC leaving that address. Where it went, and whether it is TAC at all, takes a key to tell.
    pub notes_away: Vec<u32>,
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

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: u128 = 100_000_000;

    #[test]
    fn fee_is_share_plus_flat() {
        // 1.5% + 0.1 on 252.89340101 leaves 249
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
