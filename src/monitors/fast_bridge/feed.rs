//! Drives the ports: one task per source, each sending what it read as an event. A read that fails is sent
//! as `None`, so the state can keep the last value and show its source as not answering.

use super::{
    config::Refresh,
    model::{Ad, Health, LogEntry, Signal, Swap, Terms, Want},
    ports::{Ports, QUEUE},
};
use crate::shell::monitor::Wake;
use std::time::Duration;
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
    Wants(Option<Vec<Want>>),
    Ads(Option<Vec<Ad>>),
    Swaps(Option<Vec<Swap>>),
    Eth(Option<f64>),
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

/// `pool_account` empty means: learn it from the log's start line. Must be called inside the runtime.
pub fn start(ports: Ports, refresh: &Refresh, pool_account: String, wake: Wake) -> Receiver<Event> {
    let (tx, rx) = channel(QUEUE);
    let out = Out { tx, wake };
    let Ports { provider, board, swaps, gas, log } = ports;
    let learn = pool_account.is_empty();
    let (pool_tx, mut pool_rx) = watch::channel(pool_account);

    let o = out.clone();
    tokio::spawn(async move {
        let mut entries = log.start();
        while let Some(entry) = entries.recv().await {
            if learn && let Some(Signal::Started(id)) = &entry.signal {
                let _ = pool_tx.send(id.pool_account.clone());
            }
            if !o.send(Event::Log(entry)).await {
                return;
            }
        }
        // the log has ended, but the gas task still needs the account it learned from it
        o.tx.closed().await;
        drop(pool_tx);
    });

    let (o, every) = (out.clone(), Duration::from_secs(refresh.lp_secs.max(1)));
    tokio::spawn(async move {
        loop {
            let (terms, health) = tokio::join!(provider.terms(), provider.health());
            if !o.send(Event::Terms(terms.ok())).await || !o.send(Event::Health(health.ok())).await {
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
            if !o.send(Event::Swaps(swaps.swaps().await.ok())).await {
                return;
            }
            sleep(every).await;
        }
    });

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
        model::{Identity, Level},
    };

    #[tokio::test]
    async fn every_port_is_read_and_the_pool_account_is_learned_from_the_log() {
        let data = Memory { eth: 0.5, wants: vec![Want { btc_asset: None, amount: 7, max_fee_bps: None, until: 0 }], ..Default::default() };
        let started = LogEntry {
            signal: Some(Signal::Started(Identity { pool_account: "0xpool".into(), ..Default::default() })),
            ..LogEntry::plain(Level::Info, "fast bridge LP")
        };
        let (wake_tx, _wake_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut rx = start(create_memory_ports(data, vec![started]), &Refresh::default(), String::new(), Wake::new(wake_tx, Default::default()));
        let mut seen = Vec::new();
        while seen.len() < 7 {
            seen.push(rx.recv().await.expect("the feed is running"));
        }

        let has = |f: fn(&Event) -> bool| seen.iter().any(f);
        assert!(has(|e| matches!(e, Event::Log(l) if l.msg == "fast bridge LP")));
        assert!(has(|e| matches!(e, Event::Terms(Some(_)))));
        assert!(has(|e| matches!(e, Event::Health(Some(_)))));
        assert!(has(|e| matches!(e, Event::Wants(Some(w)) if w.len() == 1)));
        assert!(has(|e| matches!(e, Event::Ads(Some(_)))));
        assert!(has(|e| matches!(e, Event::Swaps(Some(_)))));
        assert!(has(|e| matches!(e, Event::Eth(Some(v)) if *v == 0.5)));
    }
}
