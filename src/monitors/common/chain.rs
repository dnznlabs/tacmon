//! What an explorer's answers make of a Bitcoin transaction: how far along it is, and whether it is stuck. And whose
//! turn it is to be asked about, when a provider's records name more transactions than one round should ask for.
//! Arithmetic and bookkeeping only: the one read here goes through the `Chain` port.

use super::{
    model::{ChainTx, Outlook},
    ports::Chain,
};
use std::collections::{HashMap, HashSet};

/// How many swaps' transactions are followed: the open ones first, then the newest. It bounds what an explorer is asked.
pub const MAX_SWAPS: usize = 50;
/// Blocks after which a transaction is settled, and not asked about again.
pub const SETTLED: u64 = 6;
/// A fee rate under this share of the hour estimate is slow. Under it by anything is not: two explorers asked in
/// the same minute give estimates several times apart.
pub const SLOW_PERCENT: u64 = 80;
/// How long a slow transaction is given before it is called stuck: about a block.
pub const GRACE_SECS: u64 = 600;
/// The most transactions asked about by id in one round. The explorer is a shared public service, and a provider
/// with a long history catches up over a few rounds.
pub const MAX_LOOKUPS: usize = 12;

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

/// What is known of one transaction now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    pub status: Status,
    pub rate_milli: Option<u64>,
    /// When its block was mined, or for an unconfirmed one when this run of tacmon first saw it.
    pub at: Option<u64>,
    /// Unconfirmed, and paying well under the going rate.
    pub slow: bool,
    /// Unconfirmed, and either waiting longer than the operator allows, or slow and past its grace.
    pub stuck: bool,
}

/// How many blocks deep a confirmed transaction is, its own block counted. `None` unconfirmed, or without a tip.
pub fn depth(tx: &ChainTx, outlook: Option<Outlook>) -> Option<u64> {
    tx.height.zip(outlook).map(|(h, o)| o.tip.saturating_sub(h).saturating_add(1))
}

/// `tx` is what an explorer gave for the transaction, by an address's list or by its id. `asked` says whether it was
/// asked about by id at all: without an answer, that is the difference between one the explorer does not know and one
/// nobody has asked about. `first_seen` is when this run first saw it unconfirmed.
pub fn standing(tx: Option<&ChainTx>, asked: bool, outlook: Option<Outlook>, first_seen: Option<u64>, now: u64, stuck_secs: u64) -> Standing {
    let status = match tx {
        Some(tx) => tx.height.map_or(Status::Unconfirmed, |_| Status::Confirmed(depth(tx, outlook))),
        None if asked => Status::Missing,
        None => Status::Unknown,
    };
    let rate_milli = tx.and_then(ChainTx::rate_milli);
    let unconfirmed = status == Status::Unconfirmed;
    let at = if unconfirmed { first_seen } else { tx.and_then(|tx| tx.time) };
    let slow = unconfirmed && rate_milli.zip(outlook).is_some_and(|(rate, o)| rate.saturating_mul(100) < o.hour_milli.saturating_mul(SLOW_PERCENT));
    let seen_for = |secs: u64| at.is_some_and(|t| now.saturating_sub(t) >= secs);
    Standing { status, rate_milli, at, slow, stuck: unconfirmed && (seen_for(stuck_secs) || (slow && seen_for(GRACE_SECS))) }
}

/// Where a transaction sorts on a page: stuck first, then the other unconfirmed, then what an explorer has not
/// placed, then confirmed.
pub fn rank(s: &Standing) -> u8 {
    match (s.status, s.stuck) {
        (Status::Unconfirmed, true) => 0,
        (Status::Unconfirmed, false) => 1,
        (Status::Missing, _) => 2,
        (Status::Unknown, _) => 3,
        (Status::Confirmed(_), _) => 4,
    }
}

/// Whose turn it is to be asked about by id. One answered as settled is not asked again; the rest take turns, the
/// one asked longest ago first.
#[derive(Default)]
pub struct Turns {
    settled: HashSet<String>,
    asked: HashMap<String, u64>,
    round: u64,
}

