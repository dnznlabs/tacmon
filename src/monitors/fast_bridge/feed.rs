//! Drives the ports: one task per source, each sending what it read as an event. A read that fails is sent
//! as `None`, so the state can keep the last value and show its source as not answering.

use super::{
    config::Refresh,
    ledger,
    model::{Ad, ChainTx, Coin, Health, LogEntry, Outlook, PoolScan, ReverseSide, ReverseStock, Signal, Swap, Terms, Want},
    ports::{PoolLog, Ports, QUEUE, SilentCheck},
};
use crate::shell::monitor::Wake;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

/// The most transactions asked about by id in one round. The explorer is a shared public service, and a provider
/// with a long history catches up over a few rounds.
const MAX_LOOKUPS: usize = 12;
/// The most transactions whose notes sent away are asked about in one round.
const MAX_AWAY: usize = 6;
/// How long before a transaction the key-holding tool could not judge is put to it again.
const SILENT_RETRY: Duration = Duration::from_secs(600);

/// The shortest reach back the first read of the pool's log is cut to when a node refuses a longer one. Free public
/// nodes allow from 50 blocks to a day and a half.
const MIN_REACH: u64 = 32;
/// How many of the newest blocks are read again each round: one taken back by a reorganisation takes its locks with it.
const REREAD: u64 = 12;
/// The most locks kept. A day of the whole pool's is under a hundred.
const MAX_POOL_LOCKS: usize = 4096;
use tokio::{
    sync::{
        mpsc::{Receiver, Sender, channel},
        watch,
    },
    time::sleep,
};

#[derive(Debug)]
pub enum Event {
    Log(LogEntry),
    Terms(Option<Terms>),
    Health(Option<Health>),
    Reverse(Option<ReverseSide>),
    Wants(Option<Vec<Want>>),
    Ads(Option<Vec<Ad>>),
    Swaps(Option<Vec<Swap>>),
    Eth(Option<f64>),
    /// What is unspent at the provider's main address, and at its reverse address.
    MainCoins(Option<Vec<Coin>>),
    ReverseCoins(Option<Vec<Coin>>),
    /// What is at the reverse address, read by the configured status command.
    Stock(Option<ReverseStock>),
    /// The latest transactions at the main address, and at the reverse one.
    MainTxs(Option<Vec<ChainTx>>),
    ReverseTxs(Option<Vec<ChainTx>>),
    /// Swap transactions asked about by id this round: `None` beside one the explorer does not know.
    Lookups {
        found: Vec<(String, Option<ChainTx>)>,
        /// Whether every one asked was answered.
        answered: bool,
        /// Every id the round held as wanted. The feed forgets what it knew of any other id at this moment, so the
        /// state must too: an answer kept by one and dropped by the other is never asked for again.
        wanted: Vec<String>,
    },
    Outlook(Option<Outlook>),
    /// Transactions at the main address that sent a note-sized output elsewhere, and whether any such output of each
    /// is still unspent. Only the ones the explorer answered for.
    NotesAway(Vec<(String, bool)>),
    /// What the key says of one of those: the TAC it paid the reverse key silently, or `None` when none of it did.
    Silent(String, Option<u128>),
    /// The end of a pass of the key-holding tool over them: the transaction it gave no answer for, or `None` when it
    /// answered for every one.
    SilentRead(Option<String>),
    /// The pool's log on Ethereum as far as it has been read, whole each time.
    Pool(Option<PoolScan>),
}

/// What has been read of the pool's log, and where the next read starts.
struct PoolReader {
    scan: PoolScan,
    /// The block to read from next. `None`: nothing read yet.
    next: Option<u64>,
    /// How far back a first read reaches: halved each time a node refuses it, and never by a request that got no answer.
    reach: u64,
}

impl Default for PoolReader {
    fn default() -> Self {
        Self { scan: PoolScan::default(), next: None, reach: ledger::POOL_LOOKBACK }
    }
}

