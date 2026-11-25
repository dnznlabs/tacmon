//! Every port answered from memory: the backend for tests and for `--demo`. It reads nothing and sends nothing.

use crate::monitors::fast_bridge::{
    model::{Ad, Capacity, Coin, DUST, Direction, Health, Identity, Level, LogEntry, ReverseSide, ReverseStock, Signal, Swap, Terms, UNIT, Want},
    ports::{Board, Chain, Fut, GasBalance, Inventory, LogFeed, Ports, Provider, QUEUE, SwapStore},
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
    pub reverse: ReverseSide,
    /// What is unspent at each address. An address not listed holds nothing.
    pub coins: Vec<(String, Vec<Coin>)>,
    pub stock: ReverseStock,
}

impl Provider for Memory {
    fn terms(&self) -> Fut<'_, Terms> {
        Box::pin(async move { Ok(self.terms.clone()) })
    }
    fn health(&self) -> Fut<'_, Health> {
        Box::pin(async move { Ok(self.health.clone()) })
    }
    fn reverse(&self) -> Fut<'_, ReverseSide> {
        Box::pin(async move { Ok(self.reverse.clone()) })
    }
}

impl Inventory for Memory {
    fn reverse_stock(&self) -> Fut<'_, ReverseStock> {
        Box::pin(async move { Ok(self.stock.clone()) })
    }
}

impl Chain for Memory {
    fn unspent<'a>(&'a self, address: &'a str) -> Fut<'a, Vec<Coin>> {
        Box::pin(async move { Ok(self.coins.iter().find(|(a, _)| a == address).map(|(_, c)| c.clone()).unwrap_or_default()) })
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
    Ports {
        provider: m.clone(),
        board: m.clone(),
        swaps: m.clone(),
        gas: Some(m.clone()),
        chain: Some(m.clone()),
        inventory: Some(m),
        log: Box::new(MemoryLog(log)),
    }
}

/// A made-up provider mid-shift, for `--demo`. Every key, address and transaction id is invented.
pub fn create_demo_ports(now: u64) -> Ports {
    let t = |v: u128| v * UNIT;
    let terms = Terms { min: t(10), max: t(1000), fee_bps: 100, flat: UNIT / 10, available: Some(t(1840)), ..Default::default() };
    let me = "02a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
    let swap = |id: &str, phase: &str, age: u64, gets: u128| Swap {
        direction: Direction::Forward,
        id: format!("{id}{}", "0".repeat(56)),
        phase: phase.into(),
        created: now - age,
        deadline: now - age + 14_400,
        pays: gets * 10_000 / 9_900 + UNIT / 10,
        gets,
        lock_txid: format!("{}{id}", "f".repeat(56)),
        take_txid: if phase == "done" { format!("{}{id}", "e".repeat(56)) } else { String::new() },
    };
    // the bridger pays pool TAC and gets a note on Bitcoin; no deadline until it has offered its pool lock
    let reverse_swap = |id: &str, phase: &str, age: u64, gets: u128| Swap {
        direction: Direction::Reverse,
        deadline: if matches!(phase, "built" | "broadcast") { 0 } else { now - age + 14_400 },
        pays: gets * 10_000 / 9_970 + UNIT / 10,
        take_txid: String::new(),
        ..swap(id, phase, age, gets)
    };
    let reverse_terms = Terms { min: t(10), max: t(1000), fee_bps: 30, flat: UNIT / 10, available: Some(t(4)), ..Default::default() };
    let (main, reverse) = ("bc1qdemo0000000000000000000000000000000demo", "bc1qdemo00000000000000000000000000000reverse");
    let coin = |txid: String, vout, sats| Coin { txid, vout, sats, confirmed: true };
    let take = |id: &str| format!("{}{id}", "e".repeat(56));
    let ad = |name: &str, key: &str, bps: u64, max: u128, avail: u128, any_tac: bool, reverse_terms: Option<Terms>| Ad {
        name: name.into(),
        claim_pub: key.into(),
        terms: Terms { min: t(10), max: t(max), fee_bps: bps, flat: UNIT / 10, available: Some(t(avail)), ..Default::default() },
        any_tac,
        reverse: reverse_terms.is_some(),
        reverse_terms,
    };
    let data = Memory {
        health: Health { ok: true, error: String::new() },
        wants: vec![
            Want { btc_asset: None, amount: t(3400), max_fee_bps: None, until: now + 30_000 },
            Want { btc_asset: None, amount: t(2000), max_fee_bps: Some(200), until: now + 80_000 },
            Want { btc_asset: None, amount: t(1000), max_fee_bps: Some(50), until: now + 52_000 },
            Want { btc_asset: None, amount: t(620), max_fee_bps: Some(100), until: now + 24_000 },
            Want { btc_asset: None, amount: t(180), max_fee_bps: None, until: now + 70_000 },
            Want { btc_asset: None, amount: t(60), max_fee_bps: None, until: now + 15_000 },
        ],
        ads: vec![
            ad("demo-lp", me, 100, 1000, 1840, true, Some(reverse_terms.clone())),
            ad("harbor", "03aa", 30, 300, 0, false, None),
            ad("north", "03bb", 150, 5000, 420, true, Some(Terms { fee_bps: 50, available: Some(t(900)), ..reverse_terms.clone() })),
        ],
        swaps: vec![
            swap("a1b2c3d4", "offered", 1_300, t(249)),
            swap("e5f6a7b8", "locking", 200, t(500)),
            swap("c9d0e1f2", "done", 9_000, t(120)),
            swap("0a1b2c3d", "done", 21_000, t(75)),
            reverse_swap("b7c8d9e0", "claiming", 900, t(40)),
            reverse_swap("f1e2d3c4", "done", 15_000, t(60)),
        ],
        eth: 0.0412,
        reverse: ReverseSide::On { terms: reverse_terms, open: Some(0), max_open: 2 },
        coins: vec![
            (
                main.into(),
                vec![
                    coin(take("c9d0e1f2"), 0, DUST),
                    coin(take("c9d0e1f2"), 1, DUST),
                    coin(take("0a1b2c3d"), 0, DUST),
                    coin(take("0a1b2c3d"), 1, DUST),
                    coin(format!("{}a1b2c3d4", "f".repeat(56)), 3, 2_046),
                    coin(format!("{}12345678", "d".repeat(56)), 1, 19_500),
                    coin(format!("{}9abcdef0", "d".repeat(56)), 0, 50_000),
                ],
            ),
            (reverse.into(), vec![coin(format!("{}0f0f0f0f", "c".repeat(56)), 0, 6_200), coin(format!("{}0f0f0f0f", "c".repeat(56)), 1, DUST)]),
        ],
        stock: ReverseStock { address: reverse.into(), notes: 1, tac: t(4), sats: Some(6_200) },
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
                bitcoin_address: main.into(),
                claim_pub: me.into(),
                pool_account: "0x00000000000000000000000000000000000d3m0".into(),
                dry_run: false,
            })),
        ),
        entry("09:00:02", Level::Info, "the reverse side is on", "", Some(Signal::ReverseOn(reverse.into()))),
        entry("09:00:03", Level::Info, "on the board", "board demo", None),
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
