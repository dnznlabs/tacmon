//! What the pages draw, and what each event from the feed does to it.

use super::{
    advisor::{self, Advice, Cost, Rival},
    config::BtcEthConfig,
    feed::Event,
    ledger::{self, TxRow},
    model::*,
    upkeep::{self, Held, Place},
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::{HashMap, HashSet, VecDeque};

const MAX_LOG: usize = 5000;
/// The most swaps whose counted fee is kept, the oldest forgotten first. The log gives each once.
const MAX_FEES: usize = 5000;

/// The sizes the Market page compares fees at: 100,000 sats and 0.05 ETH, the smallest a swap usually is.
pub const REF_SATS: u128 = 100_000;
pub const REF_ETH: u128 = 5_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Activity,
    Books,
    Market,
    Advisor,
    Maintenance,
    Transactions,
}

impl Page {
    // Market, Advisor, Maint and Tx sit where the fast bridge monitor has them: a key means the same page in both
    pub const ALL: [Page; 7] = [Page::Overview, Page::Activity, Page::Books, Page::Market, Page::Advisor, Page::Maintenance, Page::Transactions];
    pub const TITLES: [&'static str; 7] = ["Overview", "Log", "Books", "Market", "Advisor", "Maint", "Tx"];
}

/// An ad's name as the pages show it. One that gives none gets the provider's own default: "Provider" and the first
/// six hex digits after the key's prefix byte.
pub fn shown_name(a: &Ad) -> String {
    if a.name.is_empty() { format!("Provider {}", a.claim_pub.get(2..8).unwrap_or("")) } else { a.name.clone() }
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub entry: LogEntry,
    /// Times this line came in a row.
    pub count: u32,
}

pub struct State {
    pub now: u64,
    /// Per page: rows scrolled from the top (tables), or lines back from the newest (the log).
    pub scroll: [usize; Page::ALL.len()],
    /// Log: warnings and alerts only.
    pub only_warn: bool,
    /// Market: which direction's terms the ads are listed by.
    pub market_dir: Direction,
    /// Maintenance: which command is picked, counting only the steps that have one. Kept in range where it is read,
    /// since the steps change under it.
    pick: usize,
    /// Where the provider is on this machine, for the commands the Maintenance page prints.
    place: Place,
    /// Advisor: the fees compared, in hundredths of a percent.
    ladder: Vec<u64>,
    /// Transactions: the picked row's id, and where it was, for when that transaction is no longer listed.
    tx_pick: Option<(String, usize)>,
    /// Where a transaction's page is on the configured explorer's site. Empty with no explorer: the id is copied bare.
    tx_link: String,
    /// A link a key asked to copy, until the shell takes it.
    pub copy: Option<String>,

    pub id: Identity,
    pub cap: Capacity,
    /// Whether any capacity line has been read: until one has, `cap` is nothing, not zero.
    pub cap_seen: bool,
    /// When the provider wrote the capacity line `cap` came from, where the line said.
    pub cap_at: Option<u64>,
    /// The BTC→ETH terms and the books as last read, and whether `/info` answered last time.
    pub terms: Option<Terms>,
    pub books: Option<Books>,
    /// The provider's own word on its gas reserve and whether it shields by itself, as last read.
    pub refill: Option<Refill>,
    pub info_ok: bool,
    /// `Some(true)` healthy, `Some(false)` the provider says its last tick failed, `None` it does not answer.
    pub healthy: Option<bool>,
    pub health_error: String,
    /// Whether an ETH balance is read at all, and the last one read.
    pub shows_eth: bool,
    pub eth: Option<f64>,
    pub log: VecDeque<LogLine>,
    pub swaps: Vec<Swap>,
    pub swaps_ok: bool,
    /// The quotes the provider keeps, as last counted, and whether the last count answered. `None`: never counted.
    pub quotes: Option<Quotes>,
    pub quotes_ok: bool,
    /// The ads for the pair this provider serves. The board carries every pair: the rest are counted in
    /// `other_ads` and never drawn as this market.
    pub ads: Vec<Ad>,
    pub other_ads: usize,
    /// Everything the board last listed, whatever its pair, kept so the list can be drawn again when the pair is learned.
    all_ads: Vec<Ad>,
    /// The pair the provider last said it serves. Kept when `/info` stops answering.
    pair: (Option<Asset>, Option<Asset>),
    pub ads_ok: bool,
    /// Whether the board's ads have ever been read: until they have, no other provider is not the same as none.
    pub ads_seen: bool,
    /// The reverse side as last read, and whether the last read answered.
    pub reverse: Option<ReverseSide>,
    pub reverse_ok: bool,
    /// The address the reverse side sells from: the log's word once that side is on, the configured one until then.
    pub reverse_address: String,
    reverse_address_set: String,
    /// The price the provider started with (ETH per BTC) and when it said so, where the line gave a time.
    pub start_price: Option<(f64, Option<u64>)>,
    /// Whether the provider has a price to quote from, by its log's last word. `None`: the log has not said.
    pub priced: Option<bool>,
    /// What the provider's books counted each ended swap as earning, by swap id, and the order they came in.
    fees: HashMap<String, Fee>,
    fee_order: VecDeque<String>,

