//! Where a provider's fee stands against the others on the board, and against what a swap costs it. Arithmetic
//! only: no state, no I/O. One direction at a time, in that direction's own unit: sats for BTC→ETH, pool ETH for
//! ETH→BTC.

use super::model::{Books, Flag, Terms, fee_on, flag};

/// Another provider's terms for the same direction, and who it says it is. The board checks no signature.
#[derive(Debug, Clone, Copy)]
pub struct Rival<'a> {
    pub name: &'a str,
    pub terms: &'a Terms,
}

/// Whether these terms would take a swap of this size now. What it can take unknown is not nothing.
pub fn takes(t: &Terms, size: u128) -> bool {
    (t.min..=t.max).contains(&size) && t.available.is_none_or(|a| a >= size)
}

/// Where the provider's fee stands at one size. The app takes the quote that pays the user most, so the cheapest
/// at a size is the one picked at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// The board's ads have never been read: there is nothing to compare with, which is not the same as no one.
    Unknown,
    /// No other provider on the board would take this size now.
    Alone,
    /// At or under the cheapest other, whose whole fee at this size `next` is.
    Cheapest { next: u128, name: String },
    /// Dearer than the cheapest other. `flat` is the flat fee that would match it at the provider's own share,
    /// or `None` when the share alone is already over it.
    Dearer { cheapest: u128, name: String, flat: Option<u128> },
}

/// One size, the provider's whole fee on it, and where that stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct At {
    pub size: u128,
    pub fee: u128,
    pub standing: Standing,
}

/// What a swap costs the provider, as an average of the swaps its books have counted, both directions together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    /// The Bitcoin fees of its own transactions, in sats, and the relay's fees, in pool ETH.
    pub sats: u128,
    pub eth: u128,
    /// Both together, in each unit, at the price given. `None` where a part cannot be priced: half a cost is not
    /// shown as the whole of one.
    pub as_sats: Option<u128>,
    pub as_eth: Option<u128>,
    pub swaps: u64,
}

/// `price` is ETH per BTC, which is also pool units per sat: both count to eight places. `None` with no swap counted.
pub fn cost(books: &Books, price: Option<f64>) -> Option<Cost> {
    let swaps = u128::from(books.swaps);
    if swaps == 0 {
        return None;
    }
    let (sats, eth) = (books.cost_sats.div_ceil(swaps), books.cost_eth.div_ceil(swaps));
    let price = price.filter(|p| p.is_finite() && *p > 0.0);
    // rounded up: a cost is covered or it is not
    let priced = |amount: u128, by: fn(f64, f64) -> f64| match (amount, price) {
        (0, _) => Some(0),
        (_, None) => None,
        (a, Some(p)) => Some(by(a as f64, p).ceil() as u128),
    };
    let as_sats = priced(eth, |a, p| a / p).map(|e| sats.saturating_add(e));
    let as_eth = priced(sats, |a, p| a * p).map(|s| eth.saturating_add(s));
    Some(Cost { sats, eth, as_sats, as_eth, swaps: books.swaps })
}

/// From what size a fee covers a cost. The fee only grows with the swap, so there is one edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Even {
    /// Every size the provider takes covers it.
    Always,
    /// This size and larger do; the provider takes smaller ones too.
    From(u128),
    /// Only a size over the most the provider takes would.
    OverMax(u128),
    /// No size does: there is no share, and the flat fee is under the cost.
    Never,
}

pub fn even(t: &Terms, cost: u128) -> Even {
    if fee_on(t.min, t.fee_bps, t.flat) >= cost {
        return Even::Always;
    }
    if t.fee_bps == 0 {
        return Even::Never;
    }
    // the flat fee is under the cost here, or the least size would have covered it
    let Some(short) = cost.saturating_sub(t.flat).checked_mul(10_000) else { return Even::Never };
    let size = short.div_ceil(u128::from(t.fee_bps));
    if size > t.max { Even::OverMax(size) } else { Even::From(size) }
}

/// One fee compared: what it takes at the least and the most the provider takes, and what is left of that after
/// the cost of a swap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub bps: u64,
    pub current: bool,
    pub fee_low: u128,
    pub fee_high: u128,
    /// `None` while what a swap costs is not known.
    pub net_low: Option<i128>,
    pub net_high: Option<i128>,
    /// Whether this fee would be the cheapest at each size. `None`: there is no one to compare with.
    pub picked_low: Option<bool>,
    pub picked_high: Option<bool>,
    pub high: Flag,
}

