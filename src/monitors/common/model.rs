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