    /// Whether an explorer is configured at all.
    pub explorer: bool,
    /// The latest transactions at each address, and whether the last read of each answered.
    pub main_txs: Option<Vec<ChainTx>>,
    pub main_txs_ok: bool,
    pub reverse_txs: Option<Vec<ChainTx>>,
    pub reverse_txs_ok: bool,
    /// The unspent coins at each address, and whether the last read of each answered. `None`: never read.
    pub main_coins: Option<Vec<Coin>>,
    pub main_coins_ok: bool,
    pub reverse_coins: Option<Vec<Coin>>,
    pub reverse_coins_ok: bool,
    /// Swap transactions asked about by id: `Some(None)` is one the explorer does not know. Whether every one asked last was answered.
    looked: HashMap<String, Option<ChainTx>>,
    pub lookups_ok: bool,
    pub outlook: Option<Outlook>,
    pub outlook_ok: bool,
    /// When this run first saw each unconfirmed transaction: an explorer does not say when one was broadcast.
    first_seen: HashMap<String, u64>,
    tx_stuck_secs: u64,
    /// Whether the pool's log is read at all, what has been read of it, and whether the last read answered.
    pub shows_pool: bool,
    pub pool: Option<PoolScan>,
    pub pool_ok: bool,
    /// Where an Ethereum transaction's page is, from `eth_explorer`. Empty: the hash is copied bare.
    eth_link: String,

    pub deadline_warn_secs: u64,
    /// A pool account from the config wins over the one the log names.
    pool_account_fixed: bool,
}

impl State {
    pub fn new(cfg: &BtcEthConfig, shows_eth: bool, explorer: bool, shows_pool: bool) -> Self {
        Self {
            now: 0,
            scroll: [0; Page::ALL.len()],
            only_warn: false,
            market_dir: Direction::Forward,
            pick: 0,
            place: Place::new(&cfg.data_dir.to_string_lossy(), &cfg.log_cmd, &cfg.lp_url),
            ladder: cfg.fee_ladder_bps.clone(),
            tx_pick: None,
            // an Esplora API lives under /api of the site whose pages show a transaction at /tx/<id>
            tx_link: Some(cfg.btc_explorer.trim_end_matches('/'))
                .filter(|b| !b.is_empty())
                .map(|b| format!("{}/tx/", b.strip_suffix("/api").unwrap_or(b)))
                .unwrap_or_default(),
            copy: None,
            id: Identity { pool_account: cfg.pool_account.clone(), ..Default::default() },
            cap: Capacity::default(),
            cap_seen: false,
            cap_at: None,
            terms: None,
            books: None,
            refill: None,
            info_ok: false,
            healthy: None,
            health_error: String::new(),
            shows_eth,
            eth: None,
            log: VecDeque::new(),
            swaps: Vec::new(),
            swaps_ok: false,
            quotes: None,
            quotes_ok: false,
            ads: Vec::new(),
            other_ads: 0,
            all_ads: Vec::new(),
            pair: (None, None),
            ads_ok: false,
            ads_seen: false,
            reverse: None,
            reverse_ok: false,
            reverse_address: cfg.reverse_address.clone(),
            reverse_address_set: cfg.reverse_address.clone(),
            start_price: None,
            priced: None,
            fees: HashMap::new(),
            fee_order: VecDeque::new(),
            explorer,
            main_txs: None,
            main_txs_ok: false,
            reverse_txs: None,
            reverse_txs_ok: false,
            main_coins: None,
            main_coins_ok: false,
            reverse_coins: None,
            reverse_coins_ok: false,
            looked: HashMap::new(),
            lookups_ok: true,
            outlook: None,
            outlook_ok: false,
            first_seen: HashMap::new(),
            tx_stuck_secs: cfg.tx_stuck_secs,
            shows_pool,
            pool: None,
            pool_ok: false,
            eth_link: Some(cfg.eth_explorer.trim_end_matches('/')).filter(|b| !b.is_empty()).map(|b| format!("{b}/tx/")).unwrap_or_default(),
            deadline_warn_secs: cfg.deadline_warn_secs,
            pool_account_fixed: !cfg.pool_account.is_empty(),
        }
    }

    pub fn tick(&mut self, now: u64) {
        self.now = now;
    }

    /// Draws `ads` again from what the board listed, keeping the pair this provider serves. Until the provider has
    /// said what that is, nothing can be told apart and everything is kept.
    fn sift(&mut self) {
        let (btc, evm) = self.pair;
        let mut ads: Vec<Ad> =
            self.all_ads.iter().filter(|a| (btc.is_none() || a.terms.btc_asset == btc) && (evm.is_none() || a.terms.evm_asset == evm)).cloned().collect();
        ads.sort_by_key(|a| effective_bps(REF_SATS, a.terms.fee_bps, a.terms.flat));
        self.other_ads = self.all_ads.len() - ads.len();
        self.ads = ads;
    }