impl PoolReader {
    /// One round: the tip, then the log from where the last read ended. `None` when the node did not answer.
    async fn read(&mut self, pool: &dyn PoolLog) -> Option<PoolScan> {
        let tip = pool.tip().await.ok()?;
        // never read, or away for longer than is kept: start over from the tip
        let fresh = self.next.is_none_or(|n| tip.saturating_sub(n) > ledger::POOL_LOOKBACK);
        let from = if fresh { tip.saturating_sub(self.reach) } else { self.next.unwrap_or(tip).saturating_sub(REREAD).max(self.scan.since).min(tip) };
        let found = match pool.locks(from, tip).await {
            Ok(Some(found)) => found,
            // refused: a first read reaches half as far next time. One from where the last ended was refused for the
            // blocks that have piled up since, and they only pile higher: it starts over, at a reach the node allowed
            Ok(None) => {
                if fresh {
                    self.reach = (self.reach / 2).max(MIN_REACH);
                }
                self.next = None;
                return None;
            }
            // no answer says nothing about what the node allows
            Err(_) => return None,
        };
        let scan = &mut self.scan;
        if fresh {
            *scan = PoolScan { since: from, ..Default::default() };
        }
        scan.since = scan.since.max(tip.saturating_sub(ledger::POOL_LOOKBACK));
        // what was read again replaces what was held for those blocks
        scan.locks.retain(|l| l.block < from && l.block >= scan.since);
        scan.locks.extend(found.into_iter().filter(|l| (from..=tip).contains(&l.block)));
        if let Some(over) = scan.locks.len().checked_sub(MAX_POOL_LOCKS).filter(|n| *n > 0) {
            scan.locks.drain(..over);
            scan.since = scan.locks.first().map_or(tip, |l| l.block + 1);
        }
        scan.tip = tip;
        self.next = Some(tip + 1);
        Some(scan.clone())
    }
}

/// Whose turn it is to be asked whether its note sent away has been spent: the one asked longest ago first, and one
/// known to have moved never again.
#[derive(Default)]
struct AwayTurns {
    asked: HashMap<String, u64>,
    moved: HashSet<String>,
    /// The ones last answered as still unspent.
    unspent: HashSet<String>,
}

impl AwayTurns {
    /// The candidates to ask about this round, `MAX_AWAY` at most. What is no longer a candidate is forgotten.
    fn pick(&mut self, candidates: &[String], round: u64) -> Vec<String> {
        for known in [&mut self.moved, &mut self.unspent] {
            known.retain(|id| candidates.contains(id));
        }
        self.asked.retain(|id, _| candidates.contains(id));
        let mut ask: Vec<&String> = candidates.iter().filter(|id| !self.moved.contains(*id)).collect();
        ask.sort_by_key(|id| self.asked.get(*id).copied().unwrap_or(0));
        let ask: Vec<String> = ask.into_iter().take(MAX_AWAY).cloned().collect();
        for id in &ask {
            self.asked.insert(id.clone(), round);
        }
        ask
    }
    /// What the explorer said of one: whether any of its notes is still unspent.
    fn answer(&mut self, id: &str, unspent: bool) {
        if unspent {
            self.unspent.insert(id.to_string());
        } else {
            self.unspent.remove(id);
            self.moved.insert(id.to_string());
        }
    }
}

/// The ids the key has given a verdict on, `ledger::SILENT_KEPT` at most, the oldest forgotten first.
#[derive(Default)]
struct Judged {
    order: std::collections::VecDeque<String>,
    known: HashSet<String>,
}

impl Judged {
    fn insert(&mut self, id: &str) {
        if self.known.insert(id.to_string()) {
            self.order.push_back(id.to_string());
            if self.order.len() > ledger::SILENT_KEPT
                && let Some(old) = self.order.pop_front()
            {
                self.known.remove(&old);
            }
        }
    }
}

/// One pass of the key-holding tool over the ids it has not judged, every one of them: the verdicts it gave, and the
/// id it gave none for. It stops at that one: a tool that cannot run is not started again for each of the rest.
async fn judge(silent: &dyn SilentCheck, ids: &[String], judged: &mut Judged) -> (Vec<(String, Option<u128>)>, Option<String>) {
    let mut verdicts = Vec::new();
    for id in ids.iter().filter(|id| !judged.known.contains(*id)).cloned().collect::<Vec<_>>() {
        match silent.waiting(&id).await {
            Ok(tac) => {
                judged.insert(&id);
                verdicts.push((id, tac));
            }
            Err(_) => return (verdicts, Some(id)),
        }
    }
    (verdicts, None)
}

#[derive(Clone)]
struct Out {
    tx: Sender<Event>,
    wake: Wake,
}

