//! What the waiting bridges say about a provider's terms. Arithmetic only: no state, no I/O.

use super::model::{Terms, UNIT, Want, effective_bps, fee_on, total};

/// The app marks a bridge as a high fee when the whole fee is over this share of the amount (its `warnFeeBps`).
pub const HIGH_FEE_BPS: u64 = 500;
/// The size fees are compared at, as on the Market page: a flat fee weighs more on a small bridge.
pub const REFERENCE: u128 = 100 * UNIT;

/// A fee as the bridger meets it: the share and the flat part together, at the reference size.
pub fn all_in(bps: u64, flat: u128) -> u64 {
    effective_bps(REFERENCE, bps, flat)
}

/// Why a provider with these terms and this much free TAC is not taking a waiting bridge, or `None` when it could.
/// `free` unknown is not zero: the bridge is then judged on the terms alone.
pub fn blocker(terms: &Terms, free: Option<u128>, w: &Want) -> Option<&'static str> {
    if w.amount > terms.max {
        Some("over your max")
    } else if w.amount < terms.min {
        Some("under your min")
    } else if w.max_fee_bps.is_some_and(|cap| effective_bps(w.amount, terms.fee_bps, terms.flat) > cap) {
        Some("fee over their cap")
    } else if free.is_some_and(|free| w.amount > free) {
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
    /// The app would mark a bridge at this fee as a high fee.
    pub high: bool,
}

/// Where the provider's fee stands against the others advertising now. It is about being chosen, not about earning
/// the most: a bridger is offered the cheaper provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// No other provider with TAC to lend is on the board.
    Alone,
    /// At or under the cheapest other provider, whose all-in fee this is.
    Cheapest { next: u64 },
    /// Dearer than the cheapest other provider. `matching` is the highest fee compared that would be at or under
    /// it, or `None` when every fee compared is dearer.
    Dearer { cheapest: u64, matching: Option<u64> },
}

pub struct Advice {
    pub ladder: Vec<FeeRow>,
    pub standing: Standing,
    /// The fee to mark on the ladder: the provider's own when it is already the cheapest, or the one that would match.
    pub competitive_bps: Option<u64>,
    pub in_range_volume: u128,
    pub over_count: usize,
    pub over_volume: u128,
    pub largest: u128,
    pub under_count: usize,
}