    /// A failed read (`None`) keeps the last value on screen and marks its source as not answering.
    pub fn apply(&mut self, ev: Event) {
        match ev {
            Event::Log(entry) => self.log_entry(entry),
            Event::Info(i) => {
                self.info_ok = i.is_some();
                if let Some(i) = i.map(|i| *i) {
                    let pair = (i.terms.btc_asset, i.terms.evm_asset);
                    self.terms = Some(i.terms);
                    // an answer without books is a provider whose upkeep has not looked yet: the last books are not kept as its word
                    self.books = i.books;
                    self.refill = i.refill;
                    if pair != self.pair {
                        self.pair = pair;
                        self.sift();
                    }
                }
            }
            Event::Health(h) => {
                self.healthy = h.as_ref().map(|h| h.ok);
                self.health_error = h.map(|h| h.error).unwrap_or_default();
            }
            Event::Ads(a) => {
                self.ads_ok = a.is_some();
                if let Some(a) = a {
                    self.ads_seen = true;
                    self.all_ads = a;
                    self.sift();
                }
            }
            Event::Swaps(s) => {
                self.swaps_ok = s.is_some();
                if let Some(mut s) = s {
                    // open ones first, then the newest
                    s.sort_by(|a, b| a.closed().cmp(&b.closed()).then(b.created.cmp(&a.created)));
                    self.swaps = s;
                }
            }
            Event::Eth(e) => self.eth = e,
            // a failed count keeps the last one, marked: the quote directories can fail to read while the records do not
            Event::Quotes(q) => {
                self.quotes_ok = q.is_some();
                self.quotes = q.or(self.quotes);
            }
            Event::MainTxs(t) => {
                self.main_txs_ok = t.is_some();
                if t.is_some() {
                    self.main_txs = t;
                    self.see_unconfirmed();
                }
            }
            Event::ReverseTxs(t) => {
                self.reverse_txs_ok = t.is_some();
                if t.is_some() {
                    self.reverse_txs = t;
                    self.see_unconfirmed();
                }
            }
            Event::MainCoins(c) => {
                self.main_coins_ok = c.is_some();
                self.main_coins = c.or(self.main_coins.take());
            }
            Event::ReverseCoins(c) => {
                self.reverse_coins_ok = c.is_some();
                self.reverse_coins = c.or(self.reverse_coins.take());
            }
            Event::Lookups { found, answered, wanted } => {
                self.lookups_ok = answered;
                // pruned by the feed's own list and only here: the feed forgets the same ids in the same round, so
                // nothing it still holds as settled can go missing from this side
                let wanted: HashSet<String> = wanted.into_iter().collect();
                self.looked.retain(|id, _| wanted.contains(id));
                self.looked.extend(found.into_iter().filter(|(id, _)| wanted.contains(id)));
                self.see_unconfirmed();
            }
            Event::Pool(p) => {
                self.pool_ok = p.is_some();
                if p.is_some() {
                    self.pool = p;
                }
            }
            Event::Outlook(o) => {
                self.outlook_ok = o.is_some();
                if o.is_some() {
                    self.outlook = o;
                }
            }
            Event::Reverse(r) => {
                self.reverse_ok = r.is_some();
                if r.is_some() {
                    self.reverse = r;
                }
            }
        }
    }

    /// Notes when each unconfirmed transaction was first seen, and forgets the ones that are no longer unconfirmed.
    fn see_unconfirmed(&mut self) {
        let lists = self.main_txs.iter().chain(&self.reverse_txs).flatten();
        let waiting: HashSet<&str> = lists.chain(self.looked.values().flatten()).filter(|t| t.height.is_none()).map(|t| t.txid.as_str()).collect();
        self.first_seen.retain(|id, _| waiting.contains(id.as_str()));
        for id in waiting {
            if !self.first_seen.contains_key(id) {
                self.first_seen.insert(id.to_string(), self.now);
            }
        }
    }

    /// The Transactions page's rows.
    pub fn ledger(&self) -> Vec<TxRow> {
        ledger::rows(&ledger::Facts {
            swaps: &self.swaps,
            main: self.main_txs.as_deref(),
            reverse: self.reverse_txs.as_deref(),
            looked: &self.looked,
            outlook: self.outlook,
            first_seen: &self.first_seen,
            pool: self.pool.as_ref(),
            now: self.now,
            stuck_secs: self.tx_stuck_secs,
        })
    }
    /// The stuck transactions that are the provider's to act on. A user's lock is the user's to bump.
    pub fn stuck(&self) -> Vec<TxRow> {
        self.ledger().into_iter().filter(|r| r.standing.stuck && r.role != Some(Role::UserLock)).collect()
    }
    /// The picked row's place in `rows`: the transaction picked while it is listed, else the row where it was.
    /// The rows reorder as transactions confirm, so the pick follows the transaction and not the position.
    pub fn tx_picked(&self, rows: &[TxRow]) -> Option<usize> {
        let last = rows.len().checked_sub(1)?;
        Some(match &self.tx_pick {
            Some((id, was)) => rows.iter().position(|r| r.txid == *id).unwrap_or((*was).min(last)),
            None => 0,
        })
    }

    /// What a swap costs the provider, by its books and the price it values its sats at.
    pub fn swap_cost(&self) -> Option<Cost> {
        advisor::cost(self.books.as_ref()?, self.price().map(|(p, ..)| p))
    }
    /// Where the fee of one direction stands. `None` while that direction's terms are not known, or it is off.
    pub fn advice(&self, d: Direction) -> Option<Advice> {
        let own = match d {
            Direction::Forward => self.terms.as_ref()?,
            Direction::Reverse => self.reverse_terms()?,
        };
        // nothing posted under the provider's own key is a rival: right after a change of terms its own ad still
        // carries the old ones, and would be read as another provider. An ad under its key that is not its own is
        // the header's to call out, not a fee to be measured against.
        let own_key = &self.id.claim_pub;
        let named: Vec<(String, &Terms)> = self
            .ads
            .iter()
            .filter(|a| own_key.is_empty() || a.claim_pub != *own_key)
            .filter_map(|a| {
                let terms = if d == Direction::Forward { Some(&a.terms) } else { a.reverse_terms.as_ref() };
                Some((shown_name(a), terms?))
            })
            .collect();
        let rivals: Vec<Rival> = named.iter().map(|(name, terms)| Rival { name, terms }).collect();
        let (cost, step) = match d {
            Direction::Forward => (self.swap_cost().and_then(|c| c.as_sats), 1_000),
            Direction::Reverse => (self.swap_cost().and_then(|c| c.as_eth), UNIT / 1_000),
        };
        Some(advisor::advise(own, self.ads_seen.then_some(rivals.as_slice()), &self.ladder, cost, step))
    }

    /// What an address holds by the explorer's list of its coins: confirmed sats, then sats still unconfirmed.
    pub fn held(coins: Option<&[Coin]>) -> Option<Held> {
        let sum = |confirmed: bool| coins.map(|c| c.iter().filter(|c| c.confirmed == confirmed).map(|c| c.sats).sum::<u64>());
        Some(Held { confirmed: sum(true)?, unconfirmed: sum(false)?, coins: coins?.len() })
    }

    /// What the provider keeps at its pool account for gas: its own word, or Tacit's default where it does not say.
    pub fn gas_reserve(&self) -> f64 {
        self.refill.and_then(|r| r.gas_reserve_eth).unwrap_or(upkeep::GAS_RESERVE_ETH)
    }