impl Out {
    /// `false` once the monitor has gone, which ends the task that asked. Waits while the queue is full.
    async fn send(&self, ev: Event) -> bool {
        // woken first as well: a reader waiting on a full queue needs the shell to come and empty it
        self.wake.wake();
        let ok = self.tx.send(ev).await.is_ok();
        self.wake.wake();
        ok
    }
}

/// `pool_account` empty means: learn it from the log's start line. `reverse_address` is the configured one, until the
/// log names the address the side actually sells from. Must be called inside the runtime.
pub fn start(ports: Ports, refresh: &Refresh, pool_account: String, reverse_address: String, wake: Wake) -> Receiver<Event> {
    let (tx, rx) = channel(QUEUE);
    let out = Out { tx, wake };
    let Ports { provider, board, swaps, gas, chain, pool, inventory, silent, log } = ports;
    let learn = pool_account.is_empty();
    let (pool_tx, mut pool_rx) = watch::channel(pool_account);
    // the main address and the reverse one, as far as they are known
    let (addr_tx, mut addr_rx) = watch::channel((String::new(), reverse_address));
    // the transactions the swap records name, for the chain task to ask about
    let (linked_tx, linked_rx) = watch::channel(Vec::<String>::new());
    // the transactions whose note sent away is still unspent, for the key to say what they are
    let (away_tx, mut away_rx) = watch::channel(Vec::<String>::new());

    let o = out.clone();
    tokio::spawn(async move {
        let mut entries = log.start();
        while let Some(entry) = entries.recv().await {
            match &entry.signal {
                Some(Signal::Started(id)) => {
                    if learn {
                        let _ = pool_tx.send(id.pool_account.clone());
                    }
                    addr_tx.send_modify(|a| a.0.clone_from(&id.bitcoin_address));
                }
                Some(Signal::ReverseOn(address)) => addr_tx.send_modify(|a| a.1.clone_from(address)),
                _ => {}
            }
            if !o.send(Event::Log(entry)).await {
                return;
            }
        }
        // the log has ended, but the gas and chain tasks still need what it learned from it
        o.tx.closed().await;
        drop((pool_tx, addr_tx));
    });

    let (o, every) = (out.clone(), Duration::from_secs(refresh.lp_secs.max(1)));
    tokio::spawn(async move {
        loop {
            let (terms, health, reverse) = tokio::join!(provider.terms(), provider.health(), provider.reverse());
            if !o.send(Event::Terms(terms.ok())).await || !o.send(Event::Health(health.ok())).await || !o.send(Event::Reverse(reverse.ok())).await {
                return;
            }
            sleep(every).await;
        }
    });

    // not under 5 s: the board is a shared public service
    let (o, every) = (out.clone(), Duration::from_secs(refresh.board_secs.max(5)));
    tokio::spawn(async move {
        loop {
            let (wants, ads) = tokio::join!(board.wants(), board.ads());
            if !o.send(Event::Wants(wants.ok())).await || !o.send(Event::Ads(ads.ok())).await {
                return;
            }
            sleep(every).await;
        }
    });

    let (o, every) = (out.clone(), Duration::from_secs(refresh.files_secs.max(1)));
    tokio::spawn(async move {
        loop {
            let read = swaps.swaps().await.ok();
            if let Some(read) = &read {
                linked_tx.send_if_modified(|ids| {
                    let now = ledger::linked_ids(read);
                    let changed = *ids != now;
                    *ids = now;
                    changed
                });
            }
            if !o.send(Event::Swaps(read)).await {
                return;
            }
            sleep(every).await;
        }
    });

    if let Some(chain) = chain {
        // not under 30 s: a public explorer, asked about addresses that change a few times an hour
        let (o, every) = (out.clone(), Duration::from_secs(refresh.chain_secs.max(30)));
        tokio::spawn(async move {
            // ids answered as settled are not asked again; the rest take turns, the one asked longest ago first
            let (mut settled, mut asked, mut round) = (HashSet::<String>::new(), HashMap::<String, u64>::new(), 0u64);
            let mut away = AwayTurns::default();
            loop {
                round += 1;
                let (main, reverse) = addr_rx.borrow_and_update().clone();
                if main.is_empty() && reverse.is_empty() {
                    if addr_rx.changed().await.is_err() {
                        return;
                    }
                    continue;
                }
                let (m, r) = tokio::join!(chain.unspent(&main), chain.unspent(&reverse));
                if (!main.is_empty() && !o.send(Event::MainCoins(m.ok())).await) || (!reverse.is_empty() && !o.send(Event::ReverseCoins(r.ok())).await) {
                    return;
                }
                let (m, r, outlook) = tokio::join!(chain.history(&main), chain.history(&reverse), chain.outlook());
                let (m, r, outlook) = (m.ok(), r.ok(), outlook.ok());
                // a transaction listed at an address needs no asking by id
                let listed: HashSet<String> = m.iter().chain(&r).flatten().map(|t| t.txid.clone()).collect();
                let (m_kept, r_kept) = (m.clone(), r.clone());
                // what the explorer has already answered is not held back by the lookups, which can be slow
                if (!main.is_empty() && !o.send(Event::MainTxs(m)).await)
                    || (!reverse.is_empty() && !o.send(Event::ReverseTxs(r)).await)
                    || !o.send(Event::Outlook(outlook)).await
                {
                    return;
                }
                let (m, r) = (m_kept, r_kept);
                let wanted = linked_rx.borrow().clone();
                // a note that left the main address for neither a swap nor the reverse address: is it still where it went?
                // Only once it is a block deep: the tool that judges it reads it from an explorer of its own
                let at_reverse: HashSet<&str> = r.iter().flatten().map(|t| t.txid.as_str()).collect();
                let deep = |t: &ChainTx| t.height.zip(outlook).is_some_and(|(h, o)| o.tip.saturating_sub(h).saturating_add(1) >= ledger::SILENT_DEPTH);
                let candidates: Vec<&ChainTx> = m
                    .iter()
                    .flatten()
                    .filter(|t| !t.notes_away.is_empty() && deep(t) && !at_reverse.contains(t.txid.as_str()) && !wanted.contains(&t.txid))
                    .collect();
                let mut waiting = Vec::new();
                // a list that was not read says nothing of what has stopped being a candidate
                if m.is_some() && outlook.is_some() {
                    let ids: Vec<String> = candidates.iter().map(|t| t.txid.clone()).collect();
                    for id in away.pick(&ids, round) {
                        let Some(t) = candidates.iter().find(|t| t.txid == id) else { continue };
                        let mut unspent = Some(false);
                        for vout in &t.notes_away {
                            unspent = match (unspent, chain.spent(&t.txid, *vout).await) {
                                (Some(any), Ok(spent)) => Some(any || !spent),
                                _ => None,
                            };
                        }
                        if let Some(unspent) = unspent {
                            away.answer(&id, unspent);
                            waiting.push((id, unspent));
                        }
                    }
                    // the ones known to be unspent, for the key to say what they are
                    let now: Vec<String> = ids.into_iter().filter(|id| away.unspent.contains(id)).collect();
                    away_tx.send_if_modified(|ids| {
                        let changed = *ids != now;
                        *ids = now;
                        changed
                    });
                }
                settled.retain(|id| wanted.contains(id));
                asked.retain(|id, _| wanted.contains(id));
                let mut ask: Vec<String> = wanted.iter().filter(|id| !listed.contains(*id) && !settled.contains(*id)).cloned().collect();
                ask.sort_by_key(|id| asked.get(id).copied().unwrap_or(0));
                let (mut found, mut answered) = (Vec::new(), true);
                for id in ask.into_iter().take(MAX_LOOKUPS) {
                    match chain.tx(&id).await {
                        Ok(tx) => {
                            let deep = |t: &ChainTx| t.height.zip(outlook).is_some_and(|(h, o)| o.tip.saturating_sub(h).saturating_add(1) >= ledger::SETTLED);
                            if tx.as_ref().is_some_and(deep) {
                                settled.insert(id.clone());
                            }
                            found.push((id.clone(), tx));
                        }
                        Err(_) => answered = false,
                    }
                    asked.insert(id, round);
                }
                if !o.send(Event::NotesAway(waiting)).await || !o.send(Event::Lookups { found, answered, wanted }).await {
                    return;
                }
                // an address learned meanwhile is read at once, not a whole interval later
                let _ = tokio::time::timeout(every, addr_rx.changed()).await;
            }
        });
    }

    if let Some(silent) = silent {
        let o = out.clone();
        tokio::spawn(async move {
            // each transaction is judged once: what it paid, and to whom, does not change
            let mut judged = Judged::default();
            loop {
                let ids = away_rx.borrow_and_update().clone();
                let (verdicts, unread) = judge(silent.as_ref(), &ids, &mut judged).await;
                for (id, tac) in verdicts {
                    if !o.send(Event::Silent(id, tac)).await {
                        return;
                    }
                }
                let failed = unread.is_some();
                if !o.send(Event::SilentRead(unread)).await {
                    return;
                }
                // one the tool gave no answer for is put to it again, and not at once: each run scans with the key
                let gone =
                    if failed { matches!(tokio::time::timeout(SILENT_RETRY, away_rx.changed()).await, Ok(Err(_))) } else { away_rx.changed().await.is_err() };
                if gone {
                    return;
                }
            }
        });
    }

    if let Some(pool) = pool {
        // not under 30 s: a block comes every 12 s, and the node is a shared public one
        let (o, every) = (out.clone(), Duration::from_secs(refresh.eth_secs.max(30)));
        tokio::spawn(async move {
            let mut reader = PoolReader::default();
            loop {
                if !o.send(Event::Pool(reader.read(pool.as_ref()).await)).await {
                    return;
                }
                sleep(every).await;
            }
        });
    }

    if let Some(inventory) = inventory {
        // not under a minute: each run scans the pool, and holds the key while it does
        let (o, every) = (out.clone(), Duration::from_secs(refresh.status_secs.max(60)));
        tokio::spawn(async move {
            loop {
                if !o.send(Event::Stock(inventory.reverse_stock().await.ok())).await {
                    return;
                }
                sleep(every).await;
            }
        });
    }

    if let Some(gas) = gas {
        let (o, every) = (out, Duration::from_secs(refresh.eth_secs.max(10)));
        tokio::spawn(async move {
            loop {
                let account = pool_rx.borrow().clone();
                if account.is_empty() {
                    if pool_rx.changed().await.is_err() {
                        return;
                    }
                    continue;
                }
                if !o.send(Event::Eth(gas.eth(&account).await.ok())).await {
                    return;
                }
                sleep(every).await;
            }
        });
    }
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::fast_bridge::{
        adapters::memory::{Memory, create_memory_ports},
        model::{Identity, Level, PoolLock, Role},
        ports::Fut,
    };

    #[tokio::test]
    async fn every_port_is_read_and_the_addresses_are_learned_from_the_log() {
        let coin = Coin { txid: "a".repeat(64), vout: 0, sats: 9_000, confirmed: true };
        let data = Memory {
            eth: 0.5,
            wants: vec![Want { btc_asset: None, amount: 7, max_fee_bps: None, until: 0 }],
            coins: vec![("bc1qmain".into(), vec![coin.clone()]), ("bc1qrev".into(), vec![coin])],
            swaps: vec![Swap { linked: vec![(Role::BridgerLock, "b".repeat(64)), (Role::Take, "c".repeat(64))], ..Default::default() }],
            txs: vec![(
                "bc1qmain".into(),
                vec![
                    ChainTx { txid: "c".repeat(64), ..Default::default() },
                    ChainTx { txid: "7".repeat(64), notes_away: vec![0, 1], height: Some(5), ..Default::default() },
                    ChainTx { txid: "8".repeat(64), notes_away: vec![0], height: Some(5), ..Default::default() },
                    // not a block deep yet: not asked about, by the explorer or by the key
                    ChainTx { txid: "9".repeat(64), notes_away: vec![0], height: Some(9), ..Default::default() },
                ],
            )],
            // one of the first one's two notes has moved, and the second one's only note has
            spent: vec![("7".repeat(64), 1), ("8".repeat(64), 0)],
            silent: vec![("7".repeat(64), 200)],
            lookups: vec![ChainTx { txid: "b".repeat(64), height: Some(1), ..Default::default() }],
            outlook: Outlook { tip: 9, hour_milli: 1 },
            eth_tip: 50,
            pool_locks: vec![PoolLock { leaf: "d".repeat(64), tx: format!("0x{}", "e".repeat(64)), block: 40, time: None }],
            ..Default::default()
        };
        let started = LogEntry {
            signal: Some(Signal::Started(Identity { pool_account: "0xpool".into(), bitcoin_address: "bc1qmain".into(), ..Default::default() })),
            ..LogEntry::plain(Level::Info, "fast bridge LP")
        };
        let reverse_on = LogEntry { signal: Some(Signal::ReverseOn("bc1qrev".into())), ..LogEntry::plain(Level::Info, "the reverse side is on") };
        let (wake_tx, _wake_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut rx = start(
            create_memory_ports(data, vec![started, reverse_on]),
            &Refresh::default(),
            String::new(),
            String::new(),
            Wake::new(wake_tx, Default::default()),
        );
        let mut seen = Vec::new();
        let wanted = |seen: &[Event]| {
            let has = |f: fn(&Event) -> bool| seen.iter().any(f);
            [
                has(|e| matches!(e, Event::Log(l) if l.msg == "fast bridge LP")),
                has(|e| matches!(e, Event::Terms(Some(_)))),
                has(|e| matches!(e, Event::Health(Some(_)))),
                has(|e| matches!(e, Event::Reverse(Some(ReverseSide::Off)))),
                has(|e| matches!(e, Event::Wants(Some(w)) if w.len() == 1)),
                has(|e| matches!(e, Event::Ads(Some(_)))),
                has(|e| matches!(e, Event::Swaps(Some(_)))),
                has(|e| matches!(e, Event::Eth(Some(v)) if *v == 0.5)),
                has(|e| matches!(e, Event::MainCoins(Some(c)) if c.len() == 1)),
                has(|e| matches!(e, Event::ReverseCoins(Some(c)) if c.len() == 1)),
                has(|e| matches!(e, Event::Stock(Some(_)))),
                has(|e| matches!(e, Event::MainTxs(Some(t)) if t.len() == 4)),
                has(|e| matches!(e, Event::SilentRead(None))),
                has(|e| matches!(e, Event::NotesAway(w) if *w == [("7".repeat(64), true), ("8".repeat(64), false)])),
                has(|e| matches!(e, Event::Silent(id, Some(200)) if *id == "7".repeat(64))),
                has(|e| matches!(e, Event::ReverseTxs(Some(t)) if t.is_empty())),
                has(|e| matches!(e, Event::Outlook(Some(o)) if o.tip == 9)),
                // the take is listed at the main address, so only the bridger's lock is asked about by id
                has(|e| matches!(e, Event::Lookups { found, answered: true, wanted } if found.len() == 1 && found[0].0 == "b".repeat(64) && wanted.len() == 2)),
                has(|e| matches!(e, Event::Pool(Some(p)) if (p.tip, p.since, p.locks.len()) == (50, 0, 1))),
            ]
        };

        while !wanted(&seen).iter().all(|x| *x) && seen.len() < 80 {
            seen.push(tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.expect("an event in time").expect("the feed is running"));
        }

        assert_eq!(wanted(&seen), [true; 19]);
    }

    /// A node that refuses a read of more than `most` blocks, does not answer while `down`, and notes every range it was asked for.
    struct Narrow {
        tip: std::sync::atomic::AtomicU64,
        most: u64,
        down: std::sync::atomic::AtomicBool,
        asked: std::sync::Mutex<Vec<(u64, u64)>>,
    }

    fn narrow(tip: u64, most: u64) -> Narrow {
        Narrow { tip: tip.into(), most, down: false.into(), asked: Default::default() }
    }

    impl PoolLog for Narrow {
        fn tip(&self) -> Fut<'_, u64> {
            Box::pin(async move { Ok(self.tip.load(std::sync::atomic::Ordering::Relaxed)) })
        }
        fn locks(&self, from: u64, to: u64) -> Fut<'_, Option<Vec<PoolLock>>> {
            Box::pin(async move {
                self.asked.lock().expect("the list").push((from, to));
                anyhow::ensure!(!self.down.load(std::sync::atomic::Ordering::Relaxed), "no answer");
                if to - from > self.most {
                    return Ok(None);
                }
                // one lock in the range, and one outside it that an honest node would not send
                Ok(Some(vec![
                    PoolLock { leaf: "a".repeat(64), block: to, ..Default::default() },
                    PoolLock { leaf: "b".repeat(64), block: from - 1, ..Default::default() },
                ]))
            })
        }
    }

    async fn rounds(reader: &mut PoolReader, node: &Narrow, n: usize) -> Vec<Option<(u64, usize)>> {
        let mut reads = Vec::new();
        for _ in 0..n {
            reads.push(reader.read(node).await.map(|p| (p.since, p.locks.len())));
        }
        reads
    }

    #[tokio::test]
    async fn a_node_that_refuses_a_days_log_is_asked_for_half_as_much_each_round_and_then_only_for_the_newest_blocks() {
        let node = narrow(100_000, 2_000);
        let mut reader = PoolReader::default();

        let reads = rounds(&mut reader, &node, 4).await;

        // a day, half, a quarter (which the node allows), then the newest blocks again: the lock read twice is held once
        assert_eq!(*node.asked.lock().expect("the list"), [(92_800, 100_000), (96_400, 100_000), (98_200, 100_000), (99_989, 100_000)]);
        assert_eq!(reads, [None, None, Some((98_200, 1)), Some((98_200, 1))]);
    }

    #[tokio::test]
    async fn after_a_gap_wider_than_the_node_allows_the_read_starts_over_and_does_not_ask_for_the_same_blocks_for_a_day() {
        use std::sync::atomic::Ordering::Relaxed;
        // allows 50 blocks: the first read gets down to a reach of 32
        let node = narrow(100_000, 50);
        let mut reader = PoolReader::default();
        let _ = rounds(&mut reader, &node, 9).await;
        node.asked.lock().expect("the list").clear();

        // twenty minutes with the lid shut
        node.tip.store(100_100, Relaxed);
        let reads = rounds(&mut reader, &node, 3).await;

        assert_eq!(*node.asked.lock().expect("the list"), [(99_989, 100_100), (100_068, 100_100), (100_089, 100_100)]);
        assert_eq!(reads, [None, Some((100_068, 1)), Some((100_068, 1))]);
    }

    #[tokio::test]
    async fn a_node_that_does_not_answer_costs_no_reach() {
        use std::sync::atomic::Ordering::Relaxed;
        let node = narrow(100_000, 10_000);
        let mut reader = PoolReader::default();
        node.down.store(true, Relaxed);
        let failed = rounds(&mut reader, &node, 8).await;

        node.down.store(false, Relaxed);
        let reads = rounds(&mut reader, &node, 1).await;

        assert_eq!((failed, reads), (vec![None; 8], vec![Some((92_800, 1))]));
    }

    /// A key-holding tool that gives no answer for the ids in `broken`.
    struct Tool {
        broken: Vec<String>,
        started: std::sync::Mutex<Vec<String>>,
    }

    impl SilentCheck for Tool {
        fn waiting<'a>(&'a self, txid: &'a str) -> Fut<'a, Option<u128>> {
            Box::pin(async move {
                self.started.lock().expect("the list").push(txid.to_string());
                anyhow::ensure!(!self.broken.iter().any(|b| b == txid), "no answer");
                Ok(Some(7))
            })
        }
    }

    #[test]
    fn every_note_sent_away_gets_its_turn_however_many_there_are_and_one_that_has_moved_is_not_asked_again() {
        let ids: Vec<String> = (0..8).map(|n| n.to_string().repeat(64)).collect();
        let mut turns = AwayTurns::default();

        let first = turns.pick(&ids, 1);
        // the first two have been spent, the next four are held by whoever got them
        for (n, id) in first.iter().enumerate() {
            turns.answer(id, n >= 2);
        }
        let second = turns.pick(&ids, 2);

        assert_eq!((first.as_slice(), &second[..2]), (&ids[..6], &ids[6..]));
        assert!(second.len() == MAX_AWAY && !second.contains(&ids[0]) && !second.contains(&ids[1]));
    }

    #[tokio::test]
    async fn the_key_judges_every_candidate_in_a_pass_once_each_and_stops_at_the_first_it_gets_no_answer_for() {
        let ids: Vec<String> = (0..8).map(|n| n.to_string().repeat(64)).collect();
        let (working, broken) = (Tool { broken: vec![], started: Default::default() }, Tool { broken: vec![ids[2].clone()], started: Default::default() });
        let (mut all, mut some) = (Judged::default(), Judged::default());

        let (first, again) = (judge(&working, &ids, &mut all).await, judge(&working, &ids, &mut all).await);
        let stopped = judge(&broken, &ids, &mut some).await;

        assert_eq!((first.0.len(), first.1, again.0.len(), working.started.lock().expect("the list").len()), (8, None, 0, 8));
        assert_eq!((stopped.0.len(), stopped.1, broken.started.lock().expect("the list").len()), (2, Some(ids[2].clone()), 3));
    }
}
