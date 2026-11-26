//! The Bitcoin transactions a provider's swaps and addresses depend on, as rows, and which of them are stuck.
//! Arithmetic only: no state, no I/O.

use super::model::{ChainTx, Direction, Outlook, Role, Swap};
pub use crate::monitors::common::chain::{MAX_SWAPS, SETTLED, Status};
use crate::monitors::common::chain::{Standing, rank, standing};
use std::collections::{HashMap, HashSet};

/// Which of the provider's addresses a transaction pays or spends from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Main,
    Reverse,
    /// Listed at both: it moved coins from one of the provider's addresses to the other.
    Both,
    /// Listed at neither: known through a swap record only. A user's lock pays a script, not an address of the provider's.
    Elsewhere,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxRow {
    pub txid: String,
    pub place: Place,
    /// What it is to a swap, and that swap's id and direction. `None`: no followed swap names it.
    pub role: Option<Role>,
    pub swap: Option<(String, Direction)>,
    /// The swap's sats, where this transaction carries them, by the swap record.
    pub swap_sats: Option<u128>,
    /// What it added to the provider's addresses less what it spent from them, by the explorer. `None` when it is
    /// listed at neither.
    pub sats: Option<i128>,
    pub standing: Standing,
}

pub struct Facts<'a> {
    pub swaps: &'a [Swap],
    /// The transactions listed at each address. `None`: not read.
    pub main: Option<&'a [ChainTx]>,
    pub reverse: Option<&'a [ChainTx]>,
    /// Transactions asked about by id: `Some(None)` is one the explorer does not know.
    pub looked: &'a HashMap<String, Option<ChainTx>>,
    pub outlook: Option<Outlook>,
    /// When each unconfirmed transaction was first seen.
    pub first_seen: &'a HashMap<String, u64>,
    pub now: u64,
    pub stuck_secs: u64,
}

/// The swaps whose transactions are followed: open ones first, then the newest, `MAX_SWAPS` at most.
pub fn followed(swaps: &[Swap]) -> Vec<&Swap> {
    let mut out: Vec<&Swap> = swaps.iter().collect();
    out.sort_by(|a, b| a.closed().cmp(&b.closed()).then(b.created.cmp(&a.created)));
    out.truncate(MAX_SWAPS);
    out
}

/// The ids of those swaps' transactions, each once, in that order.
pub fn linked_ids(swaps: &[Swap]) -> Vec<String> {
    let mut seen = HashSet::new();
    followed(swaps).into_iter().flat_map(|s| s.linked.iter().map(|(_, id)| id)).filter(|id| seen.insert(id.as_str())).cloned().collect()
}

