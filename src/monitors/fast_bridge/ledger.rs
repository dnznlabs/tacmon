//! The Bitcoin transactions a provider's swaps and addresses depend on, as rows, and which of them are stuck.
//! Arithmetic only: no state, no I/O.

use super::model::{ChainTx, Direction, Outlook, PoolLock, PoolScan, Role, Swap};
pub use crate::monitors::common::chain::{MAX_SWAPS, SETTLED, Status};
use crate::monitors::common::chain::{Standing, rank, standing};
use std::collections::{HashMap, HashSet};

/// How far back the pool's log on Ethereum is read, in blocks: a day. A swap is open for hours.
pub const POOL_LOOKBACK: u64 = 7_200;
/// Blocks after which Ethereum does not take a transaction back: two epochs.
pub const POOL_SETTLED: u64 = 64;
/// Ethereum makes a block every twelve seconds, or skips one: never sooner.
const BLOCK_SECS: u64 = 12;

/// How many of the key's verdicts on notes sent away are kept, oldest out first. The feed and the state keep the
/// same ones by this rule, so neither forgets a verdict the other still holds.
pub const SILENT_KEPT: usize = 512;
/// How deep a transaction must be before the key-holding tool is asked about it: a block on top of its own, so the
/// tool's explorer has it and a "none of it is this key's" is said of a transaction that is there to read.
pub const SILENT_DEPTH: u64 = 2;

/// Which of the provider's addresses a transaction pays or spends from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Main,
    Reverse,
    /// Listed at both: it moved coins from one of the provider's addresses to the other.
    Both,
    /// Listed at neither: known through a swap record only.
    Elsewhere,
    /// On Ethereum, in the pool contract.
    Pool,
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
    /// Unconfirmed, and paying well under the going rate.
    pub slow: bool,
    /// Unconfirmed, and either waiting longer than the operator allows, or slow and past its grace.
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
    /// The pool's log on Ethereum. `None`: not read.
    pub pool: Option<&'a PoolScan>,
    pub now: u64,
    pub stuck_secs: u64,
}

/// The lock a forward swap put into the pool, where the log that was read shows it.
fn pool_lock<'a>(scan: &'a PoolScan, s: &Swap) -> Option<&'a PoolLock> {
    (!s.pool_leaf.is_empty()).then(|| scan.locks.iter().rev().find(|l| l.leaf == s.pool_leaf)).flatten()
}

/// A swap still making its pool lock, and how long it has been.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockWait {
    pub swap: String,
    pub secs: u64,
}

/// A forward swap that is making its pool lock: the record names the leaf, and the lock is not known to have landed.
pub fn locking(s: &Swap) -> bool {
    s.direction == Direction::Forward && s.phase == "locking" && !s.pool_leaf.is_empty()
}

