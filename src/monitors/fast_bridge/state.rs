//! What the pages draw, and what each event from the feed does to it.

use super::{
    advisor::{self, Advice},
    config::FastBridgeConfig,
    feed::Event,
    ledger::{self, Place, TxRow},
    model::*,
    upkeep::{self, Facts, Upkeep},
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::{HashMap, HashSet, VecDeque};

const MAX_LOG: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Activity,
    Demand,
    Market,
    Advisor,
    Maintenance,
    Transactions,
}

impl Page {
    pub const ALL: [Page; 7] = [Page::Overview, Page::Activity, Page::Demand, Page::Market, Page::Advisor, Page::Maintenance, Page::Transactions];
    pub const TITLES: [&'static str; 7] = ["Overview", "Log", "Demand", "Market", "Advisor", "Maint", "Tx"];
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub entry: LogEntry,
    /// Times this line came in a row.
    pub count: u32,
}

pub struct State {
    pub now: u64,
    /// Per page: rows scrolled from the top (tables), or lines back from the newest (activity).
    pub scroll: [usize; Page::ALL.len()],
    /// Activity: warnings and alerts only.
    pub only_warn: bool,
    /// Market: which direction's terms the ads are listed by.
    pub market_dir: Direction,
    /// Maintenance: which command is picked, counting only the steps that have one. Kept in range where it is read,
    /// since the steps change as the provider does.
    pub pick: usize,
    /// Transactions: the picked row's id, and where it was, for when that transaction is no longer listed.
    tx_pick: Option<(String, usize)>,
    /// Where a transaction's page is on the configured explorer's site. Empty with no explorer: the id is copied bare.
    tx_link: String,
    /// The same for an Ethereum transaction, from `eth_explorer`. Empty: the hash is copied bare.
    eth_link: String,
    /// A command or a link a key asked to copy, until the shell takes it.
    pub copy: Option<String>,

    pub id: Identity,
    pub cap: Capacity,
    /// Whether any capacity line has been read: until one has, `cap` is nothing, not zero.
    pub cap_seen: bool,
    /// When the provider wrote the capacity line `cap` came from, where the line said.
    pub cap_at: Option<u64>,
    pub terms: Option<Terms>,
    /// `Some(true)` healthy, `Some(false)` the provider says its last tick failed, `None` it does not answer.
    pub healthy: Option<bool>,
    pub health_error: String,
    /// Whether an ETH balance is read at all, and the last one read.
    pub shows_eth: bool,
    pub eth: Option<f64>,
    pub log: VecDeque<LogLine>,
    pub swaps: Vec<Swap>,
    pub swaps_ok: bool,
    /// The wants and ads for the pair this provider serves. The board carries every pair: the rest are counted in
    /// `other_wants` and `other_ads` and never drawn as this market.
    pub wants: Vec<Want>,
    pub ads: Vec<Ad>,
    pub other_wants: usize,
    pub other_ads: usize,
    /// Everything the board last listed, whatever its pair, kept so the lists can be drawn again when the pair is learned.
    all_wants: Vec<Want>,
    all_ads: Vec<Ad>,
    /// The pair the provider last said it serves. Kept when `/info` stops answering: a provider that cannot be
    /// reached has not started serving every pair on the board.
    pair: (Option<Asset>, Option<Asset>),
    /// Whether the board's last answer for wants, and for ads, was read.
    pub board_ok: bool,
    pub ads_ok: bool,
    /// Whether the board's ads have ever been read: until they have, an empty list is not an empty market.
    pub ads_seen: bool,
    /// The reverse side as last read, and whether the last read answered.
    pub reverse: Option<ReverseSide>,
    pub reverse_ok: bool,
    /// The configured address until the log names the one the side sells from.
    pub reverse_address: String,
    /// What is unspent at each address as last read (`None` never read), and whether the last read answered.
    pub main_coins: Option<Vec<Coin>>,
    pub main_coins_ok: bool,
    pub reverse_coins: Option<Vec<Coin>>,
    pub reverse_coins_ok: bool,
    /// Whether an explorer is configured at all.
    pub explorer: bool,
    /// What the status command last read at the reverse address, when it read it, and whether its last run answered.
    /// `shows_stock`: whether one is configured at all.
    pub stock: Option<ReverseStock>,
    pub stock_at: u64,
    pub stock_ok: bool,
    pub shows_stock: bool,
    /// The latest transactions at each address as last read (`None` never read), and whether the last read answered.
    pub main_txs: Option<Vec<ChainTx>>,
    pub main_txs_ok: bool,
    pub reverse_txs: Option<Vec<ChainTx>>,
    pub reverse_txs_ok: bool,
    /// Swap transactions asked about by id: `Some(None)` is one the explorer does not know. Whether every one asked last was answered.
    looked: HashMap<String, Option<ChainTx>>,
    pub lookups_ok: bool,
    pub outlook: Option<Outlook>,
    pub outlook_ok: bool,
    /// When each transaction now unconfirmed was first seen by this run.
    first_seen: HashMap<String, u64>,
    /// Transactions at the main address that sent a note elsewhere, and whether one of those notes is still unspent.
    away: HashMap<String, bool>,
    /// What the key said of each: the TAC it paid the reverse key silently, or `None` when none of it did.
    /// The oldest first, `ledger::SILENT_KEPT` at most: never dropped for not being listed, since the feed will not
    /// ask the key twice.
    silent: VecDeque<(String, Option<u128>)>,
    /// The transaction the key-holding tool last gave no answer for. `None`: it answered, or was never asked.
    silent_unread: Option<String>,
    /// When each forward swap now making its pool lock was first seen doing so by this run.
    locking_since: HashMap<String, u64>,
    /// Whether the pool's log on Ethereum is read at all, what was last read of it, and whether the last read answered.
    pub shows_pool: bool,
    pub pool: Option<PoolScan>,
    pub pool_ok: bool,
    tx_stuck_secs: u64,

