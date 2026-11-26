//! Every port answered from memory: the backend for tests and for `--demo`. It reads nothing and sends nothing.

use crate::monitors::btc_eth::{
    model::{
        Ad, Books, Buyback, Capacity, ChainTx, Direction, Fee, Health, Identity, Info, Level, LogEntry, Notes, Outlook, PoolLock, Quotes, ReverseSide, Role,
        Signal, Stock, Swap, Target, Terms, UNIT,
    },
    ports::{Board, Chain, Fut, GasBalance, LogFeed, PoolLog, Ports, Provider, QUEUE, SwapStore},
};
use crate::monitors::common::model::Coin;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, channel};

#[derive(Debug, Clone, Default)]
pub struct Memory {
    pub info: Info,
    pub health: Health,
    pub reverse: ReverseSide,
    pub ads: Vec<Ad>,
    pub swaps: Vec<Swap>,
    pub quotes: Quotes,
    pub eth: f64,
    /// The transactions listed at each address, the ones known by id only, and the chain's outlook.
    pub txs: Vec<(String, Vec<ChainTx>)>,
    pub lookups: Vec<ChainTx>,
    pub outlook: Outlook,
    /// Ethereum's newest block, and every lock the pool's log holds.
    pub eth_tip: u64,
    pub pool_locks: Vec<PoolLock>,
}

impl PoolLog for Memory {
    fn tip(&self) -> Fut<'_, u64> {
        Box::pin(async move { Ok(self.eth_tip) })
    }
    fn locks(&self, from: u64, to: u64) -> Fut<'_, Option<Vec<PoolLock>>> {
        Box::pin(async move { Ok(Some(self.pool_locks.iter().filter(|l| (from..=to).contains(&l.block)).cloned().collect())) })
    }
}

impl Chain for Memory {
    fn unspent<'a>(&'a self, _address: &'a str) -> Fut<'a, Vec<Coin>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn history<'a>(&'a self, address: &'a str) -> Fut<'a, Vec<ChainTx>> {
        Box::pin(async move { Ok(self.txs.iter().find(|(a, _)| a == address).map(|(_, t)| t.clone()).unwrap_or_default()) })
    }
    fn tx<'a>(&'a self, txid: &'a str) -> Fut<'a, Option<ChainTx>> {
        Box::pin(async move { Ok(self.lookups.iter().find(|t| t.txid == txid).cloned()) })
    }
    fn spent<'a>(&'a self, _txid: &'a str, _vout: u32) -> Fut<'a, bool> {
        Box::pin(async move { Ok(false) })
    }
    fn outlook(&self) -> Fut<'_, Outlook> {
        Box::pin(async move { Ok(self.outlook) })
    }
}

impl Provider for Memory {
    fn info(&self) -> Fut<'_, Info> {
        Box::pin(async move { Ok(self.info.clone()) })
    }
    fn health(&self) -> Fut<'_, Health> {
        Box::pin(async move { Ok(self.health.clone()) })
    }
    fn reverse(&self) -> Fut<'_, ReverseSide> {
        Box::pin(async move { Ok(self.reverse.clone()) })
    }
}

impl Board for Memory {
    fn ads(&self) -> Fut<'_, Vec<Ad>> {
        Box::pin(async move { Ok(self.ads.clone()) })
    }
}

impl SwapStore for Memory {
    fn swaps(&self) -> Fut<'_, Vec<Swap>> {
        Box::pin(async move { Ok(self.swaps.clone()) })
    }
    fn quotes(&self) -> Fut<'_, Quotes> {
        Box::pin(async move { Ok(self.quotes) })
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
        pool: Some(m),
        log: Box::new(MemoryLog(log)),
    }
}

