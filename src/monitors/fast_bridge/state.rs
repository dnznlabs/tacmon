//! What the pages draw, and what each event from the feed does to it.

use super::{
    advisor::{self, Advice},
    config::FastBridgeConfig,
    feed::Event,
    model::*,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::collections::VecDeque;

const MAX_LOG: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Activity,
    Demand,
    Market,
    Advisor,
}

impl Page {
    pub const ALL: [Page; 5] = [Page::Overview, Page::Activity, Page::Demand, Page::Market, Page::Advisor];
    pub const TITLES: [&'static str; 5] = ["Overview", "Activity", "Demand", "Market", "Advisor"];
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

    pub id: Identity,
    pub cap: Capacity,
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
    pub wants: Vec<Want>,
    pub ads: Vec<Ad>,
    /// Whether the board's last answer for wants, and for ads, was read.
    pub board_ok: bool,
    pub ads_ok: bool,

    pub deadline_warn_secs: u64,
    fee_ladder_bps: Vec<u64>,
    /// A pool account from the config wins over the one the log names.
    pool_account_fixed: bool,
}

impl State {
    pub fn new(cfg: &FastBridgeConfig, shows_eth: bool) -> Self {
        Self {
            now: 0,
            scroll: [0; Page::ALL.len()],
            only_warn: false,
            id: Identity { pool_account: cfg.pool_account.clone(), ..Default::default() },
            cap: Capacity::default(),
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
            board_ok: false,
            ads_ok: false,
            deadline_warn_secs: cfg.advisor.deadline_warn_secs,
            fee_ladder_bps: cfg.advisor.fee_ladder_bps.clone(),
            pool_account_fixed: !cfg.pool_account.is_empty(),
        }
    }

    /// A failed read (`None`) keeps the last value on screen and marks its source as not answering.
    pub fn apply(&mut self, ev: Event) {
        match ev {
            Event::Log(entry) => self.log_entry(entry),
            Event::Terms(t) => self.terms = t,
            Event::Health(h) => {
                self.healthy = h.as_ref().map(|h| h.ok);
                self.health_error = h.map(|h| h.error).unwrap_or_default();
            }
            Event::Wants(w) => {
                self.board_ok = w.is_some();
                if let Some(mut w) = w {
                    // amount 0 is how a bridger takes a want down; one past its time is no longer waiting
                    let now = self.now;
                    w.retain(|w| w.amount > 0 && (w.until == 0 || now == 0 || w.until > now));
                    w.sort_by_key(|w| std::cmp::Reverse(w.amount));
                    self.wants = w;
                }
            }
            Event::Ads(a) => {
                self.ads_ok = a.is_some();
                if let Some(mut a) = a {
                    a.sort_by_key(|a| effective_bps(100 * UNIT, a.terms.fee_bps, a.terms.flat));
                    self.ads = a;
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
        }
    }

    pub fn on_key(&mut self, page: Page, k: KeyEvent) {
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
                self.cap_at = entry.at;
            }
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
    pub fn free(&self) -> u128 {
        self.terms.as_ref().and_then(|t| t.available).unwrap_or(self.cap.held.saturating_sub(self.cap.promised))
    }
    /// `Some` when `free` is the log's word and not the provider's answer: how old that word is, where the line gave a time.
    pub fn free_from_log(&self) -> Option<Option<u64>> {
        self.terms.as_ref().and_then(|t| t.available).is_none().then(|| self.cap_at.map(|at| self.now.saturating_sub(at)))
    }
    /// TAC on Bitcoin taken by finished swaps, by this data directory's records.
    pub fn received(&self) -> u128 {
        total(self.swaps.iter().filter(|s| s.done()).map(|s| s.pays))
    }
    /// TAC on Bitcoin that open swaps will pay.
    pub fn incoming(&self) -> u128 {
        total(self.swaps.iter().filter(|s| !s.closed()).map(|s| s.pays))
    }
    pub fn earned(&self) -> u128 {
        total(self.swaps.iter().filter(|s| s.done()).map(Swap::fee))
    }
    /// Every ad posted under this provider's claim key. The board checks no signature, so anyone can post one.
    fn my_ads(&self) -> impl Iterator<Item = &Ad> {
        self.ads.iter().filter(|a| !self.id.claim_pub.is_empty() && a.claim_pub == self.id.claim_pub)
    }
    /// More than one ad uses this provider's key: at most one of them is its own.
    pub fn ad_contested(&self) -> bool {
        self.my_ads().nth(1).is_some()
    }
    /// The ad that is this provider's: the only one under its key, or among several the one carrying the terms it reports itself.
    pub fn my_ad(&self) -> Option<&Ad> {
        if !self.ad_contested() {
            return self.my_ads().next();
        }
        let own = self.terms.as_ref()?;
        self.my_ads().find(|a| (a.terms.min, a.terms.max, a.terms.fee_bps, a.terms.flat) == (own.min, own.max, own.fee_bps, own.flat))
    }
    pub fn name(&self) -> String {
        self.my_ad().map(|a| a.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "provider".into())
    }
    /// `None` while the provider's terms are not known.
    pub fn blocker(&self, w: &Want) -> Option<Option<&'static str>> {
        self.terms.as_ref().map(|t| advisor::blocker(t, self.free(), w))
    }
    pub fn advice(&self) -> Option<Advice> {
        self.terms.as_ref().map(|t| advisor::advise(t, &self.wants, &self.fee_ladder_bps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State::new(&FastBridgeConfig::default(), false)
    }
    fn swap(phase: &str, created: u64, pays: u128, gets: u128) -> Swap {
        Swap { id: format!("{phase}{created}"), phase: phase.into(), created, deadline: 0, pays, gets, lock_txid: String::new() }
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

        assert_eq!((st.received(), st.incoming(), st.earned()), (110, 220, 10));
        assert_eq!(st.swaps[0].phase, "offered");
    }

    #[test]
    fn a_failed_read_keeps_the_last_value_and_marks_the_source() {
        let mut st = state();
        st.apply(Event::Wants(Some(vec![Want { amount: 5, max_fee_bps: None, until: 0 }])));

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
        let own = Terms { min: 10, max: 500, fee_bps: 150, flat: 1, available: None };
        let ad = |name: &str, terms: Terms| Ad { name: name.into(), claim_pub: "02me".into(), terms, any_tac: false, reverse: false };
        st.apply(Event::Ads(Some(vec![ad("impostor", Terms { fee_bps: 1, ..own.clone() }), ad("dx-lp", own.clone())])));
        let before_terms = (st.ad_contested(), st.name());

        st.apply(Event::Terms(Some(own)));

        assert_eq!((before_terms, st.name()), ((true, "provider".to_string()), "dx-lp".to_string()));
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
        let want = |until| Want { amount: 5, max_fee_bps: None, until };

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
    fn free_prefers_what_the_provider_says_over_the_log() {
        let mut st = state();
        st.apply(Event::Log(LogEntry {
            signal: Some(Signal::Capacity(Capacity { held: 500, promised: 200, open: 1, holds: 0 })),
            ..LogEntry::plain(Level::Info, "capacity")
        }));
        let from_log = st.free();

        st.apply(Event::Terms(Some(Terms { available: Some(250), ..Default::default() })));

        assert_eq!((from_log, st.free()), (300, 250));
    }
}
