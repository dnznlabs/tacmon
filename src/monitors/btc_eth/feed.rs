//! Drives the ports: one task per source, each sending what it read as an event. A read that fails is sent
//! as `None`, so the state can keep the last value and show its source as not answering.

use super::{
    config::Refresh,
    ledger,
    model::{Ad, ChainTx, Health, Info, LogEntry, Outlook, ReverseSide, Signal, Swap},
    ports::{Ports, QUEUE},
};
use crate::{
    monitors::common::{chain::Turns, feed::Out},
    shell::monitor::Wake,
};
use std::{collections::HashSet, time::Duration};
use tokio::{
    sync::{
        mpsc::{Receiver, channel},
        watch,
    },
    time::sleep,
};

#[derive(Debug)]
pub enum Event {
    Log(LogEntry),
    /// Boxed: with the books it is several times the size of any other event.
    Info(Option<Box<Info>>),
    Health(Option<Health>),
    Reverse(Option<ReverseSide>),
    Ads(Option<Vec<Ad>>),
    Swaps(Option<Vec<Swap>>),
    Eth(Option<f64>),
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
}

/// `pool_account` empty means: learn it from the log's start line. Must be called inside the runtime.
pub fn start(ports: Ports, refresh: &Refresh, pool_account: String, wake: Wake) -> Receiver<Event> {
    let (tx, rx) = channel(QUEUE);
    let out = Out { tx, wake };
    let Ports { provider, board, swaps, gas, chain, log } = ports;
    let learn = pool_account.is_empty();
    let (pool_tx, mut pool_rx) = watch::channel(pool_account);
    // the main address and the reverse one, as far as the log has named them
    let (addr_tx, mut addr_rx) = watch::channel((String::new(), String::new()));
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
                    // a provider that has started again says afresh whether it has a reverse address
                    addr_tx.send_modify(|a| *a = (id.bitcoin_address.clone(), String::new()));
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
            let (info, health, reverse) = tokio::join!(provider.info(), provider.health(), provider.reverse());
            if !o.send(Event::Info(info.ok().map(Box::new))).await || !o.send(Event::Health(health.ok())).await || !o.send(Event::Reverse(reverse.ok())).await {
                return;
            }
            sleep(every).await;
        }
    });

    // not under 5 s: the board is a shared public service
    let (o, every) = (out.clone(), Duration::from_secs(refresh.board_secs.max(5)));
    tokio::spawn(async move {
        loop {
            if !o.send(Event::Ads(board.ads().await.ok())).await {
                return;
            }
            sleep(every).await;
        }
    });

    let (o, every) = (out.clone(), Duration::from_secs(refresh.files_secs.max(1)));
    tokio::spawn(async move {
        loop {
            let read = swaps.swaps().await.ok();
            if let Some(s) = &read {
                let ids = ledger::linked_ids(s);
                linked_tx.send_if_modified(|was| {
                    let changed = *was != ids;
                    if changed {
                        *was = ids;
                    }
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
            let mut turns = Turns::default();
            loop {
                let (main, reverse) = addr_rx.borrow_and_update().clone();
                let wanted = linked_rx.borrow().clone();
                // an address that is not known is not asked about: an empty one would be a failed read, not no transactions
                let list = |address: String| {
                    let chain = &chain;
                    async move { if address.is_empty() { None } else { Some(chain.history(&address).await.ok()) } }
                };
                let (m, r, outlook) = tokio::join!(list(main), list(reverse), chain.outlook());
                let outlook = outlook.ok();
                // a transaction listed at an address needs no asking by id
                let listed: HashSet<String> = m.iter().chain(&r).flatten().flatten().map(|t| t.txid.clone()).collect();
                // what the explorer has already answered is not held back by the lookups, which can be slow
                for ev in [m.map(Event::MainTxs), r.map(Event::ReverseTxs), Some(Event::Outlook(outlook))].into_iter().flatten() {
                    if !o.send(ev).await {
                        return;
                    }
                }
                let (found, answered) = turns.ask(chain.as_ref(), &wanted, &listed, outlook).await;
                if !o.send(Event::Lookups { found, answered, wanted }).await {
                    return;
                }
                // an address learned meanwhile is read at once, not a whole interval later. A swap's new transaction
                // waits for the next round: swaps must not set how often a public explorer is asked
                let _ = tokio::time::timeout(every, addr_rx.changed()).await;
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
    use crate::monitors::btc_eth::{
        adapters::memory::{Memory, create_memory_ports},
        model::{Identity, Level, Role, Terms},
    };
    use std::sync::{Arc, atomic::AtomicBool};
    use tokio::sync::mpsc::unbounded_channel;

    #[tokio::test]
    async fn every_port_is_read_and_the_pool_account_and_the_addresses_are_learned_from_the_log() {
        let (main, reverse) = ("bc1qmain", "bc1qreverse");
        let id = |c: char| c.to_string().repeat(64);
        let tx = |c: char| ChainTx { txid: id(c), fee: 1, vsize: 1, ..Default::default() };
        let data = Memory {
            info: Info { terms: Terms { min: 1, max: 2, ..Default::default() }, books: None },
            eth: 0.25,
            // "a" is listed at the main address and so not asked about by id; "b" is known by id alone; "c" not at all
            swaps: vec![Swap { linked: vec![(Role::Take, id('a')), (Role::UserLock, id('b')), (Role::Refund, id('c'))], ..Default::default() }],
            txs: vec![(main.into(), vec![tx('a')]), (reverse.into(), vec![tx('r')])],
            lookups: vec![tx('a'), tx('b')],
            outlook: Outlook { tip: 9, hour_milli: 1_000 },
            ..Default::default()
        };
        let started = LogEntry {
            signal: Some(Signal::Started(Identity { pool_account: format!("0x{}", "ab".repeat(20)), bitcoin_address: main.into(), ..Default::default() })),
            ..LogEntry::plain(Level::Info, "fast bridge LP")
        };
        let reverse_on = LogEntry { signal: Some(Signal::ReverseOn(reverse.into())), ..LogEntry::plain(Level::Info, "the reverse side is on") };
        let (wake_tx, _wake_rx) = unbounded_channel();
        let wake = Wake::new(wake_tx, Arc::new(AtomicBool::new(false)));

        let mut rx = start(create_memory_ports(data, vec![started, reverse_on]), &Refresh::default(), String::new(), wake);

        const FAILED: usize = 11;
        let mut seen = [false; FAILED];
        while seen.iter().any(|s| !s) {
            let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.expect("an event in time").expect("the feed");
            let at = match ev {
                Event::Log(_) => 0,
                Event::Info(i) => i.map_or(FAILED, |_| 1),
                Event::Health(h) => h.map_or(FAILED, |_| 2),
                Event::Reverse(r) => r.map_or(FAILED, |_| 3),
                Event::Ads(a) => a.map_or(FAILED, |_| 4),
                Event::Swaps(s) => s.map_or(FAILED, |s| if s.len() == 1 { 5 } else { FAILED }),
                Event::Eth(e) => e.map_or(FAILED, |e| if e == 0.25 { 6 } else { FAILED }),
                Event::MainTxs(t) => t.map_or(FAILED, |t| if t == [tx('a')] { 7 } else { FAILED }),
                Event::ReverseTxs(t) => t.map_or(FAILED, |t| if t == [tx('r')] { 8 } else { FAILED }),
                Event::Outlook(o) => o.map_or(FAILED, |o| if o.tip == 9 { 9 } else { FAILED }),
                // a round before the records were read wants nothing, and one before the log named the main address
                // has no list to find "a" in: neither is the round this test waits for
                Event::Lookups { wanted, found, .. } if wanted.is_empty() || found.iter().any(|(asked, _)| *asked == id('a')) => continue,
                Event::Lookups { found, answered, wanted } => {
                    if answered && wanted.len() == 3 && found == [(id('b'), Some(tx('b'))), (id('c'), None)] {
                        10
                    } else {
                        FAILED
                    }
                }
            };
            assert!(at < FAILED, "a read failed or read something else");
            seen[at] = true;
        }
    }
}