    pub deadline_warn_secs: u64,
    fee_ladder_bps: Vec<u64>,
    /// A pool account from the config wins over the one the log names.
    pool_account_fixed: bool,
}

impl State {
    pub fn new(cfg: &FastBridgeConfig, shows_eth: bool, explorer: bool, shows_stock: bool) -> Self {
        Self {
            now: 0,
            scroll: [0; Page::ALL.len()],
            only_warn: false,
            market_dir: Direction::Forward,
            pick: 0,
            tx_pick: None,
            // an Esplora API lives under /api of the site whose pages show a transaction at /tx/<id>
            tx_link: Some(cfg.btc_explorer.trim_end_matches('/'))
                .filter(|b| !b.is_empty())
                .map(|b| format!("{}/tx/", b.strip_suffix("/api").unwrap_or(b)))
                .unwrap_or_default(),
            eth_link: Some(cfg.eth_explorer.trim_end_matches('/')).filter(|b| !b.is_empty()).map(|b| format!("{b}/tx/")).unwrap_or_default(),
            copy: None,
            id: Identity { pool_account: cfg.pool_account.clone(), ..Default::default() },
            cap: Capacity::default(),
            cap_seen: false,
            cap_at: None,
            terms: None,
            healthy: None,
            health_error: String::new(),
            shows_eth,
            eth: None,
            log: VecDeque::new(),
            swaps: Vec::new(),
            swaps_ok: false,
            wants: Vec::new(),
            ads: Vec::new(),
            other_wants: 0,
            other_ads: 0,
            all_wants: Vec::new(),
            all_ads: Vec::new(),
            pair: (None, None),
            board_ok: false,
            ads_ok: false,
            ads_seen: false,
            reverse: None,
            reverse_ok: false,
            reverse_address: cfg.reverse_address.clone(),
            main_coins: None,
            main_coins_ok: false,
            reverse_coins: None,
            reverse_coins_ok: false,
            explorer,
            stock: None,
            stock_at: 0,
            stock_ok: false,
            shows_stock,
            main_txs: None,
            main_txs_ok: false,
            reverse_txs: None,
            reverse_txs_ok: false,
            looked: HashMap::new(),
            lookups_ok: false,
            outlook: None,
            outlook_ok: false,
            first_seen: HashMap::new(),
            away: HashMap::new(),
            silent: VecDeque::new(),
            silent_unread: None,
            locking_since: HashMap::new(),
            shows_pool: false,
            pool: None,
            pool_ok: false,
            tx_stuck_secs: cfg.advisor.tx_stuck_secs,
            deadline_warn_secs: cfg.advisor.deadline_warn_secs,
            fee_ladder_bps: cfg.advisor.fee_ladder_bps.clone(),
            pool_account_fixed: !cfg.pool_account.is_empty(),
        }
    }

    /// The clock moving on: a want past its time stops waiting whether or not the board has answered since.
    pub fn tick(&mut self, now: u64) {
        self.now = now;
        self.wants.retain(|w| w.until == 0 || w.until > now);
    }

    /// Draws `wants` and `ads` again from what the board listed, keeping the pair this provider serves. Until the
    /// provider has said what that is, nothing can be told apart and everything is kept.
    fn sift(&mut self) {
        let (btc, evm) = self.pair;
        let now = self.now;
        // amount 0 is how a bridger takes a want down; one past its time is no longer waiting
        let live = |w: &&Want| w.amount > 0 && (w.until == 0 || now == 0 || w.until > now);
        let mut wants: Vec<Want> = self.all_wants.iter().filter(live).filter(|w| btc.is_none() || w.btc_asset == btc).copied().collect();
        wants.sort_by_key(|w| std::cmp::Reverse(w.amount));
        self.other_wants = self.all_wants.iter().filter(live).count() - wants.len();
        self.wants = wants;

        let mut ads: Vec<Ad> =
            self.all_ads.iter().filter(|a| (btc.is_none() || a.terms.btc_asset == btc) && (evm.is_none() || a.terms.evm_asset == evm)).cloned().collect();
        ads.sort_by_key(|a| effective_bps(100 * UNIT, a.terms.fee_bps, a.terms.flat));
        self.other_ads = self.all_ads.len() - ads.len();
        self.ads = ads;
    }

