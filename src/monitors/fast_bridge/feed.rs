//! Drives the ports: one task per source, each sending what it read as an event. A read that fails is sent
//! as `None`, so the state can keep the last value and show its source as not answering.

use super::{
    config::Refresh,
    ledger,
    model::{Ad, ChainTx, Coin, Health, LogEntry, Outlook, PoolScan, ReverseSide, ReverseStock, Signal, Swap, Terms, Want},
    ports::{PoolLog, Ports, QUEUE},
};
use crate::shell::monitor::Wake;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

/// The most transactions asked about by id in one round. The explorer is a shared public service, and a provider
/// with a long history catches up over a few rounds.
const MAX_LOOKUPS: usize = 12;

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
    let Ports { provider, board, swaps, gas, chain, pool, inventory, log } = ports;
    let learn = pool_account.is_empty();
    let (pool_tx, mut pool_rx) = watch::channel(pool_account);
    // the main address and the reverse one, as far as they are known
    let (addr_tx, mut addr_rx) = watch::channel((String::new(), reverse_address));
    // the transactions the swap records name, for the chain task to ask about
    let (linked_tx, linked_rx) = watch::channel(Vec::<String>::new());

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
                // what the explorer has already answered is not held back by the lookups, which can be slow
                if (!main.is_empty() && !o.send(Event::MainTxs(m)).await)
                    || (!reverse.is_empty() && !o.send(Event::ReverseTxs(r)).await)
                    || !o.send(Event::Outlook(outlook)).await
                {
                    return;
                }
                let wanted = linked_rx.borrow().clone();
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
                if !o.send(Event::Lookups { found, answered, wanted }).await {
                    return;
                }
                // an address learned meanwhile is read at once, not a whole interval later
                let _ = tokio::time::timeout(every, addr_rx.changed()).await;
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
            txs: vec![("bc1qmain".into(), vec![ChainTx { txid: "c".repeat(64), ..Default::default() }])],
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
                has(|e| matches!(e, Event::MainTxs(Some(t)) if t.len() == 1)),
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

        assert_eq!(wanted(&seen), [true; 16]);
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
}
