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
    pub wants: Vec<Want>,
    pub ads: Vec<Ad>,
    /// Whether the board's last answer for wants, and for ads, was read.
    pub board_ok: bool,
    pub ads_ok: bool,
    /// Whether the board's ads have ever been read: until they have, an empty list is not an empty market.
    pub ads_seen: bool,

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
            board_ok: false,
            ads_ok: false,
            ads_seen: false,
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
                self.ads_seen |= a.is_some();
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
                self.cap_seen = true;
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
    /// `None` while the provider's terms are not known.
    pub fn blocker(&self, w: &Want) -> Option<Option<&'static str>> {
        self.terms.as_ref().map(|t| advisor::blocker(t, self.free(), w))
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
    fn a_lone_ad_under_the_key_with_other_terms_is_not_this_provider() {
        let mut st = state();
        st.id.claim_pub = "02me".into();
        let own = Terms { min: 10, max: 500, fee_bps: 150, flat: 1, available: None };
        let forged = Ad { name: "impostor".into(), claim_pub: "02me".into(), terms: Terms { fee_bps: 1, ..own.clone() }, any_tac: false, reverse: false };
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
        st.apply(Event::Wants(Some(vec![Want { amount: 5, max_fee_bps: None, until: 1_000 }])));
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
    fn advice_measures_against_other_providers_that_can_lend_and_not_against_itself() {
        let mut st = state();
        st.id.claim_pub = "02me".into();
        let own = Terms { min: 10 * UNIT, max: 500 * UNIT, fee_bps: 150, flat: 0, available: Some(UNIT) };
        let ad = |key: &str, bps: u64, available: u128| Ad {
            name: key.into(),
            claim_pub: key.into(),
            terms: Terms { fee_bps: bps, available: Some(available), ..own.clone() },
            any_tac: false,
            reverse: false,
        };
        st.apply(Event::Terms(Some(own.clone())));

        let before_any_ads = st.advice().map(|a| a.standing);

        st.apply(Event::Ads(Some(vec![ad("02me", 150, 10 * UNIT), ad("03empty", 10, 0), ad("03other", 200, 10 * UNIT)])));

        let a = st.advice().expect("advice");
        assert_eq!(before_any_ads, Some(advisor::Standing::Unknown));
        assert_eq!((a.standing, a.cheapest_name.as_deref()), (advisor::Standing::Cheapest { next: 200 }, Some("03other")));
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
}