    /// The Maintenance page's steps.
    pub fn upkeep(&self) -> Vec<Step> {
        let stuck = self.stuck();
        let lost: Vec<&str> = self.swaps.iter().filter(|s| s.lost()).map(|s| s.id.as_str()).collect();
        upkeep::upkeep(&upkeep::Facts {
            place: &self.place,
            healthy: self.healthy,
            health_error: &self.health_error,
            terms: self.terms.as_ref(),
            open: self.swaps.iter().filter(|s| !s.closed()).count(),
            promised: self.cap_seen.then_some(self.cap.promised),
            notes: self.notes(),
            reverse: self.reverse.as_ref(),
            reverse_address: &self.reverse_address,
            reverse_held: Self::held(self.reverse_coins.as_deref()),
            pool_account: &self.id.pool_account,
            gas: self.eth,
            gas_reserve: self.gas_reserve(),
            min_chunk: self.refill.and_then(|r| r.min_chunk_eth).unwrap_or(upkeep::MIN_CHUNK_ETH),
            auto_shield: self.refill.and_then(|r| r.auto_shield),
            priced: self.priced,
            on_board: self.on_board(),
            stuck: &stuck,
            lost: &lost,
            explorer: self.explorer,
            hour_milli: self.outlook.map(|o| o.hour_milli),
            now: self.now,
        })
    }
    /// The commands on the Maintenance page as they paste: one per step that has one, its lines joined with ` \`.
    pub fn commands(&self) -> Vec<String> {
        self.upkeep().iter().filter(|s| !s.cmd.is_empty()).map(|s| s.cmd.join(" \\\n")).collect()
    }
    /// The picked command's place among them, or `None` when there is none.
    pub fn picked(&self) -> Option<usize> {
        let n = self.commands().len();
        (n > 0).then(|| self.pick.min(n - 1))
    }

    pub fn on_key(&mut self, page: Page, k: KeyEvent) {
        if page == Page::Maintenance {
            let n = self.commands().len();
            // shift picks; K and J too, for a terminal that sends shifted arrows as plain ones
            let shift = k.modifiers.contains(KeyModifiers::SHIFT);
            let s = &mut self.scroll[page as usize];
            match k.code {
                KeyCode::Up if shift => self.pick = self.pick.min(n.saturating_sub(1)).saturating_sub(1),
                KeyCode::Char('K') => self.pick = self.pick.min(n.saturating_sub(1)).saturating_sub(1),
                KeyCode::Down if shift => self.pick = (self.pick + 1).min(n.saturating_sub(1)),
                KeyCode::Char('J') => self.pick = (self.pick + 1).min(n.saturating_sub(1)),
                KeyCode::Up | KeyCode::Char('k') => *s = s.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => *s += 1,
                KeyCode::PageUp => *s = s.saturating_sub(10),
                KeyCode::PageDown => *s += 10,
                KeyCode::Char('g') | KeyCode::Home => *s = 0,
                KeyCode::Char('c') | KeyCode::Enter => self.copy = self.picked().and_then(|i| self.commands().into_iter().nth(i)),
                _ => {}
            }
            return;
        }
        if page == Page::Transactions {
            // a table with a cursor: every arrow moves the pick, and the window follows it
            let rows = self.ledger();
            let Some(at) = self.tx_picked(&rows) else { return };
            let last = rows.len() - 1;
            let to = match k.code {
                KeyCode::Up | KeyCode::Char('k' | 'K') => at.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j' | 'J') => (at + 1).min(last),
                KeyCode::PageUp => at.saturating_sub(10),
                KeyCode::PageDown => (at + 10).min(last),
                KeyCode::Char('g') | KeyCode::Home => 0,
                KeyCode::Char('G') | KeyCode::End => last,
                KeyCode::Char('c') | KeyCode::Enter => {
                    // an Ethereum hash has a site of its own
                    let site = if rows[at].place == ledger::Place::Pool { &self.eth_link } else { &self.tx_link };
                    self.copy = Some(format!("{site}{}", rows[at].txid));
                    at
                }
                _ => at,
            };
            self.tx_pick = Some((rows[to].txid.clone(), to));
            return;
        }
        // in the log "up" goes back in time, which is further from the newest line
        let back = page == Page::Activity;
        let s = &mut self.scroll[page as usize];
        let (older, newer) = (|s: &mut usize, n: usize| *s += n, |s: &mut usize, n: usize| *s = s.saturating_sub(n));
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => (if back { older } else { newer })(s, 1),
            KeyCode::Down | KeyCode::Char('j') => (if back { newer } else { older })(s, 1),
            KeyCode::PageUp => (if back { older } else { newer })(s, 20),
            KeyCode::PageDown => (if back { newer } else { older })(s, 20),
            KeyCode::Char('g') | KeyCode::Home | KeyCode::Char('G') | KeyCode::End => *s = 0,
            KeyCode::Char('w') if page == Page::Activity => {
                self.only_warn = !self.only_warn;
                *s = 0;
            }
            KeyCode::Char('d') if page == Page::Market => {
                self.market_dir = match self.market_dir {
                    Direction::Forward => Direction::Reverse,
                    Direction::Reverse => Direction::Forward,
                };
                *s = 0;
            }
            _ => {}
        }
    }

