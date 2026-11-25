//! Drives the ports: one task per source, each sending what it read as an event. A read that fails is sent
//! as `None`, so the state can keep the last value and show its source as not answering.

use super::{
    config::Refresh,
    ledger,
    model::{Ad, ChainTx, Coin, Health, LogEntry, Outlook, ReverseSide, ReverseStock, Signal, Swap, Terms, Want},
    ports::{Ports, QUEUE},
};
use crate::shell::monitor::Wake;
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

/// The most transactions asked about by id in one round. The explorer is a shared public service, and a provider
/// with a long history catches up over a few rounds.
const MAX_LOOKUPS: usize = 12;
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
    /// Swap transactions asked about by id this round: `None` beside one the explorer does not know. The flag is
    /// whether every one asked was answered.
    Lookups(Vec<(String, Option<ChainTx>)>, bool),
    Outlook(Option<Outlook>),
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
    let Ports { provider, board, swaps, gas, chain, inventory, log } = ports;
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
                let listed: HashSet<&str> = m.iter().chain(&r).flatten().map(|t| t.txid.as_str()).collect();
                let wanted = linked_rx.borrow().clone();
                settled.retain(|id| wanted.contains(id));
                asked.retain(|id, _| wanted.contains(id));
                let mut ask: Vec<&String> = wanted.iter().filter(|id| !listed.contains(id.as_str()) && !settled.contains(*id)).collect();
                ask.sort_by_key(|id| asked.get(*id).copied().unwrap_or(0));
                let (mut found, mut answered) = (Vec::new(), true);
                for id in ask.into_iter().take(MAX_LOOKUPS) {
                    match chain.tx(id).await {
                        Ok(tx) => {
                            let deep = |t: &ChainTx| t.height.zip(outlook).is_some_and(|(h, o)| o.tip.saturating_sub(h).saturating_add(1) >= ledger::SETTLED);
                            if tx.as_ref().is_some_and(deep) {
                                settled.insert(id.clone());
                            }
                            found.push((id.clone(), tx));
                        }
                        Err(_) => answered = false,
                    }
                    asked.insert(id.clone(), round);
                }
                drop(listed);
                if (!main.is_empty() && !o.send(Event::MainTxs(m)).await)
                    || (!reverse.is_empty() && !o.send(Event::ReverseTxs(r)).await)
                    || !o.send(Event::Outlook(outlook)).await
                    || !o.send(Event::Lookups(found, answered)).await
                {
                    return;
                }
                // an address learned meanwhile is read at once, not a whole interval later
                let _ = tokio::time::timeout(every, addr_rx.changed()).await;
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
        model::{Identity, Level, Role},
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
                has(|e| matches!(e, Event::Lookups(l, true) if l.len() == 1 && l[0].0 == "b".repeat(64))),
            ]
        };

        while !wanted(&seen).iter().all(|x| *x) && seen.len() < 80 {
            seen.push(tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.expect("an event in time").expect("the feed is running"));
        }

        assert_eq!(wanted(&seen), [true; 15]);
    }
}
