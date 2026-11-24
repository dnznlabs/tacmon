//! The port a monitor implements. The shell knows nothing of what a monitor watches: it gives each one
//! a header area, a page area and the keys it does not use itself.

use super::{Msg, theme::Theme};
use ratatui::{Frame, crossterm::event::KeyEvent, layout::Rect};
use tokio::sync::mpsc::UnboundedSender;

/// How a monitor's readers tell the shell there is something new to draw.
#[derive(Clone)]
pub struct Wake(UnboundedSender<Msg>);

impl Wake {
    pub fn new(tx: UnboundedSender<Msg>) -> Self {
        Self(tx)
    }
    pub fn wake(&self) {
        // the shell has gone: nothing is left to draw for
        let _ = self.0.send(Msg::Wake);
    }
}

pub trait Monitor {
    /// The name on the command line and in `--snapshot` ("fast-bridge").
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    /// What this instance is watching (a provider's name), shown beside the title.
    fn subject(&self) -> String;
    fn pages(&self) -> &'static [&'static str];
    /// Starts the readers. Called once, inside the runtime, before the first draw.
    fn start(&mut self, wake: Wake);
    /// Takes in what the readers have sent since the last call. Called before every draw.
    fn refresh(&mut self, now: u64);
    /// A key the shell does not use itself (scrolling, a page's own toggles).
    fn on_key(&mut self, page: usize, key: KeyEvent);
    /// Lines inside the header frame, which the shell draws.
    fn header_height(&self) -> u16;
    fn draw_header(&self, f: &mut Frame, area: Rect, th: &Theme);
    fn draw_page(&self, f: &mut Frame, area: Rect, page: usize, th: &Theme);
    /// The page's own keys, for the footer.
    fn hints(&self, page: usize) -> &'static str;
}