pub struct Advice {
    /// The least the provider takes, and the most it can take now where that is another size.
    pub low: At,
    pub high: Option<At>,
    pub rows: Vec<Row>,
    pub even: Option<Even>,
    /// The flat fee at which the smallest swap covers the cost, at the provider's own share.
    pub cover_flat: Option<u128>,
    /// Where the app would mark the provider's own fee as high.
    pub flag: Flag,
    /// The least and the most any other provider that has something to give takes. `None`: no such provider.
    pub others: Option<(u128, u128)>,
}

/// The cheapest whole fee among the rivals that would take `size` now, and whose it is.
fn cheapest<'a>(rivals: &[Rival<'a>], size: u128) -> Option<(u128, &'a str)> {
    rivals.iter().filter(|r| takes(r.terms, size)).map(|r| (fee_on(size, r.terms.fee_bps, r.terms.flat), r.name)).min_by_key(|(fee, _)| *fee)
}

fn standing(own: &Terms, size: u128, rivals: Option<&[Rival]>) -> At {
    let fee = fee_on(size, own.fee_bps, own.flat);
    let standing = match rivals.map(|r| cheapest(r, size)) {
        None => Standing::Unknown,
        Some(None) => Standing::Alone,
        Some(Some((next, name))) if fee <= next => Standing::Cheapest { next, name: name.to_string() },
        Some(Some((cheapest, name))) => Standing::Dearer { cheapest, name: name.to_string(), flat: cheapest.checked_sub(fee_on(size, own.fee_bps, 0)) },
    };
    At { size, fee, standing }
}

fn net(fee: u128, cost: Option<u128>) -> Option<i128> {
    Some(i128::try_from(fee).ok()?.saturating_sub(i128::try_from(cost?).ok()?))
}

