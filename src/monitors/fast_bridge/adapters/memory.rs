//! Every port answered from memory: the backend for tests and for `--demo`. It reads nothing and sends nothing.

use crate::monitors::fast_bridge::{
    model::{Ad, Capacity, Health, Identity, Level, LogEntry, Signal, Swap, Terms, UNIT, Want},
    ports::{Board, Fut, GasBalance, LogFeed, Ports, Provider, QUEUE, SwapStore},
};
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, channel};

#[derive(Debug, Clone, Default)]
pub struct Memory {
    pub terms: Terms,
    pub health: Health,
    pub wants: Vec<Want>,
    pub ads: Vec<Ad>,
    pub swaps: Vec<Swap>,
    pub eth: f64,
}

impl Provider for Memory {
    fn terms(&self) -> Fut<'_, Terms> {
        Box::pin(async move { Ok(self.terms.clone()) })
    }
    fn health(&self) -> Fut<'_, Health> {
        Box::pin(async move { Ok(self.health.clone()) })
    }
}

impl Board for Memory {
    fn wants(&self) -> Fut<'_, Vec<Want>> {
        Box::pin(async move { Ok(self.wants.clone()) })
    }
    fn ads(&self) -> Fut<'_, Vec<Ad>> {
        Box::pin(async move { Ok(self.ads.clone()) })
    }
}

impl SwapStore for Memory {
    fn swaps(&self) -> Fut<'_, Vec<Swap>> {
        Box::pin(async move { Ok(self.swaps.clone()) })
    }
}

impl GasBalance for Memory {
    fn eth<'a>(&'a self, _account: &'a str) -> Fut<'a, f64> {
        Box::pin(async move { Ok(self.eth) })
    }
}

/// Gives its entries once, then stays open and silent, as a quiet provider's log does.
pub struct MemoryLog(pub Vec<LogEntry>);

impl LogFeed for MemoryLog {
    fn start(self: Box<Self>) -> Receiver<LogEntry> {
        let (tx, rx) = channel(QUEUE);
        tokio::spawn(async move {
            for e in self.0 {
                if tx.send(e).await.is_err() {
                    return;
                }
            }
            tx.closed().await;
        });
        rx
    }
}

pub fn create_memory_ports(data: Memory, log: Vec<LogEntry>) -> Ports {
    let m = Arc::new(data);
    Ports { provider: m.clone(), board: m.clone(), swaps: m.clone(), gas: Some(m), log: Box::new(MemoryLog(log)) }
}

/// A made-up provider mid-shift, for `--demo`. Every key, address and transaction id is invented.
pub fn create_demo_ports(now: u64) -> Ports {
    let t = |v: u128| v * UNIT;
    let terms = Terms { min: t(10), max: t(1000), fee_bps: 100, flat: UNIT / 10, available: Some(t(1840)) };
    let me = "02a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
    let swap = |id: &str, phase: &str, age: u64, gets: u128| Swap {
        id: format!("{id}{}", "0".repeat(56)),
        phase: phase.into(),
        created: now - age,
        deadline: now - age + 14_400,
        pays: gets * 10_000 / 9_900 + UNIT / 10,
        gets,
        lock_txid: format!("{}{id}", "f".repeat(56)),
    };
    let ad = |name: &str, key: &str, bps: u64, max: u128, avail: u128, any_tac: bool| Ad {
        name: name.into(),
        claim_pub: key.into(),
        terms: Terms { min: t(10), max: t(max), fee_bps: bps, flat: UNIT / 10, available: Some(t(avail)) },
        any_tac,
        reverse: false,
    };
    let data = Memory {
        health: Health { ok: true, error: String::new() },
        wants: vec![
            Want { amount: t(3400), max_fee_bps: None, until: now + 30_000 },
            Want { amount: t(2000), max_fee_bps: Some(200), until: now + 80_000 },
            Want { amount: t(1000), max_fee_bps: Some(50), until: now + 52_000 },
            Want { amount: t(620), max_fee_bps: Some(100), until: now + 24_000 },
            Want { amount: t(180), max_fee_bps: None, until: now + 70_000 },
            Want { amount: t(60), max_fee_bps: None, until: now + 15_000 },
        ],
        ads: vec![ad("demo-lp", me, 100, 1000, 1840, true), ad("harbor", "03aa", 30, 300, 0, false), ad("north", "03bb", 150, 5000, 420, true)],
        swaps: vec![
            swap("a1b2c3d4", "offered", 1_300, t(249)),
            swap("e5f6a7b8", "locking", 200, t(500)),
            swap("c9d0e1f2", "done", 9_000, t(120)),
            swap("0a1b2c3d", "done", 21_000, t(75)),
        ],
        eth: 0.0412,
        terms,
    };
    let entry =
        |time: &str, level, msg: &str, detail: &str, signal| LogEntry { time: time.into(), at: None, level, msg: msg.into(), detail: detail.into(), signal };
    let log = vec![
        entry(
            "09:00:01",
            Level::Info,
            "fast bridge LP",
            "",
            Some(Signal::Started(Identity {
                bitcoin_address: "bc1qdemo0000000000000000000000000000000demo".into(),
                claim_pub: me.into(),
                pool_account: "0x00000000000000000000000000000000000d3m0".into(),
                dry_run: false,
            })),
        ),
        entry("09:00:02", Level::Info, "on the board", "board demo", None),
        entry("09:14:20", Level::Info, "offer accepted", "id c9d0e1f2…0000", None),
        entry("09:31:07", Level::Info, "swap", "c9d0e1f2…0000  taking → done", None),
        entry("10:02:44", Level::Warn, "an explorer is left alone for a while", "explorer demo  why HTTP 429  forSecs 20", None),
        entry(
            "10:02:59",
            Level::Info,
            "capacity",
            "held 2,589.00  promised 749.00  open 2  holds 0",
            Some(Signal::Capacity(Capacity { held: t(2589), promised: t(749), open: 2, holds: 0 })),
        ),
        entry("10:03:10", Level::Info, "swap", "a1b2c3d4…0000  locked → offered", None),
    ];
    create_memory_ports(data, log)
}