/// A made-up provider mid-shift, for `--demo`. Every key, address and transaction id is invented.
pub fn create_demo_ports(now: u64) -> Ports {
    // 31 ETH per BTC: a sat is worth 31 pool units
    const PRICE: u128 = 31;
    let txid = |fill: char, id: &str| format!("{}{id}", fill.to_string().repeat(56));
    let me = "02d1e2f3a4b5c60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
    let terms = Terms { min: 50_000, max: 500_000, fee_bps: 30, flat: 1_000, available: Some(500_000), ..Default::default() };
    let reverse_terms = Terms { min: UNIT / 100, max: UNIT, fee_bps: 30, flat: 20_000, available: Some(26 * UNIT / 100), ..Default::default() };
    // forward: the user's sats, less the fee, at the price; reverse: the user's ETH, less the fee, at the price
    let forward = |id: &str, phase: &str, age: u64, sats: u128| Swap {
        direction: Direction::Forward,
        id: format!("{id}{}", "0".repeat(56)),
        phase: phase.into(),
        created: now - age,
        deadline: now - age + 14_400,
        sats,
        eth: (sats - sats * 30 / 10_000 - 1_000) * PRICE,
        lock_txid: txid('f', id),
        // the user's lock, and the provider's take of it once it is taking
        linked: [(Role::UserLock, txid('f', id))].into_iter().chain(matches!(phase, "taking" | "done").then(|| (Role::Take, txid('7', id)))).collect(),
        said: Vec::new(),
        lock_height: None,
        // the provider's lock of ETH in the pool, once it has made it
        pool_leaf: if matches!(phase, "offered" | "taking" | "done") { txid('5', id) } else { String::new() },
    };
    let reverse = |id: &str, phase: &str, age: u64, sats: u128| Swap {
        direction: Direction::Reverse,
        deadline: if matches!(phase, "built" | "broadcast") { 0 } else { now - age + 14_400 },
        eth: sats * PRICE * 10_000 / 9_970 + 20_000,
        lock_txid: txid('e', id),
        linked: vec![(Role::Lock, txid('e', id))],
        pool_leaf: String::new(),
        ..forward(id, phase, age, sats)
    };
    let (main, reverse_address) = ("bc1qdemo0000000000000000000000000000000pair", "bc1qdemo000000000000000000000000pairreverse");
    const TIP: u64 = 977_500;
    const ETH_TIP: u64 = 24_100_000;
    // a transaction `age` seconds old: confirmed in the block of that time, or still waiting
    let seen = |txid: String, age: u64, confirmed: bool, fee: u64, received: u64, sent: u64| ChainTx {
        txid,
        fee,
        vsize: 150,
        height: confirmed.then(|| TIP - age / 600),
        time: confirmed.then(|| now - age),
        received,
        sent,
        ..Default::default()
    };
    let ad = |name: &str, key: &str, bps: u64, flat: u128, max: u128, avail: u128, reverse_terms: Option<Terms>| Ad {
        name: name.into(),
        claim_pub: key.into(),
        terms: Terms { max, fee_bps: bps, flat, available: Some(avail), ..terms.clone() },
        reverse: reverse_terms.is_some(),
        reverse_terms,
    };
    let books = Books {
        since: now - 3 * 86_400,
        swaps: 4,
        forward: 2,
        reverse: 1,
        refunds: 1,
        fees_sats: 3_150,
        fees_eth: 64_000,
        cost_sats: 1_420,
        cost_eth: 9_000,
        loss_sats: 0,
        loss_eth: 0,
        net_eth: Some(108_630),
        reinvested_eth: 108_630,
        owed_eth: 0,
        stock: Some(Stock {
            eth: 172 * UNIT / 100,
            sats: Some(840_000),
            value_eth: 172 * UNIT / 100 + 840_000 * PRICE,
            at: now - 95,
            first_value_eth: 195 * UNIT / 100,
            first_at: now - 3 * 86_400,
        }),
        notes: Some(Notes { count: 5, total: 172 * UNIT / 100, usable: 172 * UNIT / 100, dust: 1, largest: 90 * UNIT / 100 }),
        target: Some(Target { eth: 10 * UNIT, btc_as_eth: 10 * UNIT, reached: false }),
        lean_bps: Some(6),
        buyback: Some(Buyback { on: false, routed_eth: 0, sends: 0 }),
    };
    let data = Memory {
        info: Info { terms: terms.clone(), books: Some(books) },
        health: Health { ok: true, error: String::new() },
        reverse: ReverseSide::On { terms: reverse_terms.clone(), open: Some(1), max_open: 2 },
        ads: vec![
            ad("demo-pair", me, 30, 1_000, 500_000, 500_000, Some(reverse_terms.clone())),
            ad("harbor", "03aa", 25, 2_000, 2_500_000, 1_900_000, None),
            ad("north", "03bb", 60, 1_000, 5_000_000, 0, Some(Terms { fee_bps: 45, available: Some(3 * UNIT), ..reverse_terms })),
        ],
        swaps: vec![
            forward("a1b2c3d4", "offered", 1_500, 300_000),
            forward("e5f6a7b8", "accepted", 240, 120_000),
            reverse("b7c8d9e0", "claiming", 2_100, 250_000),
            forward("c9d0e1f2", "done", 9_000, 450_000),
            forward("0a1b2c3d", "done", 30_000, 150_000),
            reverse("f1e2d3c4", "done", 52_000, 400_000),
            forward("5d6e7f80", "refunded", 80_000, 200_000),
        ],
        quotes: Quotes { count: 9, last: Some(now - 240) },
        eth: 0.0188,
        txs: vec![
            // takes pay the main address; the reverse side's locks spend from its own, and one of them is paying too little
            (main.into(), vec![seen(txid('7', "c9d0e1f2"), 8_200, true, 420, 449_580, 0), seen(txid('7', "0a1b2c3d"), 29_000, true, 380, 149_620, 0)]),
            (
                reverse_address.into(),
                vec![
                    seen(txid('e', "b7c8d9e0"), 2_000, false, 90, 589_910, 840_000),
                    seen(txid('e', "f1e2d3c4"), 51_500, true, 410, 439_590, 840_000),
                    seen(txid('d', "00000001"), 90_000, true, 300, 840_000, 0),
                ],
            ),
        ],
        // a user's lock pays a script, so it is known by its id alone; the newest is not in any explorer yet
        lookups: vec![
            seen(txid('f', "a1b2c3d4"), 1_400, true, 600, 0, 0),
            seen(txid('f', "c9d0e1f2"), 8_800, true, 520, 0, 0),
            seen(txid('f', "0a1b2c3d"), 29_800, true, 450, 0, 0),
            seen(txid('f', "5d6e7f80"), 79_000, true, 450, 0, 0),
        ],
        outlook: Outlook { tip: TIP, hour_milli: 2_400 },
        eth_tip: ETH_TIP,
        // a pool lock lands about a minute after its swap begins
        pool_locks: [("a1b2c3d4", 1_500u64), ("c9d0e1f2", 9_000), ("0a1b2c3d", 30_000)]
            .map(|(id, age)| PoolLock { leaf: txid('5', id), tx: format!("0x{}", txid('6', id)), block: ETH_TIP - (age - 60) / 12, time: Some(now - age + 60) })
            .to_vec(),
    };
    let entry =
        |time: &str, level, msg: &str, detail: &str, signal| LogEntry { time: time.into(), at: None, level, msg: msg.into(), detail: detail.into(), signal };
    let counted = |id: &str, fee| Some(Signal::Counted { id: format!("{id}{}", "0".repeat(56)), fee });
    let log = vec![
        entry("09:00:00", Level::Info, "pair provider: Bitcoin for ETH", "reverse true", Some(Signal::StartPrice(Some(30.94)))),
        entry(
            "09:00:01",
            Level::Info,
            "fast bridge LP",
            "",
            Some(Signal::Started(Identity {
                bitcoin_address: "bc1qdemo0000000000000000000000000000000pair".into(),
                claim_pub: me.into(),
                pool_account: "0x00000000000000000000000000000000000d3m0".into(),
                dry_run: false,
            })),
        ),
        entry("09:00:02", Level::Info, "the reverse side is on", "", Some(Signal::ReverseOn("bc1qdemo000000000000000000000000pairreverse".into()))),
        entry("09:00:03", Level::Info, "on the board", "board demo", None),
        entry("09:00:21", Level::Info, "upkeep is on", "buyback false  notesMerge true  recycle true  skew 15  tickSecs 120", None),
        entry(
            "09:41:10",
            Level::Info,
            "stats: a swap is counted",
            "dir rev  end done  feeEth 0.00039  id f1e2d3c4…0000",
            counted("f1e2d3c4", Fee::Eth(39_000)),
        ),
        entry("11:02:44", Level::Warn, "a price source gave nothing", "source 1  error HTTP 429", None),
        entry("12:15:07", Level::Info, "stats: a swap is counted", "dir fwd  end done  feeSats 1,450  id 0a1b2c3d…0000", counted("0a1b2c3d", Fee::Sats(1_450))),
        entry("14:31:52", Level::Info, "swap", "c9d0e1f2…0000  taking → done", None),
        entry("14:33:20", Level::Info, "stats: a swap is counted", "dir fwd  end done  feeSats 1,700  id c9d0e1f2…0000", counted("c9d0e1f2", Fee::Sats(1_700))),
        entry(
            "14:40:03",
            Level::Info,
            "capacity",
            "held 1.72000  promised 0.13113  open 2  holds 0",
            Some(Signal::Capacity(Capacity { held: 172 * UNIT / 100, promised: 13_113_231, open: 2, holds: 0 })),
        ),
        entry("14:41:15", Level::Info, "swap", "a1b2c3d4…0000  locked → offered", None),
    ];
    create_memory_ports(data, log)
}
