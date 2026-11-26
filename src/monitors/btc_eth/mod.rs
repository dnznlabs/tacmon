//! The BTC and ETH monitor: one provider of Bitcoin for ETH (a fast bridge provider in pair mode), watched through
//! its log, its API, its swap records and the board. Read only. It never sees the provider's key, opens nothing in
//! the provider's data directory but the swap records, and sends the provider nothing but GETs.

pub mod adapters;
mod advisor;
mod config;
mod feed;
mod ledger;
mod model;
mod ports;
mod state;
mod ui;
mod upkeep;

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
    // and where their provider is on this machine in the commands the Maintenance page prints
    let cfg = if demo {
        let made_up = BtcEthConfig::default();
        BtcEthConfig {
            pool_account: String::new(),
            reverse_address: String::new(),
            data_dir: made_up.data_dir,
            log_cmd: made_up.log_cmd,
            lp_url: made_up.lp_url,
            ..cfg.clone()
        }
    } else {
        cfg.clone()
    };
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
    fn page_aliases(&self) -> &'static [(&'static str, usize)] {
        // the tabs' full names, as the fast bridge monitor takes them
        &[("activity", Page::Activity as usize), ("maintenance", Page::Maintenance as usize), ("transactions", Page::Transactions as usize)]
    }
    fn start(&mut self, wake: Wake) {
        if let Some(ports) = self.ports.take() {
            self.events = Some(feed::start(ports, &self.cfg.refresh, self.cfg.pool_account.clone(), self.cfg.reverse_address.clone(), wake));
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
            Page::Maintenance => "↑↓ scroll · shift+↑↓ pick a command · c copy it · each step says where its command runs from",
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
        let cfg = BtcEthConfig { pool_account: "0xmine".into(), data_dir: "/srv/mine".into(), ..Default::default() };

        let (demo, live) = (create_btc_eth(&cfg, true), create_btc_eth(&cfg, false));

        assert_eq!((demo.state.id.pool_account.as_str(), demo.cfg.pool_account.as_str(), live.state.id.pool_account.as_str()), ("", "", "0xmine"));
        // the commands the Maintenance page prints name where the provider's data is
        let names_it = |m: &BtcEth| m.state.commands().iter().any(|c| c.contains("/srv/mine"));
        assert!(!names_it(&demo) && names_it(&live));
    }
}