    fn log_entry(&mut self, entry: LogEntry) {
        match &entry.signal {
            Some(Signal::Started(id)) => {
                let pool = if self.pool_account_fixed { self.id.pool_account.clone() } else { id.pool_account.clone() };
                self.id = Identity { pool_account: pool, ..id.clone() };
                // a provider that has started again says afresh whether its reverse side is on and what it is priced at
                self.reverse_address.clone_from(&self.reverse_address_set);
                // what was read at another address is not what this one holds
                if self.reverse_address.is_empty() {
                    self.reverse_coins = None;
                }
            }
            Some(Signal::Capacity(c)) => {
                self.cap = c.clone();
                self.cap_seen = true;
                self.cap_at = entry.at;
            }
            Some(Signal::ReverseOn(address)) => self.reverse_address.clone_from(address),
            Some(Signal::StartPrice(p)) => {
                self.start_price = p.map(|p| (p, entry.at));
                self.priced = Some(p.is_some());
            }
            Some(Signal::Priced(has)) => self.priced = Some(*has),
            Some(Signal::Counted { id, fee }) if self.fees.insert(id.clone(), *fee).is_none() => {
                self.fee_order.push_back(id.clone());
                if self.fee_order.len() > MAX_FEES
                    && let Some(old) = self.fee_order.pop_front()
                {
                    self.fees.remove(&old);
                }
            }
            // counted before: the figure is replaced, and its place in the order kept
            Some(Signal::Counted { .. }) => {}
            None => {}
        }
        // a line that repeats the one before it is counted, not listed again. Same level only: an alert must not be
        // folded into a quieter line that happens to share its words.
        if let Some(last) = self.log.back_mut()
            && last.entry.msg == entry.msg
            && last.entry.level == entry.level
            && !matches!(entry.msg.as_str(), "swap" | "capacity" | "stats: a swap is counted")
        {
            last.count += 1;
            last.entry = entry;
            return;
        }
        if self.log.len() == MAX_LOG {
            self.log.pop_front();
        }
        self.log.push_back(LogLine { entry, count: 1 });
    }

    /// Pool ETH not promised to anyone, by the log's last capacity line. `None` until the log has one.
    pub fn free_eth(&self) -> Option<u128> {
        self.cap_seen.then(|| self.cap.held.saturating_sub(self.cap.promised))
    }
    /// How old the log's capacity figures are, where the line gave a time.
    pub fn cap_age(&self) -> Option<u64> {
        self.cap_at.map(|at| self.now.saturating_sub(at))
    }
    /// Whether the swap records have ever been read: until they have, counts from them are nothing, not zero.
    pub fn swaps_known(&self) -> bool {
        self.swaps_ok || !self.swaps.is_empty()
    }
    fn of(&self, d: Direction) -> impl Iterator<Item = &Swap> {
        self.swaps.iter().filter(move |s| s.direction == d)
    }
    pub fn open(&self, d: Direction) -> usize {
        self.of(d).filter(|s| !s.closed()).count()
    }
    pub fn paid(&self, d: Direction) -> usize {
        self.of(d).filter(|s| s.paid()).count()
    }
    /// What swaps under way have at stake on each side: the sats and the pool ETH of every one not closed.
    pub fn at_stake(&self) -> (u128, u128) {
        let open = || self.swaps.iter().filter(|s| !s.closed());
        (total(open().map(|s| s.sats)), total(open().map(|s| s.eth)))
    }
    /// Swaps that ended with the provider out of pocket, as its own records name them.
    pub fn lost(&self) -> usize {
        self.swaps.iter().filter(|s| s.lost()).count()
    }
    /// Whether any reverse swap has ever been recorded here.
    pub fn has_reverse_swaps(&self) -> bool {
        self.of(Direction::Reverse).next().is_some()
    }
    /// What the provider's books counted this swap as earning. `None`: not counted yet, or counted before the log
    /// this run has read begins.
    pub fn fee(&self, s: &Swap) -> Option<Fee> {
        self.fees.get(&s.id).copied()
    }
    /// The reverse side's terms while it is on.
    pub fn reverse_terms(&self) -> Option<&Terms> {
        match &self.reverse {
            Some(ReverseSide::On { terms, .. }) => Some(terms),
            _ => None,
        }
    }
    /// The sats the reverse side sells from, as the upkeep last counted them, and when. `None` while that side is
    /// not known to be on: the provider then reports a zero that nothing counted.
    pub fn sats_to_sell(&self) -> Option<(u128, u64)> {
        let stock = self.books.as_ref()?.stock.as_ref()?;
        matches!(self.reverse, Some(ReverseSide::On { .. })).then_some((stock.sats?, stock.at))
    }
    /// The provider's count of its ETH notes, where it can be the count of what it holds. A count of no notes beside
    /// ETH held is a count that has not been made, or was made of something else: `Err` then, with what it holds.
    pub fn notes(&self) -> Option<Result<&Notes, u128>> {
        let books = self.books.as_ref()?;
        let notes = books.notes.as_ref()?;
        let held = books.stock.as_ref().map_or(0, |s| s.eth).max(if self.cap_seen { self.cap.held } else { 0 });
        Some(if notes.count == 0 && held > 0 { Err(held) } else { Ok(notes) })
    }
    /// The price to show, in ETH per BTC, with when it was read and whose it is: the one the provider's upkeep last
    /// valued its sats at, else the one it started with. Never a price of this monitor's own: it reads none.
    pub fn price(&self) -> Option<(f64, Option<u64>, &'static str)> {
        let looked = self.books.as_ref().and_then(|b| b.stock.as_ref()).and_then(|s| Some((s.price()?, Some(s.at).filter(|at| *at > 0), "upkeep")));
        looked.or(self.start_price.map(|(p, at)| (p, at, "at start")))
    }
    /// Every ad posted under this provider's claim key. The board checks no signature, so anyone can post one.
    fn my_ads(&self) -> impl Iterator<Item = &Ad> {
        self.ads.iter().filter(|a| !self.id.claim_pub.is_empty() && a.claim_pub == self.id.claim_pub)
    }
    /// An ad under this provider's key that is not its own.
    pub fn ad_contested(&self) -> bool {
        self.my_ads().count() > usize::from(self.my_ad().is_some())
    }
    /// The ad that is this provider's: the one under its key carrying the terms it reports itself. While those terms
    /// are not known, a lone ad under the key is taken at its word and one of several is not.
    pub fn my_ad(&self) -> Option<&Ad> {
        match &self.terms {
            Some(own) => self.my_ads().find(|a| (a.terms.min, a.terms.max, a.terms.fee_bps, a.terms.flat) == (own.min, own.max, own.fee_bps, own.flat)),
            None => self.my_ads().next().filter(|_| self.my_ads().nth(1).is_none()),
        }
    }
    /// Where this provider's BTC→ETH fee stands among the ads that would take a swap of `size` sats now: its place,
    /// cheapest first, and how many there are. A tie is shared. `None` when its own ad is not on the board or
    /// would not take that size itself.
    pub fn fee_rank(&self, size: u128) -> Option<(usize, usize)> {
        let mine = self.my_ad()?;
        let takes = |t: &Terms| advisor::takes(t, size);
        let cost = |t: &Terms| fee_on(size, t.fee_bps, t.flat);
        let rivals: Vec<&Ad> = self.ads.iter().filter(|a| takes(&a.terms)).collect();
        takes(&mine.terms).then(|| (1 + rivals.iter().filter(|a| cost(&a.terms) < cost(&mine.terms)).count(), rivals.len()))
    }
    /// The sizes that rank is shown at: the least and the most this provider takes.
    pub fn rank_sizes(&self) -> Vec<u128> {
        let Some(t) = &self.terms else { return Vec::new() };
        if t.min < t.max { vec![t.min, t.max.min(t.available.unwrap_or(t.max)).max(t.min)] } else { vec![t.min] }
    }
    /// `None` while it cannot be told: the board's ads were not read, or this provider's key is not known yet.
    pub fn on_board(&self) -> Option<bool> {
        (self.ads_ok && !self.id.claim_pub.is_empty()).then(|| self.my_ad().is_some())
    }
    pub fn name(&self) -> String {
        self.my_ad().map(|a| a.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "provider".into())
    }
    /// The board's ads with the terms of `d`, cheapest first at the reference size. In reverse, only ads that carry
    /// readable reverse terms.
    pub fn market(&self, d: Direction) -> Vec<(&Ad, &Terms)> {
        let (mut out, size): (Vec<(&Ad, &Terms)>, u128) = match d {
            Direction::Forward => (self.ads.iter().map(|a| (a, &a.terms)).collect(), REF_SATS),
            Direction::Reverse => (self.ads.iter().filter_map(|a| a.reverse_terms.as_ref().map(|t| (a, t))).collect(), REF_ETH),
        };
        out.sort_by_key(|(_, t)| effective_bps(size, t.fee_bps, t.flat));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State::new(&BtcEthConfig::default(), false, false, false)
    }
    fn swap(direction: Direction, phase: &str, created: u64, sats: u128, eth: u128) -> Swap {
        Swap { direction, id: format!("{phase}{created}"), phase: phase.into(), created, sats, eth, ..Default::default() }
    }
    fn signal(signal: Signal, at: Option<u64>) -> Event {
        Event::Log(LogEntry { signal: Some(signal), at, ..LogEntry::plain(Level::Info, "a line") })
    }
    fn terms(bps: u64) -> Terms {
        Terms { min: 50_000, max: 500_000, fee_bps: bps, flat: 1000, available: Some(400_000), ..Default::default() }
    }

