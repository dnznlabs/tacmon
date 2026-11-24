//! The fast bridge monitor: one liquidity provider, watched through its log, its API, its swap records and the board.
//! Read only. It never sees the provider's key and sends the provider nothing but GETs.

pub mod adapters;
mod advisor;
mod config;
mod feed;
mod model;
mod ports;
mod state;
mod ui;

pub use config::FastBridgeConfig;

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

pub struct FastBridge {
    cfg: FastBridgeConfig,
    state: State,
    /// Held until `start` hands them to the feed.
    ports: Option<Ports>,
    events: Option<Receiver<Event>>,
}

pub fn create_fast_bridge(cfg: &FastBridgeConfig, demo: bool) -> FastBridge {
    let ports = if demo { adapters::memory::create_demo_ports(now()) } else { adapters::create_ports(cfg) };
    // a demo shows nothing of the operator's: the pool account in their config would otherwise sit in the header
    let cfg = if demo { FastBridgeConfig { pool_account: String::new(), ..cfg.clone() } } else { cfg.clone() };
    FastBridge { state: State::new(&cfg, ports.gas.is_some()), cfg, ports: Some(ports), events: None }
}

impl Monitor for FastBridge {
    fn id(&self) -> &'static str {
        "fast-bridge"
    }
    fn title(&self) -> &'static str {
        "Fast bridge"
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
    fn hints(&self, page: usize) -> &'static str {
        match Page::ALL[page] {
            Page::Activity => "↑↓ scroll · g newest · w warnings only",
            _ => "↑↓ scroll",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_demo_shows_nothing_from_the_operators_config() {
        let cfg = FastBridgeConfig { pool_account: "0xmine".into(), ..Default::default() };

        let (demo, live) = (create_fast_bridge(&cfg, true), create_fast_bridge(&cfg, false));

        assert_eq!((demo.state.id.pool_account.as_str(), demo.cfg.pool_account.as_str(), live.state.id.pool_account.as_str()), ("", "", "0xmine"));
    }
}