/// The followed forward swaps that have been making their pool lock for `stuck_secs` or longer while the pool's log
/// shows none. `since` is when each was first seen doing so: a swap is not late for the time its Bitcoin leg took.
/// Only where the log was read back to before the swap began: a lock older than the read is not missing.
pub fn lock_waits(swaps: &[Swap], scan: &PoolScan, since: &HashMap<String, u64>, now: u64, stuck_secs: u64) -> Vec<LockWait> {
    followed(swaps)
        .into_iter()
        .filter(|s| locking(s))
        .filter_map(|s| {
            let secs = now.saturating_sub(*since.get(&s.id)?);
            // at most this many blocks since the swap began, so a read that reaches further back has all of them
            let covered = scan.since <= scan.tip.saturating_sub(now.saturating_sub(s.created) / BLOCK_SECS + 1);
            (secs >= stuck_secs && covered && pool_lock(scan, s).is_none()).then(|| LockWait { swap: s.id.clone(), secs })
        })
        .collect()
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
        let Standing { status, rate_milli, at, slow, stuck } =
            standing(chain, f.looked.contains_key(txid), f.outlook, f.first_seen.get(txid).copied(), f.now, f.stuck_secs);
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
            slow,
            stuck,
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
        if let Some((scan, lock)) = f.pool.and_then(|scan| Some((scan, pool_lock(scan, s)?))) {
            let r = TxRow {
                txid: lock.tx.clone(),
                place: Place::Pool,
                role: Some(Role::PoolLock),
                swap: Some((s.id.clone(), s.direction)),
                // what the provider locked for the bridger, by the swap record: the pool hides amounts
                tac: Some(s.gets),
                sats: None,
                note: false,
                status: Status::Confirmed(Some(scan.tip.saturating_sub(lock.block).saturating_add(1))),
                rate_milli: None,
                at: lock.time,
                slow: false,
                stuck: false,
                spends_reverse: false,
            };
            // a node that gives no time for the block: placed by its depth, which is no younger than this
            out.push((lock.time.unwrap_or_else(|| f.now.saturating_sub(scan.tip.saturating_sub(lock.block).saturating_mul(BLOCK_SECS))), r));
        }
    }
    for id in order {
        if seen.insert(id) {
            let r = row(id, None, None);
            out.push((r.at.unwrap_or(f.now), r));
        }
    }
    let rank = |r: &TxRow| rank(&Standing { status: r.status, rate_milli: r.rate_milli, at: r.at, slow: r.slow, stuck: r.stuck });
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
        ChainTx { txid: id(c), fee: 430, vsize: 430, height, time: height.map(|h| h * 600), received, sent, note: received == 546, notes_away: Vec::new() }
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
            pool: None,
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
    fn stuck_takes_a_rate_well_under_the_going_one_and_a_block_of_waiting_or_a_long_wait_alone() {
        // the going rate is 1.000: 0.79 is slow, 0.81 is within what two explorers disagree by
        let at = |c: char, fee: u64| ChainTx { fee, ..tx(c, None, 546, 0) };
        let reverse = [at('a', 340), at('b', 340), at('c', 349), at('d', 430), ChainTx { fee: 1, ..tx('e', Some(105), 9_000, 0) }];
        let seen = HashMap::from([(id('a'), 9_990), (id('b'), 9_000), (id('c'), 9_000), (id('d'), 8_000)]);
        let (swaps, looked) = ([], HashMap::new());

        let rows = rows(&facts(&swaps, &[], &reverse, &looked, &seen));

        let got: Vec<(String, bool, bool)> = rows.iter().map(|r| (r.txid[..1].to_string(), r.slow, r.stuck)).collect();
        // slow and just seen is not stuck yet; a fair rate is stuck only once it has waited past the operator's limit
        assert_eq!(
            got,
            [("b".into(), true, true), ("d".into(), false, true), ("a".into(), true, false), ("c".into(), false, false), ("e".into(), false, false)]
        );
        assert_eq!((rows[0].rate_milli, rows[0].note, rows[0].role, rows[4].status), (Some(790), true, None, Status::Confirmed(Some(1))));
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

    fn lock(leaf: char, tx: char, block: u64) -> PoolLock {
        PoolLock { leaf: id(leaf), tx: format!("0x{}", id(tx)), block, time: Some(block * 12) }
    }

    #[test]
    fn a_forward_swaps_pool_lock_is_a_row_with_its_depth_and_the_swaps_own_amount() {
        let swaps = [Swap { pool_leaf: id('7'), ..swap(1, "offered", &[]) }, Swap { pool_leaf: id('8'), ..swap(2, "locking", &[]) }];
        // another provider's lock is in the same log, and is nobody's row here
        let scan = PoolScan { tip: 109, since: 0, locks: vec![lock('7', 'e', 100), lock('5', 'f', 101)] };
        let (looked, seen) = (HashMap::new(), HashMap::new());

        let rows = rows(&Facts { pool: Some(&scan), ..facts(&swaps, &[], &[], &looked, &seen) });

        let got: Vec<_> = rows.iter().map(|r| (r.txid.as_str(), r.place, r.role, r.tac, r.status, r.at, r.stuck)).collect();
        assert_eq!(got, [(format!("0x{}", id('e')).as_str(), Place::Pool, Some(Role::PoolLock), Some(100), Status::Confirmed(Some(10)), Some(1_200), false)]);
    }

    #[test]
    fn a_lock_is_only_called_missing_after_a_long_wait_and_where_the_log_was_read_back_to_before_the_swap() {
        let making = |n: u64, leaf: char| Swap { pool_leaf: id(leaf), ..swap(n, "locking", &[]) };
        // began 2,000 s before now, 1,000 s, and 100 s: the last has not waited long
        let swaps = [making(8_000, '1'), making(9_000, '2'), making(9_900, '3'), Swap { phase: "offered".into(), ..making(8_000, '4') }];
        let since: HashMap<String, u64> = swaps.iter().map(|s| (s.id.clone(), s.created)).collect();
        let read = |since| PoolScan { tip: 1_000, since, locks: vec![lock('2', 'e', 990)] };

        // 2,000 s is at most 167 blocks: a read from block 800 has them all, one from block 900 does not
        let (far, near) = (lock_waits(&swaps, &read(800), &since, 10_000, 900), lock_waits(&swaps, &read(900), &since, 10_000, 900));

        assert_eq!((far, near), (vec![LockWait { swap: "swap8000".into(), secs: 2_000 }], vec![]));
    }

    #[test]
    fn a_lock_is_timed_from_when_the_swap_began_locking_and_not_from_when_the_swap_began() {
        // the bridger's Bitcoin lock took an hour to confirm: the swap is old, its pool lock is a minute old
        let swaps = [Swap { pool_leaf: id('1'), ..swap(5_000, "locking", &[]) }];
        let scan = PoolScan { tip: 1_000, since: 0, locks: vec![] };
        let seen = |at: u64| HashMap::from([("swap5000".to_string(), at)]);

        let (fresh, late, unseen) = (
            lock_waits(&swaps, &scan, &seen(9_940), 10_000, 900),
            lock_waits(&swaps, &scan, &seen(9_000), 10_000, 900),
            lock_waits(&swaps, &scan, &HashMap::new(), 10_000, 900),
        );

        assert_eq!((fresh, late, unseen), (vec![], vec![LockWait { swap: "swap5000".into(), secs: 1_000 }], vec![]));
    }
}