    #[test]
    fn a_failed_read_keeps_the_last_value_and_marks_the_source() {
        let mut st = state();
        st.apply(Event::Info(Some(Box::new(Info { terms: terms(30), books: Some(Books { swaps: 4, ..Default::default() }), refill: None }))));
        st.apply(Event::Swaps(Some(vec![swap(Direction::Forward, "done", 1, 100_000, 3_100_000)])));
        st.apply(Event::Reverse(Some(ReverseSide::Off)));
        st.apply(Event::Ads(Some(Vec::new())));

        for failed in [Event::Info(None), Event::Swaps(None), Event::Reverse(None), Event::Ads(None), Event::Health(None)] {
            st.apply(failed);
        }

        assert_eq!((st.info_ok, st.swaps_ok, st.reverse_ok, st.ads_ok, st.healthy), (false, false, false, false, None));
        assert_eq!(
            (st.terms.as_ref().map(|t| t.fee_bps), st.books.as_ref().map(|b| b.swaps), st.swaps.len(), st.reverse.clone()),
            (Some(30), Some(4), 1, Some(ReverseSide::Off))
        );
        assert!(st.swaps_known());
    }

    #[test]
    fn an_answer_without_books_takes_the_last_books_off_the_screen() {
        let mut st = state();
        st.apply(Event::Info(Some(Box::new(Info { terms: terms(30), books: Some(Books { swaps: 4, ..Default::default() }), refill: None }))));

        st.apply(Event::Info(Some(Box::new(Info { terms: terms(30), books: None, refill: None }))));

        assert!(st.info_ok && st.books.is_none());
    }

    #[test]
    fn nothing_the_log_has_not_said_is_a_figure() {
        let st = state();

        let read = (st.free_eth(), st.price(), st.priced, st.on_board(), st.swaps_known());

        assert_eq!(read, (None, None, None, None, false));
    }

    #[test]
    fn capacity_and_price_come_from_the_log_with_their_age() {
        let mut st = state();
        st.tick(1_000);

        st.apply(signal(Signal::Capacity(Capacity { held: 7_000_000, promised: 3_100_000, open: 1, holds: 0 }), Some(940)));
        st.apply(signal(Signal::StartPrice(Some(31.0)), Some(100)));
        st.apply(signal(Signal::Priced(false), Some(990)));

        assert_eq!((st.free_eth(), st.cap_age(), st.price(), st.priced), (Some(3_900_000), Some(60), Some((31.0, Some(100), "at start")), Some(false)));
    }