    /// A failed read (`None`) keeps the last value on screen and marks its source as not answering.
    pub fn apply(&mut self, ev: Event) {
        match ev {
            Event::Log(entry) => self.log_entry(entry),
            Event::Terms(t) => {
                let pair = t.as_ref().map(|t| (t.btc_asset, t.evm_asset));
                self.terms = t;
                if let Some(pair) = pair.filter(|p| *p != self.pair) {
                    self.pair = pair;
                    self.sift();
                }
            }
            Event::Health(h) => {
                self.healthy = h.as_ref().map(|h| h.ok);
                self.health_error = h.map(|h| h.error).unwrap_or_default();
            }
            Event::Wants(w) => {
                self.board_ok = w.is_some();
                if let Some(w) = w {
                    self.all_wants = w;
                    self.sift();
                }
            }
            Event::Ads(a) => {
                self.ads_ok = a.is_some();
                self.ads_seen |= a.is_some();
                if let Some(a) = a {
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
                    // when each was first seen making its pool lock, for as long as it is
                    let locking: HashSet<&str> = self.swaps.iter().filter(|s| ledger::locking(s)).map(|s| s.id.as_str()).collect();
                    self.locking_since.retain(|id, _| locking.contains(id.as_str()));
                    for id in locking {
                        if !self.locking_since.contains_key(id) {
                            self.locking_since.insert(id.to_string(), self.now);
                        }
                    }
                }
            }
            Event::Eth(e) => self.eth = e,
            Event::Reverse(r) => {
                self.reverse_ok = r.is_some();
                if r.is_some() {
                    self.reverse = r;
                }
            }
            Event::MainCoins(c) => {
                self.main_coins_ok = c.is_some();
                if c.is_some() {
                    self.main_coins = c;
                }
            }
            Event::Stock(st) => {
                self.stock_ok = st.is_some();
                if st.is_some() {
                    self.stock = st;
                    self.stock_at = self.now;
                }
            }
            Event::ReverseCoins(c) => {
                self.reverse_coins_ok = c.is_some();
                if c.is_some() {
                    self.reverse_coins = c;
                }
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
            Event::Lookups { found, answered, wanted } => {
                self.lookups_ok = answered;
                // pruned by the feed's own list and only here: the feed forgets the same ids in the same round, so
                // nothing it still holds as settled can go missing from this side
                let wanted: HashSet<String> = wanted.into_iter().collect();
                self.looked.retain(|id, _| wanted.contains(id));
                self.looked.extend(found.into_iter().filter(|(id, _)| wanted.contains(id)));
                self.see_unconfirmed();
            }
            Event::NotesAway(found) => {
                self.away.extend(found);
                // a transaction the address no longer lists is not asked about any more
                let listed: HashSet<&str> = self.main_txs.iter().flatten().map(|t| t.txid.as_str()).collect();
                self.away.retain(|id, _| listed.contains(id.as_str()));
            }
            Event::Silent(id, tac) => {
                if !self.silent.iter().any(|(known, _)| *known == id) {
                    self.silent.push_back((id, tac));
                    if self.silent.len() > ledger::SILENT_KEPT {
                        self.silent.pop_front();
                    }
                }
            }
            Event::SilentRead(unread) => self.silent_unread = unread,
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

    /// The picked row's place in `rows`: the transaction picked while it is listed, else the row where it was.
    /// The rows reorder as transactions confirm, so the pick follows the transaction and not the position.
    pub fn tx_picked(&self, rows: &[TxRow]) -> Option<usize> {
        let last = rows.len().checked_sub(1)?;
        Some(match &self.tx_pick {
            Some((id, was)) => rows.iter().position(|r| r.txid == *id).unwrap_or((*was).min(last)),
            None => 0,
        })
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

    /// The commands on the Maintenance page as they paste: one per step that has one, its lines joined with ` \`.
    pub fn commands(&self) -> Vec<String> {
        self.upkeep().steps.iter().filter(|s| !s.cmd.is_empty()).map(|s| s.cmd.join(" \\\n")).collect()
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
            // a table with a cursor: every arrow moves the pick, shifted or not, and the window follows it
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
                    // an Ethereum hash is told by its 0x, and has a site of its own
                    let site = if rows[at].place == Place::Pool { &self.eth_link } else { &self.tx_link };
                    self.copy = Some(format!("{site}{}", rows[at].txid));
                    at
                }
                _ => at,
            };
            self.tx_pick = Some((rows[to].txid.clone(), to));
            return;
        }
        // in the activity feed "up" goes back in time, which is further from the newest line
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
            }
            Some(Signal::Capacity(c)) => {
                self.cap = c.clone();
                self.cap_seen = true;
                self.cap_at = entry.at;
            }
            Some(Signal::ReverseOn(address)) => self.reverse_address.clone_from(address),
            None => {}
        }
        // a line that repeats the one before it (an explorer left alone, the hourly low-TAC warning) is counted, not listed again.
        // Same level only: an alert must not be folded into a quieter line that happens to share its words.
        if let Some(last) = self.log.back_mut()
            && last.entry.msg == entry.msg
            && last.entry.level == entry.level
            && entry.msg != "swap"
            && entry.msg != "capacity"
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

