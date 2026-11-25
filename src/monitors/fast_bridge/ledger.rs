//! The Bitcoin transactions a provider's swaps and addresses depend on, as rows, and which of them are stuck.
//! Arithmetic only: no state, no I/O.

use super::model::{ChainTx, Direction, Outlook, Role, Swap};
use std::collections::{HashMap, HashSet};

/// How many swaps' transactions are followed: the open ones first, then the newest. It bounds what an explorer is asked.
pub const MAX_SWAPS: usize = 50;
/// Blocks after which a transaction is settled, and not asked about again.
pub const SETTLED: u64 = 6;

/// Which of the provider's addresses a transaction pays or spends from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Main,
    Reverse,
    /// Listed at both: it moved coins from one of the provider's addresses to the other.
    Both,
    /// Listed at neither: known through a swap record only.
    Elsewhere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Not asked about yet, or the explorer did not answer for it.
    Unknown,
    /// The explorer answers and does not know it: not broadcast yet, dropped, or replaced.
    Missing,
    Unconfirmed,
    /// How many blocks deep. `None` while the tip is not known.
    Confirmed(Option<u64>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxRow {
    pub txid: String,
    pub place: Place,
    /// What it is to a swap, and that swap's id and direction. `None`: no followed swap names it.
    pub role: Option<Role>,
    pub swap: Option<(String, Direction)>,
    /// The swap's TAC on Bitcoin, where this transaction carries it. Never read from the chain: an explorer cannot see it.
    pub tac: Option<u128>,
    /// What it added to the provider's addresses less what it spent from them. `None` when it is listed at neither.
    pub sats: Option<i128>,
    /// It pays the provider a note-sized output: TAC may ride on it, which takes the key to tell.
    pub note: bool,
    pub status: Status,
    pub rate_milli: Option<u64>,
    /// When its block was mined, or for an unconfirmed one when this run of tacmon first saw it.
    pub at: Option<u64>,
    /// Unconfirmed, and either waiting longer than the operator allows or paying under the going rate.
    pub stuck: bool,
    /// It spends from the reverse address: the reverse key sent it, and only then can the helper bump it.
    pub spends_reverse: bool,
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
    let mut listed: HashMap<&str, (Place, ChainTx, bool)> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for (place, list) in [(Place::Main, f.main), (Place::Reverse, f.reverse)] {
        for tx in list.unwrap_or_default() {
            let spends_reverse = place == Place::Reverse && tx.sent > 0;
            match listed.get_mut(tx.txid.as_str()) {
                Some((at, sum, spends)) => {
                    *at = if *at == place { place } else { Place::Both };
                    sum.received = sum.received.saturating_add(tx.received);
                    sum.sent = sum.sent.saturating_add(tx.sent);
                    sum.note |= tx.note;
                    *spends |= spends_reverse;
                }
                None => {
                    order.push(&tx.txid);
                    listed.insert(&tx.txid, (place, tx.clone(), spends_reverse));
                }
            }
        }
    }
    let row = |txid: &str, role: Option<Role>, swap: Option<&Swap>| {
        let at_address = listed.get(txid);
        let chain = at_address.map(|(_, tx, _)| tx).or_else(|| f.looked.get(txid).and_then(Option::as_ref));
        let status = match chain {
            Some(tx) => tx.height.map_or(Status::Unconfirmed, |h| Status::Confirmed(f.outlook.map(|o| o.tip.saturating_sub(h).saturating_add(1)))),
            None if f.looked.contains_key(txid) => Status::Missing,
            None => Status::Unknown,
        };
        let rate_milli = chain.and_then(ChainTx::rate_milli);
        let at = match status {
            Status::Unconfirmed => f.first_seen.get(txid).copied(),
            _ => chain.and_then(|tx| tx.time),
        };
        let slow = rate_milli.zip(f.outlook).is_some_and(|(rate, o)| rate < o.hour_milli);
        let waited = at.is_some_and(|t| f.now.saturating_sub(t) >= f.stuck_secs);
        TxRow {
            txid: txid.to_string(),
            place: at_address.map_or(Place::Elsewhere, |(place, _, _)| *place),
            role,
            swap: swap.map(|s| (s.id.clone(), s.direction)),
            // forward the TAC on Bitcoin is what the bridger pays, in reverse what it gets
            tac: swap.zip(role).filter(|(_, r)| r.carries_tac()).map(|(s, _)| if s.direction == Direction::Forward { s.pays } else { s.gets }),
            sats: at_address.map(|(_, tx, _)| i128::from(tx.received) - i128::from(tx.sent)),
            note: at_address.is_some_and(|(_, tx, _)| tx.note),
            status,
            rate_milli,
            at,
            stuck: status == Status::Unconfirmed && (slow || waited),
            spends_reverse: at_address.is_some_and(|(_, _, spends)| *spends),
        }
    };

    let mut seen = HashSet::new();
    let mut out: Vec<(u64, TxRow)> = Vec::new();
    for s in followed(f.swaps) {
        for (role, id) in &s.linked {
            if seen.insert(id.as_str()) {
                let r = row(id, Some(*role), Some(s));
                // one with no time of its own is placed by when its swap began
                out.push((r.at.unwrap_or(s.created), r));
            }
        }
    }
    for id in order {
        if seen.insert(id) {
            let r = row(id, None, None);
            out.push((r.at.unwrap_or(f.now), r));
        }
    }
    let rank = |r: &TxRow| match (r.status, r.stuck) {
        (Status::Unconfirmed, true) => 0,
        (Status::Unconfirmed, false) => 1,
        (Status::Missing, _) => 2,
        (Status::Unknown, _) => 3,
        (Status::Confirmed(_), _) => 4,
    };
    out.sort_by(|(a_at, a), (b_at, b)| rank(a).cmp(&rank(b)).then(b_at.cmp(a_at)));
    out.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(c: char) -> String {
        c.to_string().repeat(64)
    }
    fn tx(c: char, height: Option<u64>, received: u64, sent: u64) -> ChainTx {
        ChainTx { txid: id(c), fee: 430, vsize: 430, height, time: height.map(|h| h * 600), received, sent, note: received == 546 }
    }
    fn swap(n: u64, phase: &str, linked: &[(Role, char)]) -> Swap {
        Swap {
            id: format!("swap{n}"),
            phase: phase.into(),
            created: n,
            pays: 110,
            gets: 100,
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
    fn a_swaps_transactions_take_their_status_from_the_address_list_then_a_lookup_and_say_so_when_neither_has_one() {
        let swaps = [swap(1, "taking", &[(Role::BridgerLock, 'a'), (Role::TakeCommit, 'b'), (Role::Take, 'c'), (Role::Refund, 'd')])];
        let main = [tx('c', Some(100), 546, 0)];
        let looked = HashMap::from([(id('a'), Some(tx('a', Some(104), 0, 0))), (id('b'), None)]);
        let seen = HashMap::new();

        let rows = rows(&facts(&swaps, &main, &[], &looked, &seen));

        let by = |c: char| rows.iter().find(|r| r.txid == id(c)).expect("a row");
        assert_eq!((by('c').status, by('c').place, by('c').tac, by('c').sats), (Status::Confirmed(Some(6)), Place::Main, Some(110), Some(546)));
        assert_eq!((by('a').status, by('a').place, by('a').sats), (Status::Confirmed(Some(2)), Place::Elsewhere, None));
        assert_eq!((by('b').status, by('b').tac, by('d').status), (Status::Missing, None, Status::Unknown));
        assert!(rows.iter().all(|r| r.swap == Some(("swap1".into(), Direction::Forward))));
    }

    #[test]
    fn an_unconfirmed_transaction_is_stuck_when_it_pays_under_the_going_rate_or_has_waited_too_long() {
        let cheap = ChainTx { fee: 400, ..tx('a', None, 546, 0) };
        let (fair_new, fair_old, mined) = (tx('b', None, 9_000, 0), tx('c', None, 9_000, 0), ChainTx { fee: 1, ..tx('d', Some(105), 9_000, 0) });
        let reverse = [cheap, fair_new, fair_old, mined];
        let seen = HashMap::from([(id('b'), 9_990), (id('c'), 8_000)]);
        let (swaps, looked) = ([], HashMap::new());

        let rows = rows(&facts(&swaps, &[], &reverse, &looked, &seen));

        let got: Vec<(String, bool, Status)> = rows.iter().map(|r| (r.txid[..1].to_string(), r.stuck, r.status)).collect();
        // the stuck ones lead; one never seen before has no age and sorts as newest
        assert_eq!(
            got,
            [
                ("a".into(), true, Status::Unconfirmed),
                ("c".into(), true, Status::Unconfirmed),
                ("b".into(), false, Status::Unconfirmed),
                ("d".into(), false, Status::Confirmed(Some(1)))
            ]
        );
        assert_eq!((rows[0].rate_milli, rows[0].note, rows[0].role), (Some(930), true, None));
    }

    #[test]
    fn a_transfer_between_the_two_addresses_is_one_row_and_only_one_spending_from_the_reverse_address_is_the_reverse_keys() {
        let (main, reverse) = ([tx('a', None, 0, 20_000), tx('b', None, 5_000, 0)], [tx('a', None, 19_000, 0), tx('b', None, 0, 6_000)]);
        let (swaps, looked, seen) = ([], HashMap::new(), HashMap::new());

        let rows = rows(&facts(&swaps, &main, &reverse, &looked, &seen));

        let got: Vec<(Place, Option<i128>, bool)> = rows.iter().map(|r| (r.place, r.sats, r.spends_reverse)).collect();
        assert_eq!(got, [(Place::Both, Some(-1_000), false), (Place::Both, Some(-1_000), true)]);
    }

    #[test]
    fn open_swaps_are_followed_before_the_newest_and_no_more_than_the_limit() {
        let mut swaps: Vec<Swap> = (0..60).map(|n| swap(n, "done", &[])).collect();
        swaps[3] = Swap { linked: vec![(Role::BridgerLock, id('a')), (Role::Take, id('a'))], ..swap(3, "locking", &[]) };
        swaps[59].linked = vec![(Role::Take, id('b'))];
        swaps[0].linked = vec![(Role::Take, id('c'))];

        let (kept, ids) = (followed(&swaps), linked_ids(&swaps));

        assert_eq!((kept.len(), kept[0].id.as_str(), kept[1].id.as_str()), (MAX_SWAPS, "swap3", "swap59"));
        assert_eq!(ids, [id('a'), id('b')]);
    }
}