impl Turns {
    /// Asks about up to `MAX_LOOKUPS` of `wanted`, leaving out what an address's list already gave (`listed`).
    /// Gives each answer (`None` beside a transaction the explorer does not know) and whether every one asked was
    /// answered. What is no longer wanted is forgotten: whoever keeps the answers must forget the same ids.
    pub async fn ask(
        &mut self,
        chain: &dyn Chain,
        wanted: &[String],
        listed: &HashSet<String>,
        outlook: Option<Outlook>,
    ) -> (Vec<(String, Option<ChainTx>)>, bool) {
        self.round += 1;
        self.settled.retain(|id| wanted.contains(id));
        self.asked.retain(|id, _| wanted.contains(id));
        let mut ask: Vec<&String> = wanted.iter().filter(|id| !listed.contains(*id) && !self.settled.contains(*id)).collect();
        ask.sort_by_key(|id| self.asked.get(*id).copied().unwrap_or(0));
        let (mut found, mut answered) = (Vec::new(), true);
        for id in ask.into_iter().take(MAX_LOOKUPS) {
            match chain.tx(id).await {
                Ok(tx) => {
                    if tx.as_ref().and_then(|t| depth(t, outlook)).is_some_and(|d| d >= SETTLED) {
                        self.settled.insert(id.clone());
                    }
                    found.push((id.clone(), tx));
                }
                Err(_) => answered = false,
            }
            self.asked.insert(id.clone(), self.round);
        }
        (found, answered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::common::{model::Coin, ports::Fut};
    use std::sync::Mutex;

    fn id(c: char) -> String {
        c.to_string().repeat(64)
    }
    fn tx(c: char, height: Option<u64>, fee: u64) -> ChainTx {
        ChainTx { txid: id(c), fee, vsize: 100, height, time: height.map(|h| h * 600), ..Default::default() }
    }
    const OUTLOOK: Option<Outlook> = Some(Outlook { tip: 105, hour_milli: 2_000 });

    #[test]
    fn a_transaction_is_missing_only_when_asked_about_and_confirmed_by_its_depth_where_the_tip_is_known() {
        let deep = tx('a', Some(100), 200);

        let read = [
            standing(None, false, OUTLOOK, None, 0, 1_800).status,
            standing(None, true, OUTLOOK, None, 0, 1_800).status,
            standing(Some(&deep), false, OUTLOOK, None, 0, 1_800).status,
            standing(Some(&deep), false, None, None, 0, 1_800).status,
        ];

        assert_eq!(read, [Status::Unknown, Status::Missing, Status::Confirmed(Some(6)), Status::Confirmed(None)]);
    }

    #[test]
    fn an_unconfirmed_transaction_is_stuck_past_the_operators_wait_or_slow_and_past_its_grace() {
        // 2 sat/vB is the going rate: 1.5 is under four fifths of it, 1.7 is not
        let (slow, fair) = (tx('a', None, 150), tx('b', None, 170));
        let seen = |tx: &ChainTx, since: u64| standing(Some(tx), false, OUTLOOK, Some(10_000 - since), 10_000, 1_800);

        let read = [seen(&slow, 599), seen(&slow, 600), seen(&fair, 1_799), seen(&fair, 1_800)].map(|s| (s.slow, s.stuck));

        assert_eq!(read, [(true, false), (true, true), (false, false), (false, true)]);
        // one never seen before has no age, and is not stuck on the strength of that
        assert!(!standing(Some(&slow), false, OUTLOOK, None, 10_000, 1_800).stuck);
    }

    /// An explorer that knows `known` and counts what it is asked.
    struct Explorer {
        known: Vec<ChainTx>,
        asked: Mutex<Vec<String>>,
    }
    impl Chain for Explorer {
        fn unspent<'a>(&'a self, _: &'a str) -> Fut<'a, Vec<Coin>> {
            Box::pin(async { Ok(Vec::new()) })
        }
        fn history<'a>(&'a self, _: &'a str) -> Fut<'a, Vec<ChainTx>> {
            Box::pin(async { Ok(Vec::new()) })
        }
        fn tx<'a>(&'a self, txid: &'a str) -> Fut<'a, Option<ChainTx>> {
            Box::pin(async move {
                self.asked.lock().expect("the list").push(txid.to_string());
                Ok(self.known.iter().find(|t| t.txid == txid).cloned())
            })
        }
        fn spent<'a>(&'a self, _: &'a str, _: u32) -> Fut<'a, bool> {
            Box::pin(async { Ok(false) })
        }
        fn outlook(&self) -> Fut<'_, Outlook> {
            Box::pin(async { Ok(Outlook::default()) })
        }
    }

    #[tokio::test]
    async fn a_settled_transaction_is_not_asked_about_again_and_one_listed_at_an_address_never_is() {
        let chain = Explorer { known: vec![tx('a', Some(100), 200), tx('b', Some(104), 200)], asked: Mutex::new(Vec::new()) };
        let (wanted, listed) = ([id('a'), id('b'), id('c'), id('d')], HashSet::from([id('d')]));
        let mut turns = Turns::default();

        let first = turns.ask(&chain, &wanted, &listed, OUTLOOK).await;
        let second = turns.ask(&chain, &wanted, &listed, OUTLOOK).await;

        assert_eq!(
            first.0.iter().map(|(id, tx)| (id.as_str(), tx.is_some())).collect::<Vec<_>>(),
            [(id('a').as_str(), true), (id('b').as_str(), true), (id('c').as_str(), false)]
        );
        // "a" is six deep and settled; "b" is two deep and "c" unknown, so both are asked again
        assert_eq!(second.0.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(), [id('b'), id('c')]);
        assert!(first.1 && second.1 && !chain.asked.lock().expect("the list").contains(&id('d')));
    }
}