    /// Private TAC not promised to anyone: what the provider says it can take now, or the log's last word.
    /// `None` when neither the provider nor its log has said: there is no figure, and zero would be made up.
    pub fn free(&self) -> Option<u128> {
        self.terms.as_ref().and_then(|t| t.available).or_else(|| self.cap_seen.then(|| self.cap.held.saturating_sub(self.cap.promised)))
    }
    /// Whether the swap records have ever been read: until they have, the totals from them are nothing, not zero.
    pub fn swaps_known(&self) -> bool {
        self.swaps_ok || !self.swaps.is_empty()
    }
    /// How old the log's capacity figures are, where the line gave a time.
    pub fn cap_age(&self) -> Option<u64> {
        self.cap_at.map(|at| self.now.saturating_sub(at))
    }
    /// `Some` when `free` is the log's word and not the provider's answer: how old that word is, where the line gave a time.
    pub fn free_from_log(&self) -> Option<Option<u64>> {
        self.terms.as_ref().and_then(|t| t.available).is_none().then(|| self.cap_at.map(|at| self.now.saturating_sub(at)))
    }
    fn of(&self, d: Direction) -> impl Iterator<Item = &Swap> {
        self.swaps.iter().filter(move |s| s.direction == d)
    }
    /// What bridgers have paid the provider in this direction, by this data directory's records: TAC on Bitcoin
    /// forward, pool TAC in reverse.
    pub fn received(&self, d: Direction) -> u128 {
        total(self.of(d).filter(|s| s.paid()).map(|s| s.pays))
    }
    /// What swaps under way will pay it, once they finish.
    pub fn incoming(&self, d: Direction) -> u128 {
        total(self.of(d).filter(|s| !s.closed() && !s.paid()).map(|s| s.pays))
    }
    pub fn earned(&self, d: Direction) -> u128 {
        total(self.of(d).filter(|s| s.paid()).map(Swap::fee))
    }
    pub fn open(&self, d: Direction) -> usize {
        self.of(d).filter(|s| !s.closed()).count()
    }
    /// Whether any reverse swap has ever been recorded here.
    pub fn has_reverse_swaps(&self) -> bool {
        self.of(Direction::Reverse).next().is_some()
    }
    /// The reverse side's terms while it is on.
    pub fn reverse_terms(&self) -> Option<&Terms> {
        match &self.reverse {
            Some(ReverseSide::On { terms, .. }) => Some(terms),
            _ => None,
        }
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
    /// `None` while it cannot be told: the board's ads were not read, or this provider's key is not known yet.
    pub fn on_board(&self) -> Option<bool> {
        (self.ads_ok && !self.id.claim_pub.is_empty()).then(|| self.my_ad().is_some())
    }
    pub fn name(&self) -> String {
        self.my_ad().map(|a| a.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "provider".into())
    }
    /// The board's ads with the terms of `d`, cheapest first at the reference size. In reverse, only ads that carry readable reverse terms.
    pub fn market(&self, d: Direction) -> Vec<(&Ad, &Terms)> {
        let mut out: Vec<(&Ad, &Terms)> = match d {
            Direction::Forward => self.ads.iter().map(|a| (a, &a.terms)).collect(),
            Direction::Reverse => self.ads.iter().filter_map(|a| a.reverse_terms.as_ref().map(|t| (a, t))).collect(),
        };
        out.sort_by_key(|(_, t)| effective_bps(100 * UNIT, t.fee_bps, t.flat));
        out
    }
    /// Where the reverse fee stands against the other providers' reverse terms. `None` while the side is not on.
    pub fn reverse_standing(&self) -> Option<(advisor::Standing, Option<String>)> {
        let own = self.reverse_terms()?;
        let mine = self.my_ad();
        let rivals: Vec<(u64, &Ad)> = self
            .ads
            .iter()
            .filter(|a| !mine.is_some_and(|m| std::ptr::eq(m, *a)))
            .filter_map(|a| a.reverse_terms.as_ref().filter(|t| advisor::rival(own, t)).map(|t| (advisor::all_in(t.fee_bps, t.flat), a)))
            .collect();
        let fees: Vec<u64> = rivals.iter().map(|(fee, _)| *fee).collect();
        let standing = advisor::advise(own, &[], &self.fee_ladder_bps, self.ads_seen.then_some(fees.as_slice())).standing;
        Some((standing, rivals.iter().min_by_key(|(fee, _)| *fee).map(|(_, a)| a.name.clone())))
    }
    /// `None` while the provider's terms are not known.
    pub fn blocker(&self, w: &Want) -> Option<Option<&'static str>> {
        self.terms.as_ref().map(|t| advisor::blocker(t, self.free(), w))
    }
    /// The stock as read, only while it is of the address the reverse side sells from: one read of another address says nothing of this one.
    pub fn reverse_stock(&self) -> Option<&ReverseStock> {
        self.stock.as_ref().filter(|s| s.address == self.reverse_address)
    }
    pub fn upkeep(&self) -> Upkeep {
        upkeep::upkeep(&Facts {
            swaps: &self.swaps,
            main: self.main_coins.as_deref(),
            reverse_coins: self.reverse_coins.as_deref(),
            reverse_tac: self.reverse_stock().map(|s| s.tac),
            reverse: self.reverse.as_ref(),
            main_address: &self.id.bitcoin_address,
            reverse_address: &self.reverse_address,
            pool_account: &self.id.pool_account,
            free: self.free(),
            forward_min: self.terms.as_ref().map(|t| t.min),
            explorer: self.explorer,
            // a bridger's lock is the bridger's to bump: it is on the Transactions page, not on the to-do list
            stuck: &self.ledger().into_iter().filter(|r| r.stuck && r.role != Some(Role::BridgerLock)).collect::<Vec<_>>(),
            // newest first, as the address lists them: only what the key called a silent send, and only while it waits
            silent: &self
                .main_txs
                .iter()
                .flatten()
                .filter(|t| self.away.get(&t.txid) == Some(&true))
                .filter_map(|t| Some((t.txid.clone(), self.silent.iter().find(|(id, _)| *id == t.txid)?.1?)))
                .collect::<Vec<_>>(),
            silent_unread: self.silent_unread.as_deref(),
            lock_waits: &self
                .pool
                .as_ref()
                .map(|scan| ledger::lock_waits(&self.swaps, scan, &self.locking_since, self.now, self.tx_stuck_secs))
                .unwrap_or_default(),
            hour_milli: self.outlook.map(|o| o.hour_milli),
            now: self.now,
        })
    }
    pub fn advice(&self) -> Option<Advice> {
        let own = self.terms.as_ref()?;
        // the others to be measured against: not this provider's own ad, and only one that could take the same bridge
        let mine = self.my_ad();
        let rivals: Vec<(u64, &Ad)> = self
            .ads
            .iter()
            .filter(|a| !mine.is_some_and(|m| std::ptr::eq(m, *a)) && advisor::rival(own, &a.terms))
            .map(|a| (advisor::all_in(a.terms.fee_bps, a.terms.flat), a))
            .collect();
        let fees: Vec<u64> = rivals.iter().map(|(fee, _)| *fee).collect();
        let mut advice = advisor::advise(own, &self.wants, &self.fee_ladder_bps, self.ads_seen.then_some(fees.as_slice()));
        advice.cheapest_name = rivals.iter().min_by_key(|(fee, _)| *fee).map(|(_, a)| a.name.clone());
        Some(advice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State::new(&FastBridgeConfig::default(), false, false, false)
    }
    fn swap(phase: &str, created: u64, pays: u128, gets: u128) -> Swap {
        Swap { id: format!("{phase}{created}"), phase: phase.into(), created, pays, gets, ..Default::default() }
    }
    fn reverse(phase: &str, created: u64, pays: u128, gets: u128) -> Swap {
        Swap { direction: Direction::Reverse, ..swap(phase, created, pays, gets) }
    }

    #[test]
    fn repeated_log_lines_are_counted_not_listed() {
        let mut st = state();
        let line = || Event::Log(LogEntry::plain(Level::Warn, "an explorer is left alone for a while"));

        st.apply(line());
        st.apply(line());
        st.apply(Event::Log(LogEntry::plain(Level::Info, "listening")));

        let seen: Vec<(&str, u32)> = st.log.iter().map(|l| (l.entry.msg.as_str(), l.count)).collect();
        assert_eq!(seen, [("an explorer is left alone for a while", 2), ("listening", 1)]);
    }

    #[test]
    fn money_is_summed_from_swaps_by_how_they_ended() {
        let mut st = state();
        let swaps = vec![swap("done", 1, 110, 100), swap("offered", 2, 220, 200), swap("refunded", 3, 330, 300)];

        st.apply(Event::Swaps(Some(swaps)));

        assert_eq!((st.received(Direction::Forward), st.incoming(Direction::Forward), st.earned(Direction::Forward)), (110, 220, 10));
        assert_eq!(st.swaps[0].phase, "offered");
    }

    #[test]
    fn each_direction_is_summed_apart_and_a_reverse_claim_is_paid_while_still_open() {
        let mut st = state();
        let swaps = vec![swap("done", 1, 110, 100), reverse("claimed", 2, 505, 500), reverse("offered", 3, 202, 200), reverse("counter-took", 4, 909, 900)];

        st.apply(Event::Swaps(Some(swaps)));

        let by = |d| (st.received(d), st.incoming(d), st.earned(d), st.open(d));
        assert_eq!((by(Direction::Forward), by(Direction::Reverse)), ((110, 0, 10, 0), (505, 202, 5, 2)));
    }

    #[test]
    fn the_market_in_reverse_lists_only_ads_with_reverse_terms_cheapest_first() {
        let mut st = state();
        let t = |bps| Terms { min: 1, max: 9 * UNIT, fee_bps: bps, available: Some(UNIT), ..Default::default() };
        let ad = |name: &str, rev: Option<Terms>| Ad {
            name: name.into(),
            claim_pub: name.into(),
            terms: t(10),
            any_tac: false,
            reverse: rev.is_some(),
            reverse_terms: rev,
        };
        st.apply(Event::Ads(Some(vec![ad("dear", Some(t(90))), ad("forward only", None), ad("cheap", Some(t(20)))])));

        let names = |d| st.market(d).iter().map(|(a, _)| a.name.clone()).collect::<Vec<_>>();

        assert_eq!(names(Direction::Reverse), ["cheap", "dear"]);
        assert_eq!(names(Direction::Forward).len(), 3);
    }

    #[test]
    fn reverse_standing_is_unknown_while_the_side_is_off_and_measured_against_reverse_terms_when_on() {
        let mut st = state();
        let t = |bps| Terms { min: UNIT, max: 9 * UNIT, fee_bps: bps, available: Some(5 * UNIT), ..Default::default() };
        st.apply(Event::Ads(Some(vec![Ad {
            name: "rival".into(),
            claim_pub: "03r".into(),
            terms: t(1),
            any_tac: false,
            reverse: true,
            reverse_terms: Some(t(50)),
        }])));
        st.apply(Event::Reverse(Some(ReverseSide::Off)));
        let off = st.reverse_standing();

        st.apply(Event::Reverse(Some(ReverseSide::On { terms: t(30), open: Some(0), max_open: 2 })));

        assert!(off.is_none());
        assert_eq!(st.reverse_standing(), Some((advisor::Standing::Cheapest { next: 50 }, Some("rival".to_string()))));
    }

    #[test]
    fn a_failed_read_keeps_the_last_value_and_marks_the_source() {
        let mut st = state();
        st.apply(Event::Wants(Some(vec![Want { btc_asset: None, amount: 5, max_fee_bps: None, until: 0 }])));

        st.apply(Event::Wants(None));

        assert_eq!((st.wants.len(), st.board_ok), (1, false));
    }

    #[test]
    fn an_alert_is_not_folded_into_a_quieter_line_with_the_same_words() {
        let mut st = state();

        st.apply(Event::Log(LogEntry::plain(Level::Alert, "swap step failed")));
        st.apply(Event::Log(LogEntry::plain(Level::Info, "swap step failed")));

        let seen: Vec<(Level, u32)> = st.log.iter().map(|l| (l.entry.level, l.count)).collect();
        assert_eq!(seen, [(Level::Alert, 1), (Level::Info, 1)]);
    }

    #[test]
    fn a_second_ad_under_the_same_key_does_not_take_the_name() {
        let mut st = state();
        st.id.claim_pub = "02me".into();
        let own = Terms { min: 10, max: 500, fee_bps: 150, flat: 1, available: None, ..Default::default() };
        let ad = |name: &str, terms: Terms| Ad { name: name.into(), claim_pub: "02me".into(), terms, any_tac: false, reverse: false, reverse_terms: None };
        st.apply(Event::Ads(Some(vec![ad("impostor", Terms { fee_bps: 1, ..own.clone() }), ad("dx-lp", own.clone())])));
        let before_terms = (st.ad_contested(), st.name());

        st.apply(Event::Terms(Some(own)));

        assert_eq!((before_terms, st.name()), ((true, "provider".to_string()), "dx-lp".to_string()));
    }

    #[test]
    fn a_lone_ad_under_the_key_with_other_terms_is_not_this_provider() {
        let mut st = state();
        st.id.claim_pub = "02me".into();
        let own = Terms { min: 10, max: 500, fee_bps: 150, flat: 1, available: None, ..Default::default() };
        let forged = Ad {
            name: "impostor".into(),
            claim_pub: "02me".into(),
            terms: Terms { fee_bps: 1, ..own.clone() },
            any_tac: false,
            reverse: false,
            reverse_terms: None,
        };
        st.apply(Event::Terms(Some(own)));

        st.apply(Event::Ads(Some(vec![forged])));

        assert_eq!((st.on_board(), st.ad_contested(), st.name()), (Some(false), true, "provider".to_string()));
    }

    #[test]
    fn on_board_is_unknown_until_the_key_is_known() {
        let mut st = state();

        st.apply(Event::Ads(Some(vec![])));

        assert_eq!(st.on_board(), None);
    }

    #[test]
    fn free_is_not_known_before_anything_has_said() {
        let mut st = state();
        let before = st.free();

        st.apply(Event::Log(LogEntry {
            signal: Some(Signal::Capacity(Capacity { held: 500, promised: 200, open: 1, holds: 0 })),
            ..LogEntry::plain(Level::Info, "capacity")
        }));

        assert_eq!((before, st.free()), (None, Some(300)));
    }

    #[test]
    fn a_want_stops_waiting_when_its_time_passes_without_a_new_answer() {
        let mut st = state();
        st.apply(Event::Wants(Some(vec![Want { btc_asset: None, amount: 5, max_fee_bps: None, until: 1_000 }])));
        st.apply(Event::Wants(None));

        st.tick(1_001);

        assert_eq!(st.wants.len(), 0);
    }

    #[test]
    fn free_read_from_the_log_says_so_and_how_old_it_is() {
        let mut st = state();
        st.now = 1_000;
        st.apply(Event::Log(LogEntry {
            at: Some(400),
            signal: Some(Signal::Capacity(Capacity { held: 500, promised: 200, open: 1, holds: 0 })),
            ..LogEntry::plain(Level::Info, "capacity")
        }));
        let from_log = st.free_from_log();

        st.apply(Event::Terms(Some(Terms { available: Some(250), ..Default::default() })));

        assert_eq!((from_log, st.free_from_log()), (Some(Some(600)), None));
    }

    #[test]
    fn wants_past_their_time_are_not_counted_as_waiting() {
        let mut st = state();
        st.now = 1_000;
        let want = |until| Want { btc_asset: None, amount: 5, max_fee_bps: None, until };

        st.apply(Event::Wants(Some(vec![want(999), want(1_001), want(0)])));

        assert_eq!(st.wants.len(), 2);
    }

    #[test]
    fn ads_that_cannot_be_read_mark_the_board_as_not_answering() {
        let mut st = state();
        st.apply(Event::Ads(Some(vec![])));

        st.apply(Event::Ads(None));

        assert!(!st.ads_ok);
    }

    #[test]
    fn advice_measures_against_other_providers_that_can_lend_and_not_against_itself() {
        let mut st = state();
        st.id.claim_pub = "02me".into();
        let own = Terms { min: 10 * UNIT, max: 500 * UNIT, fee_bps: 150, flat: 0, available: Some(UNIT), ..Default::default() };
        let ad = |key: &str, bps: u64, available: u128| Ad {
            name: key.into(),
            claim_pub: key.into(),
            terms: Terms { fee_bps: bps, available: Some(available), ..own.clone() },
            any_tac: false,
            reverse: false,
            reverse_terms: None,
        };
        st.apply(Event::Terms(Some(own.clone())));

        let before_any_ads = st.advice().map(|a| a.standing);

        st.apply(Event::Ads(Some(vec![ad("02me", 150, 10 * UNIT), ad("03empty", 10, 0), ad("03other", 200, 10 * UNIT)])));

        let a = st.advice().expect("advice");
        assert_eq!(before_any_ads, Some(advisor::Standing::Unknown));
        assert_eq!((a.standing, a.cheapest_name.as_deref()), (advisor::Standing::Cheapest { next: 200 }, Some("03other")));
    }

    #[test]
    fn only_the_pair_this_provider_serves_is_the_market() {
        let mut st = state();
        let (tac_asset, sats, eth) = (Some(Asset([1; 32])), Some(Asset([0; 32])), Some(Asset([2; 32])));
        let terms = |btc, evm| Terms { btc_asset: btc, evm_asset: evm, min: 1, max: 9, ..Default::default() };
        let ad = |name: &str, btc, evm| Ad {
            name: name.into(),
            claim_pub: name.into(),
            terms: terms(btc, evm),
            any_tac: false,
            reverse: false,
            reverse_terms: None,
        };
        let want = |btc| Want { btc_asset: btc, amount: 5, max_fee_bps: None, until: 0 };
        st.apply(Event::Ads(Some(vec![ad("tac", tac_asset, tac_asset), ad("sats for eth", sats, eth), ad("says nothing", None, None)])));
        st.apply(Event::Wants(Some(vec![want(tac_asset), want(sats), want(None)])));
        let before_the_pair_is_known = (st.ads.len(), st.wants.len());

        st.apply(Event::Terms(Some(terms(tac_asset, tac_asset))));

        assert_eq!(before_the_pair_is_known, (3, 3));
        assert_eq!((st.ads.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), st.other_ads), (vec!["tac"], 2));
        assert_eq!((st.wants.len(), st.other_wants), (1, 2));
    }