/// `ladder` is the fees to compare; the provider's own is always among the rows. `rivals` is every other provider
/// of the direction, or `None` when the board's ads have never been read. `cost` is a swap's, in the direction's
/// unit; `step` is what the high-fee edge is rounded up to.
pub fn advise(own: &Terms, rivals: Option<&[Rival]>, ladder: &[u64], cost: Option<u128>, step: u128) -> Advice {
    let low = own.min;
    // the most it can take now, which what it holds can put under the most it would
    let top = own.max.min(own.available.unwrap_or(own.max)).max(low);
    let mut fees = ladder.to_vec();
    fees.push(own.fee_bps);
    fees.sort_unstable();
    fees.dedup();
    let picked = |size: u128, fee: u128| rivals.and_then(|r| cheapest(r, size)).map(|(next, _)| fee <= next);
    let rows = fees
        .into_iter()
        .map(|bps| {
            let (fee_low, fee_high) = (fee_on(low, bps, own.flat), fee_on(top, bps, own.flat));
            Row {
                bps,
                current: bps == own.fee_bps,
                fee_low,
                fee_high,
                net_low: net(fee_low, cost),
                net_high: net(fee_high, cost),
                picked_low: picked(low, fee_low),
                picked_high: picked(top, fee_high),
                high: flag(own.min, own.max, bps, own.flat, step),
            }
        })
        .collect();
    let live = || rivals.into_iter().flatten().filter(|r| r.terms.available.is_none_or(|a| a >= r.terms.min));
    Advice {
        low: standing(own, low, rivals),
        high: (top > low).then(|| standing(own, top, rivals)),
        rows,
        even: cost.map(|c| even(own, c)),
        cover_flat: cost.map(|c| c.saturating_sub(fee_on(low, own.fee_bps, 0))),
        flag: flag(own.min, own.max, own.fee_bps, own.flat, step),
        others: live().map(|r| r.terms.min).min().zip(live().map(|r| r.terms.max).max()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(min: u128, max: u128, bps: u64, flat: u128, available: Option<u128>) -> Terms {
        Terms { min, max, fee_bps: bps, flat, available, ..Default::default() }
    }

    #[test]
    fn a_swaps_cost_is_the_books_average_in_both_units_and_nothing_where_a_part_cannot_be_priced() {
        // one swap: 500 sats of Bitcoin fees and 26,000 units to the relay, at 35.9 ETH per BTC
        let books = Books { swaps: 1, cost_sats: 500, cost_eth: 26_000, ..Default::default() };

        let (priced, unpriced, none) = (cost(&books, Some(35.9)), cost(&books, None), cost(&Books::default(), Some(35.9)));
        let sats_only = cost(&Books { cost_eth: 0, swaps: 2, ..books.clone() }, None);

        assert_eq!(priced, Some(Cost { sats: 500, eth: 26_000, as_sats: Some(1_225), as_eth: Some(43_950), swaps: 1 }));
        assert_eq!((unpriced.map(|c| (c.as_sats, c.as_eth)), none), (Some((None, None)), None));
        // with nothing owed in the other unit there is nothing to price in this one
        assert_eq!(sats_only.map(|c| (c.sats, c.as_sats, c.as_eth)), Some((250, Some(250), None)));
    }

    #[test]
    fn a_fee_covers_a_cost_from_one_size_up_and_the_edge_is_exact() {
        // 0.20% + 399 against a cost of 1,225: the share must bring 826, which takes 413,000 sats
        let cost = 1_225;

        let read = [
            even(&terms(25_000, 97_758, 20, 399, None), cost),
            even(&terms(25_000, 500_000, 20, 399, None), cost),
            even(&terms(25_000, 500_000, 20, 1_200, None), cost),
            even(&terms(25_000, 500_000, 0, 399, None), cost),
        ];

        assert_eq!(read, [Even::OverMax(413_000), Even::From(413_000), Even::Always, Even::Never]);
        assert!(fee_on(413_000, 20, 399) >= cost && fee_on(412_999, 20, 399) < cost);
    }

    #[test]
    fn a_fee_stands_by_size_among_the_others_that_would_take_that_size_now() {
        let own = terms(20_000, 97_758, 15, 500, Some(97_758));
        // one takes from 25,000 and charges more; one is cheaper at any size and has nothing left for the larger
        let (dear, small) = (terms(25_000, 1_000_000, 20, 800, None), terms(20_000, 500_000, 5, 100, Some(30_000)));
        let rivals = [Rival { name: "dear", terms: &dear }, Rival { name: "small", terms: &small }];

        let a = advise(&own, Some(&rivals), &[], None, 1_000);

        // at 20,000 the small one takes 110 where this takes 530: a flat fee of 80 would match it at 0.15%
        assert_eq!(a.low, At { size: 20_000, fee: 530, standing: Standing::Dearer { cheapest: 110, name: "small".into(), flat: Some(80) } });
        assert_eq!(a.high, Some(At { size: 97_758, fee: 646, standing: Standing::Cheapest { next: 995, name: "dear".into() } }));
        assert_eq!(a.others, Some((20_000, 1_000_000)));
    }

    #[test]
    fn ads_never_read_are_unknown_not_an_empty_board_and_a_lone_provider_is_alone() {
        let own = terms(50_000, 50_000, 30, 1_000, None);

        let (unread, alone) = (advise(&own, None, &[], None, 1_000), advise(&own, Some(&[]), &[], None, 1_000));

        assert_eq!((unread.low.standing, alone.low.standing), (Standing::Unknown, Standing::Alone));
        // one size taken is one standing, and nobody to compare with marks no row
        assert!(alone.high.is_none() && alone.others.is_none() && alone.rows.iter().all(|r| r.picked_low.is_none()));
    }

    #[test]
    fn each_fee_compared_shows_what_is_left_after_the_cost_and_whether_it_would_be_picked() {
        let own = terms(25_000, 100_000, 20, 400, None);
        let other = terms(25_000, 100_000, 30, 400, None);
        let rivals = [Rival { name: "other", terms: &other }];

        let a = advise(&own, Some(&rivals), &[10, 50], Some(500), 1_000);

        let rows: Vec<_> = a.rows.iter().map(|r| (r.bps, r.current, r.fee_low, r.net_low, r.net_high, r.picked_low)).collect();
        assert_eq!(
            rows,
            [
                (10, false, 425, Some(-75), Some(0), Some(true)),
                (20, true, 450, Some(-50), Some(100), Some(true)),
                (50, false, 525, Some(25), Some(400), Some(false))
            ]
        );
        // 50,000 sats is where 0.20% + 400 first brings in 500; at the least it takes, a flat fee of 450 would
        assert_eq!((a.even, a.cover_flat), (Some(Even::From(50_000)), Some(450)));
    }

    #[test]
    fn a_fee_the_app_would_mark_as_high_is_flagged_where_it_would_be() {
        // 3,000 flat is 5% of 60,000 sats before any share: with 1% on top the mark holds under 75,000
        let own = terms(25_000, 500_000, 100, 3_000, None);

        let a = advise(&own, Some(&[]), &[], None, 1_000);

        assert_eq!(a.flag, Flag::Under(75_000));
    }
}
