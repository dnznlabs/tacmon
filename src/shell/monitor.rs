//! The port a monitor implements. The shell knows nothing of what a monitor watches: it gives each one
//! a header area, a page area and the keys it does not use itself.

use super::{Msg, theme::Theme};
use ratatui::{Frame, crossterm::event::KeyEvent, layout::Rect};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc::UnboundedSender;

/// How a monitor's readers tell the shell there is something new to draw.
#[derive(Clone)]
pub struct Wake {
    tx: UnboundedSender<Msg>,
    /// Set while a wake is waiting to be acted on, and cleared by the shell when it takes in what is new.
    pending: Arc<AtomicBool>,
}

impl Wake {
    pub fn new(tx: UnboundedSender<Msg>, pending: Arc<AtomicBool>) -> Self {
        Self { tx, pending }
    }
    /// At most one wake waits at a time, however fast a reader produces: the queue cannot grow with the log.
    pub fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            // an error means the shell has gone: nothing is left to draw for
            let _ = self.tx.send(Msg::Wake);
        }
    }
}

pub trait Monitor {
    /// The name on the command line and in `--snapshot` ("fast-bridge").
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    /// What this instance is watching (a provider's name), shown beside the title.
    fn subject(&self) -> String;
    fn pages(&self) -> &'static [&'static str];
    /// Other names `--snapshot` takes for a page, each with the page's place: a tab that was renamed keeps its old name here.
    fn page_aliases(&self) -> &'static [(&'static str, usize)] {
        &[]
    }
    /// Starts the readers. Called once, inside the runtime, before the first draw.
    fn start(&mut self, wake: Wake);
    /// Takes in what the readers have sent since the last call. Called before every draw.
    fn refresh(&mut self, now: u64);
    /// A key the shell does not use itself (scrolling, a page's own toggles).
    fn on_key(&mut self, page: usize, key: KeyEvent);
    /// Lines the header needs inside its frame at this width. The shell draws the frame.
    fn header_height(&self, width: u16, th: &Theme) -> u16;
    fn draw_header(&self, f: &mut Frame, area: Rect, th: &Theme);
    fn draw_page(&self, f: &mut Frame, area: Rect, page: usize, th: &Theme);
    /// The page's own keys, for the footer.
    fn hints(&self, page: usize) -> &'static str;
    /// Text a key asked to put on the clipboard, taken once. The shell sends it to the terminal's clipboard.
    fn take_copy(&mut self) -> Option<String> {
        None
    }
}