/// Stuck first, then the other unconfirmed, then what an explorer has not placed, then confirmed. Newest first within each.
pub fn rows(f: &Facts) -> Vec<TxRow> {
    // one entry per transaction: listed at both addresses, it is one transfer and its sums are added
    let mut listed: HashMap<&str, (Place, ChainTx)> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for (place, list) in [(Place::Main, f.main), (Place::Reverse, f.reverse)] {
        for tx in list.unwrap_or_default() {
            match listed.get_mut(tx.txid.as_str()) {
                Some((at, sum)) => {
                    *at = if *at == place { place } else { Place::Both };
                    sum.received = sum.received.saturating_add(tx.received);
                    sum.sent = sum.sent.saturating_add(tx.sent);
                }
                None => {
                    order.push(&tx.txid);
                    listed.insert(&tx.txid, (place, tx.clone()));
                }
            }
        }
    }
    let row = |txid: &str, role: Option<Role>, swap: Option<&Swap>| {
        let at_address = listed.get(txid);
        let tx = at_address.map(|(_, tx)| tx).or_else(|| f.looked.get(txid).and_then(Option::as_ref));
        TxRow {
            txid: txid.to_string(),
            place: at_address.map_or(Place::Elsewhere, |(place, _)| *place),
            role,
            swap: swap.map(|s| (s.id.clone(), s.direction)),
            swap_sats: swap.zip(role).filter(|(_, r)| r.carries_sats()).map(|(s, _)| s.sats),
            sats: at_address.map(|(_, tx)| i128::from(tx.received) - i128::from(tx.sent)),
            standing: standing(tx, f.looked.contains_key(txid), f.outlook, f.first_seen.get(txid).copied(), f.now, f.stuck_secs),
        }
    };

    let mut seen = HashSet::new();
    let mut out: Vec<(u64, TxRow)> = Vec::new();
    for s in followed(f.swaps) {
        for (role, id) in &s.linked {
            if seen.insert(id.as_str()) {
                let r = row(id, Some(*role), Some(s));
                // one with no time of its own is placed by when its swap began
                out.push((r.standing.at.unwrap_or(s.created), r));
            }
        }
    }
    for id in order {
        if seen.insert(id) {
            let r = row(id, None, None);
            out.push((r.standing.at.unwrap_or(f.now), r));
        }
    }
    out.sort_by(|(a_at, a), (b_at, b)| rank(&a.standing).cmp(&rank(&b.standing)).then(b_at.cmp(a_at)));
    out.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(c: char) -> String {
        c.to_string().repeat(64)
    }
    fn tx(c: char, height: Option<u64>, received: u64, sent: u64) -> ChainTx {
        ChainTx { txid: id(c), fee: 430, vsize: 430, height, time: height.map(|h| h * 600), received, sent, ..Default::default() }
    }
    fn swap(n: u64, direction: Direction, phase: &str, linked: &[(Role, char)]) -> Swap {
        Swap {
            direction,
            id: format!("swap{n}"),
            phase: phase.into(),
            created: n,
            sats: 100_000,
            eth: 3_100_000,
            linked: linked.iter().map(|(r, c)| (*r, id(*c))).collect(),
            ..Default::default()
        }
    }
    fn facts<'a>(
        swaps: &'a [Swap],
        main: &'a [ChainTx],
        reverse: &'a [ChainTx],
        looked: &'a HashMap<String, Option<ChainTx>>,
        seen: &'a HashMap<String, u64>,
    ) -> Facts<'a> {
        Facts {
            swaps,
            main: Some(main),
            reverse: Some(reverse),
            looked,
            outlook: Some(Outlook { tip: 105, hour_milli: 1_000 }),
            first_seen: seen,
            now: 10_000,
            stuck_secs: 1_800,
        }
    }

    #[test]
    fn a_swaps_transactions_are_placed_by_the_address_list_then_a_lookup_and_carry_the_swaps_sats() {
        // the take pays the main address; the user's lock pays a script and is known by its id alone; the refund is not known
        let swaps =
            [swap(1, Direction::Forward, "done", &[(Role::UserLock, 'a'), (Role::Take, 'b')]), swap(2, Direction::Reverse, "refunded", &[(Role::Refund, 'c')])];
        let main = [tx('b', Some(104), 99_500, 0)];
        let looked = HashMap::from([(id('a'), Some(tx('a', Some(100), 0, 0))), (id('c'), None)]);
        let none = HashMap::new();

        let out = rows(&facts(&swaps, &main, &[], &looked, &none));

        let read = |c: char| out.iter().find(|r| r.txid == id(c)).map(|r| (r.place, r.role, r.sats, r.swap_sats, r.standing.status)).expect("a row");
        assert_eq!(read('a'), (Place::Elsewhere, Some(Role::UserLock), None, Some(100_000), Status::Confirmed(Some(6))));
        assert_eq!(read('b'), (Place::Main, Some(Role::Take), Some(99_500), Some(100_000), Status::Confirmed(Some(2))));
        assert_eq!(read('c'), (Place::Elsewhere, Some(Role::Refund), None, Some(100_000), Status::Missing));
    }

    #[test]
    fn a_stuck_transaction_is_first_and_one_at_both_addresses_is_one_transfer() {
        let swaps = [swap(1, Direction::Reverse, "broadcast", &[(Role::Lock, 'a')])];
        let (main, reverse) = ([tx('m', Some(104), 50_000, 0), tx('t', Some(103), 0, 40_000)], [tx('a', None, 0, 100_500), tx('t', Some(103), 39_500, 0)]);
        let (looked, seen) = (HashMap::new(), HashMap::from([(id('a'), 8_000)]));

        let out = rows(&facts(&swaps, &main, &reverse, &looked, &seen));

        assert_eq!(out.iter().map(|r| r.txid.clone()).collect::<Vec<_>>(), [id('a'), id('m'), id('t')]);
        assert!(out[0].standing.stuck && out[0].sats == Some(-100_500));
        assert_eq!((out[2].place, out[2].sats, out[2].role), (Place::Both, Some(-500), None));
    }

    #[test]
    fn only_the_open_and_the_newest_swaps_are_followed_and_a_transaction_is_asked_about_once() {
        let mut swaps: Vec<Swap> = (0..60).map(|n| swap(n, Direction::Forward, "done", &[(Role::UserLock, 'a')])).collect();
        swaps.push(swap(0, Direction::Forward, "offered", &[(Role::UserLock, 'b'), (Role::Take, 'b')]));

        let (kept, ids) = (followed(&swaps), linked_ids(&swaps));

        assert_eq!((kept.len(), kept[0].phase.as_str(), kept[1].created), (MAX_SWAPS, "offered", 59));
        assert_eq!(ids, [id('b'), id('a')]);
    }
}