    #[test]
    fn the_pair_is_remembered_when_the_provider_stops_answering() {
        let mut st = state();
        let (tac_asset, sats) = (Some(Asset([1; 32])), Some(Asset([0; 32])));
        let want = |btc| Want { btc_asset: btc, amount: 5, max_fee_bps: None, until: 0 };
        st.apply(Event::Terms(Some(Terms { btc_asset: tac_asset, evm_asset: tac_asset, ..Default::default() })));

        st.apply(Event::Terms(None));
        st.apply(Event::Wants(Some(vec![want(tac_asset), want(sats)])));

        assert_eq!((st.wants.len(), st.other_wants), (1, 1));
    }

    #[test]
    fn the_logs_reverse_address_replaces_the_configured_one_and_failed_reads_keep_the_last_coins() {
        let mut st = State::new(&FastBridgeConfig { reverse_address: "bc1qconfigured".into(), ..Default::default() }, false, true, false);
        let coin = Coin { txid: "a".repeat(64), vout: 0, sats: 9_000, confirmed: true };
        st.apply(Event::ReverseCoins(Some(vec![coin.clone()])));

        st.apply(Event::Log(LogEntry { signal: Some(Signal::ReverseOn("bc1qfromthelog".into())), ..LogEntry::plain(Level::Info, "the reverse side is on") }));
        st.apply(Event::ReverseCoins(None));
        st.apply(Event::Reverse(None));

        assert_eq!(st.reverse_address, "bc1qfromthelog");
        assert_eq!((st.reverse_coins, st.reverse_coins_ok, st.reverse, st.reverse_ok), (Some(vec![coin]), false, None, false));
    }

