//! The BTC and ETH monitor: one provider of Bitcoin for ETH (a fast bridge provider in pair mode), watched through
//! its log, its API, its swap records and the board. Read only. It never sees the provider's key, opens nothing in
//! the provider's data directory but the swap records, and sends the provider nothing but GETs.

pub mod adapters;
mod config;
mod feed;
mod ledger;
mod model;
mod ports;
mod state;
mod ui;

pub use config::BtcEthConfig;

use crate::{
    fmt::now,
    shell::{
        monitor::{Monitor, Wake},
        theme::Theme,
    },
};
use feed::Event;
use ports::Ports;
use ratatui::{Frame, crossterm::event::KeyEvent, layout::Rect};
use state::{Page, State};
use tokio::sync::mpsc::Receiver;

pub struct BtcEth {
    cfg: BtcEthConfig,
    state: State,
    /// Held until `start` hands them to the feed.
    ports: Option<Ports>,
    events: Option<Receiver<Event>>,
}

pub fn create_btc_eth(cfg: &BtcEthConfig, demo: bool) -> BtcEth {
    let ports = if demo { adapters::memory::create_demo_ports(now()) } else { adapters::create_ports(cfg) };
    // a demo shows nothing of the operator's: the pool account in their config would otherwise sit in the header
    let cfg = if demo { BtcEthConfig { pool_account: String::new(), ..cfg.clone() } } else { cfg.clone() };
    let state = State::new(&cfg, ports.gas.is_some(), ports.chain.is_some(), ports.pool.is_some());
    BtcEth { state, cfg, ports: Some(ports), events: None }
}

impl Monitor for BtcEth {
    fn id(&self) -> &'static str {
        "btc-eth"
    }
    fn subject(&self) -> String {
        self.state.name()
    }
    fn pages(&self) -> &'static [&'static str] {
        &Page::TITLES
    }
    fn start(&mut self, wake: Wake) {
        if let Some(ports) = self.ports.take() {
            self.events = Some(feed::start(ports, &self.cfg.refresh, self.cfg.pool_account.clone(), wake));
        }
    }
    fn refresh(&mut self, now: u64) {
        self.state.tick(now);
        if let Some(rx) = &mut self.events {
            while let Ok(ev) = rx.try_recv() {
                self.state.apply(ev);
            }
        }
    }
    fn on_key(&mut self, page: usize, key: KeyEvent) {
        self.state.on_key(Page::ALL[page], key);
    }
    fn header_height(&self, width: u16, th: &Theme) -> u16 {
        u16::try_from(ui::header_lines(&self.state, th, width).len()).unwrap_or(u16::MAX)
    }
    fn draw_header(&self, f: &mut Frame, area: Rect, th: &Theme) {
        ui::header(f, area, &self.state, th);
    }
    fn draw_page(&self, f: &mut Frame, area: Rect, page: usize, th: &Theme) {
        ui::page(f, area, Page::ALL[page], &self.state, th);
    }
    fn take_copy(&mut self) -> Option<String> {
        self.state.copy.take()
    }
    fn hints(&self, page: usize) -> &'static str {
        match Page::ALL[page] {
            Page::Transactions => "↑↓ pick a transaction · enter or c copy its explorer link",
            Page::Activity => "↑↓ scroll · g newest · w warnings only",
            Page::Market => "↑↓ scroll · d direction",
            _ => "↑↓ scroll",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_demo_shows_nothing_from_the_operators_config() {
        let cfg = BtcEthConfig { pool_account: "0xmine".into(), ..Default::default() };

        let (demo, live) = (create_btc_eth(&cfg, true), create_btc_eth(&cfg, false));

        assert_eq!((demo.state.id.pool_account.as_str(), demo.cfg.pool_account.as_str(), live.state.id.pool_account.as_str()), ("", "", "0xmine"));
    }
}