/// `ladder` is the fees to compare; the provider's own is always among the rows. `others` is the all-in fee of every
/// other provider that could take a bridge now.
pub fn advise(terms: &Terms, wants: &[Want], ladder: &[u64], others: &[u64]) -> Advice {
    let mut fees = ladder.to_vec();
    fees.push(terms.fee_bps);
    fees.sort_unstable();
    fees.dedup();
    let in_range = |w: &&Want| w.amount >= terms.min && w.amount <= terms.max;
    let ladder: Vec<FeeRow> = fees
        .into_iter()
        .map(|bps| {
            let fits: Vec<&Want> = wants
                .iter()
                .filter(in_range)
                .filter(|w| w.max_fee_bps.is_none_or(|cap| effective_bps(w.amount, bps, terms.flat) <= cap))
                // a bridge smaller than its own fee is never made, so it earns nothing
                .filter(|w| fee_on(w.amount, bps, terms.flat) < w.amount)
                .collect();
            FeeRow {
                bps,
                count: fits.len(),
                volume: total(fits.iter().map(|w| w.amount)),
                revenue: total(fits.iter().map(|w| fee_on(w.amount, bps, terms.flat))),
                current: bps == terms.fee_bps,
                high: all_in(bps, terms.flat) > HIGH_FEE_BPS,
            }
        })
        .collect();
    let over: Vec<&Want> = wants.iter().filter(|w| w.amount > terms.max).collect();
    let standing = match others.iter().min() {
        None => Standing::Alone,
        Some(&cheapest) if all_in(terms.fee_bps, terms.flat) <= cheapest => Standing::Cheapest { next: cheapest },
        // the ladder is sorted, so the last row at or under the cheapest is the highest fee that still matches it
        Some(&cheapest) => Standing::Dearer { cheapest, matching: ladder.iter().rfind(|r| all_in(r.bps, terms.flat) <= cheapest).map(|r| r.bps) },
    };
    Advice {
        competitive_bps: match standing {
            Standing::Alone => None,
            Standing::Cheapest { .. } => Some(terms.fee_bps),
            Standing::Dearer { matching, .. } => matching,
        },
        standing,
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
        let (t, free) = (terms(), Some(200 * UNIT));
        let wants = [want(600, None), want(5, None), want(100, Some(100)), want(300, None), want(100, None)];

        let why = wants.each_ref().map(|w| blocker(&t, free, w));

        assert_eq!(why, [Some("over your max"), Some("under your min"), Some("fee over their cap"), Some("not enough free TAC"), None]);
    }

    #[test]
    fn free_tac_that_is_not_known_blocks_nothing() {
        let t = terms();

        let why = blocker(&t, None, &want(300, None));

        assert_eq!(why, None);
    }

    #[test]
    fn a_fee_over_a_cap_loses_that_bridge() {
        let t = terms();
        let wants = [want(100, Some(100)), want(100, None)];

        let a = advise(&t, &wants, &[50, 300], &[]);

        let counts: Vec<(u64, usize)> = a.ladder.iter().map(|r| (r.bps, r.count)).collect();
        assert_eq!(counts, [(50, 2), (150, 1), (300, 1)]);
        assert!(a.ladder[1].current);
    }

    #[test]
    fn bridges_outside_the_size_range_are_counted_apart() {
        let t = terms();
        let wants = [want(2000, None), want(900, None), want(5, None), want(100, None)];

        let a = advise(&t, &wants, &[], &[]);

        assert_eq!((a.over_count, a.over_volume, a.largest, a.under_count, a.in_range_volume), (2, 2900 * UNIT, 2000 * UNIT, 1, 100 * UNIT));
    }

    #[test]
    fn a_bridge_smaller_than_its_fee_earns_nothing() {
        let t = Terms { min: 0, ..terms() };
        let dust = [Want { amount: 1, max_fee_bps: None, until: 0 }; 3];

        let a = advise(&t, &dust, &[], &[]);

        assert_eq!((a.ladder[0].count, a.ladder[0].revenue), (0, 0));
    }

    #[test]
    fn the_fee_marked_is_the_one_that_matches_the_cheapest_other_not_the_one_that_earns_most() {
        // own: 1.50% + 0.1 TAC is 1.60% all in at 100 TAC; the cheapest other is 0.90% all in
        let t = terms();
        let wants = [want(100, None)];

        let a = advise(&t, &wants, &[30, 80, 300, 500], &[250, 90]);

        assert_eq!(a.standing, Standing::Dearer { cheapest: 90, matching: Some(80) });
        assert_eq!(a.competitive_bps, Some(80));
    }

    #[test]
    fn a_provider_already_the_cheapest_is_told_to_keep_its_fee_not_to_raise_it() {
        let t = terms();

        let a = advise(&t, &[], &[300, 500], &[400]);

        assert_eq!((a.standing, a.competitive_bps), (Standing::Cheapest { next: 400 }, Some(150)));
    }

    #[test]
    fn with_no_other_provider_no_fee_is_marked() {
        let t = terms();

        let a = advise(&t, &[want(100, None)], &[500], &[]);

        assert_eq!((a.standing, a.competitive_bps), (Standing::Alone, None));
    }

    #[test]
    fn a_fee_the_app_would_flag_is_marked_high() {
        // the flat 0.1 TAC counts: 5.00% is 5.10% all in at 100 TAC
        let t = terms();

        let a = advise(&t, &[], &[490, 500], &[]);

        let high: Vec<(u64, bool)> = a.ladder.iter().map(|r| (r.bps, r.high)).collect();
        assert_eq!(high, [(150, false), (490, false), (500, true)]);
    }
}