    #[test]
    fn a_stock_read_of_another_address_says_nothing_of_the_reverse_side() {
        let mut st = State::new(&FastBridgeConfig { reverse_address: "bc1qmine".into(), ..Default::default() }, false, true, true);
        let stock = |address: &str| ReverseStock { address: address.into(), notes: 1, tac: 20 * UNIT, sats: Some(1) };

        st.apply(Event::Stock(Some(stock("bc1qother"))));
        let other = st.reverse_stock().is_some();
        st.apply(Event::Stock(Some(stock("bc1qmine"))));
        st.apply(Event::Stock(None));

        assert!(!other);
        assert_eq!((st.reverse_stock().map(|s| s.tac), st.stock_ok), (Some(20 * UNIT), false));
    }

    #[test]
    fn the_picked_command_is_copied_whole_and_the_pick_stays_on_a_command() {
        let mut st = state();
        let shifted = |c| KeyEvent::new(c, KeyModifiers::SHIFT);
        let all = st.commands();

        st.on_key(Page::Maintenance, KeyEvent::from(KeyCode::Down));
        let scrolled = (st.scroll[Page::Maintenance as usize], st.picked());
        st.on_key(Page::Maintenance, shifted(KeyCode::Down));
        st.on_key(Page::Maintenance, KeyEvent::from(KeyCode::Char('J')));
        st.on_key(Page::Maintenance, KeyEvent::from(KeyCode::Char('c')));

        // with nothing read, the steps are: print the reverse address, then back up
        assert_eq!(all, ["scripts/helpers/fast-bridge status", "make backup DEST=<another disk>"]);
        assert_eq!(scrolled, (1, Some(0)));
        assert_eq!((st.picked(), st.copy.as_deref()), (Some(1), Some("make backup DEST=<another disk>")));
    }

