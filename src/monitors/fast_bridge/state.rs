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
    pub board_ok: bool,

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
                    // amount 0 is how a bridger takes a want down
                    w.retain(|w| w.amount > 0);
                    w.sort_by_key(|w| std::cmp::Reverse(w.amount));
                    self.wants = w;
                }
            }
            Event::Ads(a) => {
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
            Some(Signal::Capacity(c)) => self.cap = c.clone(),
            None => {}
        }
        // a line that repeats the one before it (an explorer left alone, the hourly low-TAC warning) is counted, not listed again
        if let Some(last) = self.log.back_mut()
            && last.entry.msg == entry.msg
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
    /// TAC on Bitcoin taken by finished swaps, by this data directory's records.
    pub fn received(&self) -> u128 {
        self.swaps.iter().filter(|s| s.done()).map(|s| s.pays).sum()
    }
    /// TAC on Bitcoin that open swaps will pay.
    pub fn incoming(&self) -> u128 {
        self.swaps.iter().filter(|s| !s.closed()).map(|s| s.pays).sum()
    }
    pub fn earned(&self) -> u128 {
        self.swaps.iter().filter(|s| s.done()).map(Swap::fee).sum()
    }
    pub fn my_ad(&self) -> Option<&Ad> {
        self.ads.iter().find(|a| !self.id.claim_pub.is_empty() && a.claim_pub == self.id.claim_pub)
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