    #[test]
    fn the_upkeeps_price_is_shown_over_the_one_at_start() {
        let mut st = state();
        let stock = Stock { eth: 50_000_000, sats: Some(1_000_000), value_eth: 82_000_000, at: 500, ..Default::default() };
        st.apply(signal(Signal::StartPrice(Some(31.0)), Some(100)));

        st.apply(Event::Info(Some(Box::new(Info { terms: terms(30), books: Some(Books { stock: Some(stock), ..Default::default() }), refill: None }))));

        assert_eq!(st.price(), Some((32.0, Some(500), "upkeep")));
    }

    #[test]
    fn swaps_are_counted_by_direction_open_ones_first_and_what_is_at_stake_is_summed() {
        let mut st = state();
        let swaps = vec![
            swap(Direction::Forward, "done", 1, 100_000, 3_100_000),
            swap(Direction::Forward, "offered", 2, 200_000, 6_200_000),
            swap(Direction::Forward, "lost", 3, 50_000, 1_500_000),
            swap(Direction::Reverse, "claimed", 4, 300_000, 9_400_000),
        ];

        st.apply(Event::Swaps(Some(swaps)));

        let by = |d| (st.open(d), st.paid(d));
        assert_eq!((by(Direction::Forward), by(Direction::Reverse), st.at_stake(), st.lost()), ((1, 1), (1, 1), (500_000, 15_600_000), 1));
        assert_eq!(st.swaps.iter().map(|s| s.phase.as_str()).collect::<Vec<_>>(), ["claimed", "offered", "lost", "done"]);
    }

    #[test]
    fn a_swaps_fee_is_what_the_books_counted_and_nothing_until_they_have() {
        let mut st = state();
        let (counted, open) = (swap(Direction::Forward, "done", 1, 100_000, 3_100_000), swap(Direction::Forward, "taking", 2, 100_000, 3_100_000));

        st.apply(signal(Signal::Counted { id: counted.id.clone(), fee: Fee::Sats(1_300) }, None));

        assert_eq!((st.fee(&counted), st.fee(&open)), (Some(Fee::Sats(1_300)), None));
    }

    #[test]
    fn only_the_pairs_ads_are_the_market_and_the_providers_own_is_the_one_with_its_terms() {
        let mut st = state();
        let (sats_asset, eth_asset, other) = (Some(Asset([1; 32])), Some(Asset([2; 32])), Some(Asset([9; 32])));
        let ad = |name: &str, key: &str, bps, btc_asset| Ad {
            name: name.into(),
            claim_pub: key.into(),
            terms: Terms { btc_asset, evm_asset: eth_asset, ..terms(bps) },
            reverse: false,
            reverse_terms: None,
        };
        st.apply(signal(Signal::Started(Identity { claim_pub: "02me".into(), ..Default::default() }), None));
        st.apply(Event::Info(Some(Box::new(Info { terms: Terms { btc_asset: sats_asset, evm_asset: eth_asset, ..terms(30) }, books: None, refill: None }))));

        st.apply(Event::Ads(Some(vec![
            ad("dear", "03aa", 90, sats_asset),
            ad("mine", "02me", 30, sats_asset),
            ad("impostor", "02me", 5, sats_asset),
            ad("tac", "03bb", 1, other),
        ])));

        assert_eq!(st.market(Direction::Forward).iter().map(|(a, _)| a.name.as_str()).collect::<Vec<_>>(), ["impostor", "mine", "dear"]);
        assert_eq!((st.other_ads, st.name(), st.on_board(), st.ad_contested()), (1, "mine".to_string(), Some(true), true));
        assert!(st.market(Direction::Reverse).is_empty());
    }

    #[test]
    fn repeated_log_lines_are_counted_not_listed_and_a_restart_forgets_the_reverse_address() {
        let mut st = state();
        let line = || Event::Log(LogEntry::plain(Level::Warn, "a price source gave nothing"));
        st.apply(signal(Signal::ReverseOn("bc1qreverse".into()), None));

        st.apply(line());
        st.apply(line());
        st.apply(signal(Signal::Started(Identity::default()), None));

        assert_eq!(st.log.iter().map(|l| l.count).collect::<Vec<_>>(), [1, 2, 1]);
        assert!(st.reverse_address.is_empty());
    }

    #[test]
    fn sats_nothing_counted_and_notes_that_were_not_counted_are_not_figures() {
        let mut st = state();
        let stock = Stock { eth: 6_974_000, sats: Some(0), value_eth: 6_974_000, at: 500, ..Default::default() };
        let books = |notes: Notes| Books { stock: Some(stock.clone()), notes: Some(notes), ..Default::default() };
        let info = |b: Books| Event::Info(Some(Box::new(Info { terms: terms(30), books: Some(b), refill: None })));
        st.apply(Event::Reverse(Some(ReverseSide::Off)));

        // as a provider with the reverse side off reports itself: sats of "0", and no notes beside the ETH it holds
        st.apply(info(books(Notes::default())));
        let off = (st.sats_to_sell(), st.notes().map(|n| n.map(|n| n.count)));
        st.apply(Event::Reverse(Some(ReverseSide::On { terms: terms(30), open: Some(0), max_open: 2 })));
        st.apply(info(books(Notes { count: 2, total: 6_974_000, usable: 6_974_000, ..Default::default() })));
        let on = (st.sats_to_sell(), st.notes().map(|n| n.map(|n| n.count)));

        assert_eq!((off, on), ((None, Some(Err(6_974_000))), (Some((0, 500)), Some(Ok(2)))));
    }

    fn txid(c: char) -> String {
        c.to_string().repeat(64)
    }
    fn chain_tx(c: char, height: Option<u64>) -> ChainTx {
        ChainTx { txid: txid(c), fee: 430, vsize: 430, height, time: height.map(|h| h * 600), ..Default::default() }
    }