    #[test]
    fn a_command_of_several_lines_is_copied_as_one_that_pastes() {
        let mut st = state();
        st.reverse_address = "bc1qrev".into();
        st.explorer = true;
        st.apply(Event::Reverse(Some(ReverseSide::Off)));
        st.apply(Event::ReverseCoins(Some(vec![
            Coin { txid: "f".repeat(64), vout: 0, sats: 30_000, confirmed: true },
            Coin { txid: "f".repeat(64), vout: 1, sats: DUST, confirmed: true },
        ])));

        let cmds = st.commands();

        // two commands, so the first line ends in `&&`: without it the second would paste as more files to edit
        assert_eq!(cmds[0], "sudoedit /etc/tacit-fastbridge-lp/env && \\\n  sudo systemctl restart tacit-fastbridge-lp");
    }

    #[test]
    fn free_prefers_what_the_provider_says_over_the_log() {
        let mut st = state();
        st.apply(Event::Log(LogEntry {
            signal: Some(Signal::Capacity(Capacity { held: 500, promised: 200, open: 1, holds: 0 })),
            ..LogEntry::plain(Level::Info, "capacity")
        }));
        let from_log = st.free();

        st.apply(Event::Terms(Some(Terms { available: Some(250), ..Default::default() })));

        assert_eq!((from_log, st.free()), (Some(300), Some(250)));
    }

    #[test]
    fn an_unconfirmed_transaction_is_timed_from_when_it_was_first_seen_and_a_failed_read_keeps_the_last_list() {
        let mut st = state();
        let tx = |height| ChainTx { txid: "a".repeat(64), fee: 1_000, vsize: 100, height, ..Default::default() };

        st.tick(1_000);
        st.apply(Event::ReverseTxs(Some(vec![tx(None)])));
        st.tick(1_600);
        st.apply(Event::ReverseTxs(Some(vec![tx(None)])));
        st.apply(Event::ReverseTxs(None));
        let waiting = (st.ledger()[0].at, st.ledger()[0].stuck, st.reverse_txs_ok);
        st.tick(3_000);
        let stuck = st.ledger()[0].stuck;
        st.apply(Event::ReverseTxs(Some(vec![tx(Some(7))])));

        assert_eq!((waiting, stuck), ((Some(1_000), false, false), true));
        assert_eq!((st.ledger()[0].status, st.first_seen.len()), (ledger::Status::Confirmed(None), 0));
    }

