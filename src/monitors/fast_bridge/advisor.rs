//! What the waiting bridges say about a provider's terms. Arithmetic only: no state, no I/O.

use super::model::{Terms, Want, effective_bps, fee_on, total};

/// Why a provider with these terms and this much free TAC is not taking a waiting bridge, or `None` when it could.
pub fn blocker(terms: &Terms, free: u128, w: &Want) -> Option<&'static str> {
    if w.amount > terms.max {
        Some("over your max")
    } else if w.amount < terms.min {
        Some("under your min")
    } else if w.max_fee_bps.is_some_and(|cap| effective_bps(w.amount, terms.fee_bps, terms.flat) > cap) {
        Some("fee over their cap")
    } else if w.amount > free {
        Some("not enough free TAC")
    } else {
        None
    }
}

pub struct FeeRow {
    pub bps: u64,
    /// Waiting bridges inside the size range whose fee cap this fee meets. One that names no cap counts.
    pub count: usize,
    pub volume: u128,
    pub revenue: u128,
    pub current: bool,
}

pub struct Advice {
    pub ladder: Vec<FeeRow>,
    /// The fee that earns the most on what is waiting, or `None` when nothing in range is waiting.
    pub best_bps: Option<u64>,
    pub in_range_volume: u128,
    pub over_count: usize,
    pub over_volume: u128,
    pub largest: u128,
    pub under_count: usize,
}

/// `ladder` is the fees to compare; the provider's own is always among the rows.
pub fn advise(terms: &Terms, wants: &[Want], ladder: &[u64]) -> Advice {
    let mut fees = ladder.to_vec();
    fees.push(terms.fee_bps);
    fees.sort_unstable();
    fees.dedup();
    let in_range = |w: &&Want| w.amount >= terms.min && w.amount <= terms.max;
    let ladder: Vec<FeeRow> = fees
        .into_iter()
        .map(|bps| {
            let fits: Vec<&Want> =
                wants.iter().filter(in_range).filter(|w| w.max_fee_bps.is_none_or(|cap| effective_bps(w.amount, bps, terms.flat) <= cap)).collect();
            FeeRow {
                bps,
                count: fits.len(),
                volume: total(fits.iter().map(|w| w.amount)),
                revenue: total(fits.iter().map(|w| fee_on(w.amount, bps, terms.flat))),
                current: bps == terms.fee_bps,
            }
        })
        .collect();
    let over: Vec<&Want> = wants.iter().filter(|w| w.amount > terms.max).collect();
    Advice {
        // `max_by_key` keeps the last of equals, so reversed it keeps the lowest fee among ties
        best_bps: ladder.iter().rev().max_by_key(|r| r.revenue).filter(|r| r.revenue > 0).map(|r| r.bps),
        ladder,
        in_range_volume: total(wants.iter().filter(in_range).map(|w| w.amount)),
        over_count: over.len(),
        over_volume: total(over.iter().map(|w| w.amount)),
        largest: wants.iter().map(|w| w.amount).max().unwrap_or(0),
        under_count: wants.iter().filter(|w| w.amount < terms.min).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::fast_bridge::model::UNIT;

    fn terms() -> Terms {
        Terms { min: 10 * UNIT, max: 500 * UNIT, fee_bps: 150, flat: UNIT / 10, available: None }
    }
    fn want(tac: u128, cap: Option<u64>) -> Want {
        Want { amount: tac * UNIT, max_fee_bps: cap, until: 0 }
    }

    #[test]
    fn blocker_names_the_first_thing_in_the_way() {
        let (t, free) = (terms(), 200 * UNIT);
        let wants = [want(600, None), want(5, None), want(100, Some(100)), want(300, None), want(100, None)];

        let why = wants.each_ref().map(|w| blocker(&t, free, w));

        assert_eq!(why, [Some("over your max"), Some("under your min"), Some("fee over their cap"), Some("not enough free TAC"), None]);
    }

    #[test]
    fn a_fee_over_a_cap_loses_that_bridge() {
        let t = terms();
        let wants = [want(100, Some(100)), want(100, None)];

        let a = advise(&t, &wants, &[50, 300]);

        let counts: Vec<(u64, usize)> = a.ladder.iter().map(|r| (r.bps, r.count)).collect();
        assert_eq!(counts, [(50, 2), (150, 1), (300, 1)]);
        assert!(a.ladder[1].current);
    }

    #[test]
    fn bridges_outside_the_size_range_are_counted_apart() {
        let t = terms();
        let wants = [want(2000, None), want(900, None), want(5, None), want(100, None)];

        let a = advise(&t, &wants, &[]);

        assert_eq!((a.over_count, a.over_volume, a.largest, a.under_count, a.in_range_volume), (2, 2900 * UNIT, 2000 * UNIT, 1, 100 * UNIT));
    }

    #[test]
    fn nothing_in_range_means_no_best_fee() {
        let t = terms();
        let wants = [want(2000, None)];

        let a = advise(&t, &wants, &[100]);

        assert_eq!(a.best_bps, None);
    }
}
