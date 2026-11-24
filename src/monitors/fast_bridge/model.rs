//! What a fast bridge provider is, as this monitor sees it. No I/O and no wire formats here.
//! Amounts are TAC base units (8 decimal places) unless a name says otherwise.

pub const UNIT: u128 = 100_000_000;
pub const PHASES: [&str; 6] = ["accepted", "locking", "locked", "offered", "taking", "done"];

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swap {
    pub id: String,
    pub phase: String,
    pub created: u64,
    pub deadline: u64,
    /// What the bridger pays on Bitcoin, and what it receives in the pool.
    pub pays: u128,
    pub gets: u128,
    pub lock_txid: String,
}

impl Swap {
    pub fn fee(&self) -> u128 {
        self.pays.saturating_sub(self.gets)
    }
    pub fn done(&self) -> bool {
        self.phase == "done"
    }
    /// Finished one way or another: nothing of it is still at stake.
    pub fn closed(&self) -> bool {
        matches!(self.phase.as_str(), "done" | "refunded" | "rejected" | "aborted")
    }
}

/// A bridge no provider online could take when it was posted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
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
    pub reverse: bool,
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