    #[test]
    fn the_pick_follows_its_transaction_as_rows_reorder_and_enter_copies_its_explorer_link() {
        let cfg = FastBridgeConfig { btc_explorer: "https://explorer.example/api/".into(), ..Default::default() };
        let (mut linked, mut bare) = (State::new(&cfg, false, true, false), state());
        let tx = |c: &str, height| ChainTx { txid: c.repeat(64), fee: 1_000, vsize: 100, height, time: height, ..Default::default() };
        let key = |st: &mut State, code| st.on_key(Page::Transactions, KeyEvent::from(code));
        for st in [&mut linked, &mut bare] {
            st.apply(Event::MainTxs(Some(vec![tx("a", Some(9)), tx("b", Some(8)), tx("c", Some(7))])));
        }

        key(&mut linked, KeyCode::Down);
        key(&mut linked, KeyCode::Down);
        key(&mut linked, KeyCode::Up);
        // a new unconfirmed transaction sorts above every confirmed one: the pick stays on "b"
        linked.apply(Event::MainTxs(Some(vec![tx("d", None), tx("a", Some(9)), tx("b", Some(8)), tx("c", Some(7))])));
        let followed = linked.tx_picked(&linked.ledger());
        key(&mut linked, KeyCode::Enter);
        // one no longer listed leaves the pick where it was
        linked.apply(Event::MainTxs(Some(vec![tx("a", Some(9))])));
        key(&mut bare, KeyCode::Enter);

        assert_eq!((followed, linked.tx_picked(&linked.ledger())), (Some(2), Some(0)));
        assert_eq!(linked.copy, Some(format!("https://explorer.example/tx/{}", "b".repeat(64))));
        assert_eq!(bare.copy, Some("a".repeat(64)));
    }

    #[test]
    fn a_lookup_outlives_a_pass_that_missed_its_record_and_goes_only_when_the_feed_drops_it() {
        let mut st = state();
        let lock = "a".repeat(64);
        let named = || Swap { linked: vec![(Role::BridgerLock, lock.clone())], ..swap("locking", 5, 110, 100) };
        let deep = ChainTx { txid: lock.clone(), height: Some(1), ..Default::default() };

        st.apply(Event::Swaps(Some(vec![named()])));
        st.apply(Event::Lookups { found: vec![(lock.clone(), Some(deep)), ("b".repeat(64), None)], answered: true, wanted: vec![lock.clone()] });
        let kept = st.looked.len();
        // the record is caught mid-write for one pass: the feed still holds the id as settled and will not ask again
        st.apply(Event::Swaps(Some(vec![])));
        st.apply(Event::Swaps(Some(vec![named()])));
        let after_the_gap = st.ledger()[0].status;
        st.apply(Event::Lookups { found: vec![], answered: true, wanted: vec![] });

        assert_eq!((kept, after_the_gap), (1, ledger::Status::Confirmed(None)));
        assert_eq!(st.ledger()[0].status, ledger::Status::Unknown);
    }

    #[test]
    fn a_pool_lock_is_copied_with_the_ethereum_sites_link_and_a_failed_read_keeps_the_last_log() {
        let cfg = FastBridgeConfig { eth_explorer: "https://eth.example/".into(), ..Default::default() };
        let (mut st, leaf) = (State::new(&cfg, false, false, false), "7".repeat(64));
        st.apply(Event::Swaps(Some(vec![Swap { pool_leaf: leaf.clone(), ..swap("offered", 1, 110, 100) }])));
        let lock = PoolLock { leaf, tx: format!("0x{}", "e".repeat(64)), block: 5, time: Some(60) };
        st.apply(Event::Pool(Some(PoolScan { tip: 9, since: 0, locks: vec![lock] })));

        st.apply(Event::Pool(None));
        st.on_key(Page::Transactions, KeyEvent::from(KeyCode::Enter));

        assert_eq!((st.copy.as_deref(), st.pool_ok, st.ledger().len()), (Some(format!("https://eth.example/tx/0x{}", "e".repeat(64)).as_str()), false, 1));
    }

    #[test]
    fn only_a_note_the_key_calls_a_silent_send_is_a_normalize_step_and_only_until_it_moves() {
        let mut st = state();
        let (a, b, c) = ("a".repeat(64), "b".repeat(64), "c".repeat(64));
        let tx = |id: &String| ChainTx { txid: id.clone(), notes_away: vec![0], ..Default::default() };
        let steps = |st: &State| st.commands().iter().filter(|c| c.contains("normalize")).count();
        st.apply(Event::MainTxs(Some(vec![tx(&a), tx(&b), tx(&c)])));
        st.apply(Event::NotesAway(vec![(a.clone(), true), (b.clone(), true), (c.clone(), true)]));
        let unjudged = steps(&st);

        // the key: a paid the reverse side 200 TAC, b went to someone else, and c has not been judged
        st.apply(Event::Silent(a.clone(), Some(200 * UNIT)));
        st.apply(Event::Silent(b.clone(), None));
        let judged = steps(&st);
        st.apply(Event::NotesAway(vec![(a.clone(), false)]));

        assert_eq!((unjudged, judged, steps(&st)), (0, 1, 0));
    }

    #[test]
    fn a_verdict_outlives_a_list_that_missed_its_transaction_and_a_tool_that_gave_no_answer_is_a_step_until_it_does() {
        let mut st = state();
        let a = "a".repeat(64);
        let tx = ChainTx { txid: a.clone(), notes_away: vec![0], ..Default::default() };
        let steps = |st: &State, what: &str| st.upkeep().steps.iter().filter(|s| s.text.contains(what)).count();
        st.apply(Event::MainTxs(Some(vec![tx.clone()])));
        st.apply(Event::NotesAway(vec![(a.clone(), true)]));
        st.apply(Event::Silent(a.clone(), Some(200 * UNIT)));

        // one round the explorer's list is without it, and the feed, which has judged it, does not ask the key again
        st.apply(Event::MainTxs(Some(vec![])));
        st.apply(Event::NotesAway(vec![]));
        st.apply(Event::MainTxs(Some(vec![tx])));
        st.apply(Event::NotesAway(vec![(a.clone(), true)]));
        let back = steps(&st, "sent to the reverse side silently");
        st.apply(Event::SilentRead(Some(a.clone())));
        let failing = steps(&st, "gave no answer");
        st.apply(Event::SilentRead(None));

        assert_eq!((back, failing, steps(&st, "gave no answer")), (1, 1, 0));
    }
}