    #[test]
    fn an_unconfirmed_transaction_is_timed_from_when_it_was_first_seen_and_a_users_stuck_lock_is_not_the_providers() {
        let mut st = State::new(&BtcEthConfig::default(), false, true, false);
        let swap = Swap { linked: vec![(Role::UserLock, txid('a')), (Role::Take, txid('b'))], ..swap(Direction::Forward, "taking", 1, 100_000, 3_100_000) };
        st.apply(Event::Swaps(Some(vec![swap])));
        st.tick(1_000);

        st.apply(Event::Lookups {
            found: vec![(txid('a'), Some(chain_tx('a', None))), (txid('b'), Some(chain_tx('b', None)))],
            answered: true,
            wanted: vec![txid('a'), txid('b')],
        });
        st.tick(1_000 + 1_799);
        let waiting = (st.ledger().iter().map(|r| (r.standing.at, r.standing.stuck)).collect::<Vec<_>>(), st.stuck().len());
        st.tick(1_000 + 1_800);
        let stuck = (st.ledger().iter().filter(|r| r.standing.stuck).count(), st.stuck().iter().map(|r| r.role).collect::<Vec<_>>());
        // the take confirms, and a failed round keeps what was known
        st.apply(Event::Lookups { found: vec![(txid('b'), Some(chain_tx('b', Some(5))))], answered: false, wanted: vec![txid('a'), txid('b')] });

        assert_eq!(waiting, (vec![(Some(1_000), false), (Some(1_000), false)], 0));
        assert_eq!(stuck, (2, vec![Some(Role::Take)]));
        assert_eq!((st.stuck().len(), st.lookups_ok, st.ledger().len(), st.first_seen.len()), (0, false, 2, 1));
    }

    #[test]
    fn the_pick_follows_its_transaction_as_rows_reorder_and_enter_copies_its_explorer_link() {
        let cfg = BtcEthConfig { btc_explorer: "https://explorer.example/api/".into(), ..Default::default() };
        let (mut linked, mut bare) = (State::new(&cfg, false, true, false), state());
        let key = |st: &mut State, code| st.on_key(Page::Transactions, KeyEvent::from(code));
        for st in [&mut linked, &mut bare] {
            st.apply(Event::MainTxs(Some(vec![chain_tx('a', Some(9)), chain_tx('b', Some(8))])));
        }

        key(&mut linked, KeyCode::Down);
        key(&mut bare, KeyCode::Enter);
        // a new unconfirmed transaction sorts above every confirmed one: the pick stays on "b"
        linked.apply(Event::MainTxs(Some(vec![chain_tx('c', None), chain_tx('a', Some(9)), chain_tx('b', Some(8))])));
        let followed = linked.tx_picked(&linked.ledger());
        key(&mut linked, KeyCode::Enter);
        // one no longer listed leaves the pick where it was
        linked.apply(Event::MainTxs(Some(vec![chain_tx('a', Some(9))])));

        assert_eq!((followed, linked.tx_picked(&linked.ledger())), (Some(2), Some(0)));
        assert_eq!(linked.copy, Some(format!("https://explorer.example/tx/{}", txid('b'))));
        assert_eq!(bare.copy, Some(txid('a')));
    }

    #[test]
    fn a_fee_is_ranked_among_the_ads_that_take_that_size_now_and_not_at_a_size_it_does_not_take_itself() {
        let mut st = state();
        let pair = (Some(Asset([1; 32])), Some(Asset([2; 32])));
        let ad = |key: &str, min, max, bps, flat, available| Ad {
            name: key.into(),
            claim_pub: key.into(),
            terms: Terms { btc_asset: pair.0, evm_asset: pair.1, min, max, fee_bps: bps, flat, available },
            reverse: false,
            reverse_terms: None,
        };
        let mine = ad("02me", 20_000, 97_758, 15, 500, Some(97_758));
        st.apply(signal(Signal::Started(Identity { claim_pub: "02me".into(), ..Default::default() }), None));
        st.apply(Event::Info(Some(Box::new(Info { terms: mine.terms.clone(), books: None, refill: None }))));

        // one takes from 25,000 and charges more; one is cheaper at any size and has nothing left for the larger
        st.apply(Event::Ads(Some(vec![mine, ad("03aa", 25_000, 1_000_000, 20, 800, None), ad("03bb", 20_000, 500_000, 5, 100, Some(30_000))])));

        assert_eq!(st.rank_sizes(), [20_000, 97_758]);
        assert_eq!((st.fee_rank(20_000), st.fee_rank(97_758), st.fee_rank(10_000)), (Some((2, 2)), Some((1, 2)), None));
    }

    #[test]
    fn the_providers_own_ad_with_its_old_terms_is_not_a_rival_and_a_failed_count_of_quotes_is_marked() {
        let mut st = state();
        let ad = |key: &str, bps| Ad { name: key.into(), claim_pub: key.into(), terms: terms(bps), reverse: false, reverse_terms: None };
        st.apply(signal(Signal::Started(Identity { claim_pub: "02me".into(), ..Default::default() }), None));
        // the provider now charges 0.30%; the board still carries its ad at 0.10%
        st.apply(Event::Info(Some(Box::new(Info { terms: terms(30), books: None, refill: None }))));

        st.apply(Event::Ads(Some(vec![ad("02me", 10), ad("03aa", 90)])));
        st.apply(Event::Quotes(Some(Quotes { count: 2, last: Some(5) })));
        st.apply(Event::Quotes(None));

        let low = st.advice(Direction::Forward).expect("advice").low;
        assert!(matches!(low.standing, advisor::Standing::Cheapest { ref name, .. } if name == "03aa"), "{low:?}");
        assert_eq!((st.quotes, st.quotes_ok), (Some(Quotes { count: 2, last: Some(5) }), false));
    }
}
