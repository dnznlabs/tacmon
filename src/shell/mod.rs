//! The shell: the terminal, the keys every monitor shares, and the frame around whichever monitor is in front.
//! Two levels: a monitor (fast bridge, later others), then that monitor's pages.

pub mod monitor;
mod splash;
pub mod theme;
pub mod widgets;

use crate::fmt::now;
use anyhow::{Result, anyhow};
use monitor::{Monitor, Wake};
use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Tabs},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use theme::Theme;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub enum Msg {
    Key(KeyEvent),
    /// Once a second, so countdowns move when nothing else does.
    Tick,
    /// A monitor has something new.
    Wake,
}

pub struct Shell {
    monitors: Vec<Box<dyn Monitor>>,
    theme: Theme,
    at: usize,
    page: usize,
    quit: bool,
    tx: UnboundedSender<Msg>,
    rx: UnboundedReceiver<Msg>,
    /// Shared with every monitor's `Wake`: see there.
    pending: Arc<AtomicBool>,
    /// What the footer says in place of the keys for a moment, and until when.
    notice: Option<(String, u64)>,
    /// Esc asked whether to quit: it is too easy to hit by accident to quit on its own.
    asking: bool,
}

/// The most sent to the clipboard at once. Terminals cap OSC 52 near here; the commands copied are a few hundred bytes.
const MAX_COPY: usize = 8 << 10;

/// Standard base64, padded: what OSC 52 carries.
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { char::from(ABC[(n >> (18 - 6 * i) & 63) as usize]) } else { '=' });
        }
    }
    out
}

/// The escape that asks the terminal to put `text` on the clipboard, or `None` for text too long to send. Control
/// characters other than a newline are dropped first: the text is meant to be pasted into a shell, and base64 keeps
/// it from reaching the terminal itself either way.
fn osc52(text: &str) -> Option<String> {
    let clean: String = text.chars().filter(|c| *c == '\n' || !c.is_control()).collect();
    (clean.len() <= MAX_COPY).then(|| format!("\x1b]52;c;{}\x07", base64(clean.as_bytes())))
}

impl Shell {
    /// Must be called inside the runtime: it starts every monitor's readers.
    pub fn new(mut monitors: Vec<Box<dyn Monitor>>, theme: Theme) -> Result<Self> {
        if monitors.is_empty() {
            return Err(anyhow!("no monitor is enabled in the configuration"));
        }
        let (tx, rx) = unbounded_channel();
        let pending = Arc::new(AtomicBool::new(false));
        for m in &mut monitors {
            m.start(Wake::new(tx.clone(), pending.clone()));
        }
        Ok(Self { monitors, theme, at: 0, page: 0, quit: false, tx, rx, pending, notice: None, asking: false })
    }

    /// `page` or `monitor/page`, by name, as `--snapshot` takes it.
    pub fn show(&mut self, target: &str) -> Result<()> {
        let (monitor, page) = target.split_once('/').map_or((None, target), |(m, p)| (Some(m), p));
        if let Some(id) = monitor {
            self.at = self.monitors.iter().position(|m| m.id().eq_ignore_ascii_case(id)).ok_or_else(|| anyhow!("no monitor called \"{id}\""))?;
        }
        let pages = self.monitors[self.at].pages();
        let alias = || self.monitors[self.at].page_aliases().iter().find(|(name, at)| name.eq_ignore_ascii_case(page) && *at < pages.len()).map(|(_, at)| *at);
        self.page = pages
            .iter()
            .position(|p| p.eq_ignore_ascii_case(page))
            .or_else(alias)
            .ok_or_else(|| anyhow!("no page called \"{page}\" (pages: {})", pages.join(", ").to_lowercase()))?;
        Ok(())
    }

    /// Gathers for `wait`, then draws the current page once and returns it as plain text.
    pub async fn snapshot(&mut self, wait: Duration, width: u16, height: u16) -> Result<String> {
        // capped: an instant an absurd wait away cannot be represented
        let until = tokio::time::Instant::now() + wait.min(Duration::from_secs(3600));
        // taken in as it arrives: the readers' queues are bounded, and they wait while nothing drains them
        while let Ok(Some(_)) = tokio::time::timeout_at(until, self.rx.recv()).await {
            self.refresh();
        }
        self.refresh();
        let mut term = Terminal::new(TestBackend::new(width, height))?;
        term.draw(|f| self.draw(f))?;
        Ok(text_of(term.backend().buffer()))
    }

    /// The start screen as plain text, fully drawn: what `--snapshot splash` prints.
    pub fn splash_snapshot(&self, width: u16, height: u16) -> Result<String> {
        let mut term = Terminal::new(TestBackend::new(width, height))?;
        term.draw(|f| splash::draw(f, 1.0, &self.theme))?;
        Ok(text_of(term.backend().buffer()))
    }

    pub async fn run(&mut self, splash: bool) -> Result<()> {
        self.spawn_input();
        let tick = self.tx.clone();
        tokio::spawn(async move {
            while tick.send(Msg::Tick).is_ok() {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        let mut term = ratatui::init();
        let res = async {
            // the monitors are already reading, so the start screen costs no time the first page would not
            let began = tokio::time::Instant::now();
            while splash && began.elapsed() < splash::LENGTH + splash::HOLD {
                self.refresh();
                term.draw(|f| splash::draw(f, (began.elapsed().as_secs_f32() / splash::LENGTH.as_secs_f32()).min(1.0), &self.theme))?;
                // any key ends it; the key itself is not acted on, so a stray "q" does not quit before anything is seen
                if let Ok(Some(Msg::Key(_))) = tokio::time::timeout(splash::FRAME, self.rx.recv()).await {
                    break;
                }
            }
            while !self.quit {
                self.refresh();
                term.draw(|f| self.draw(f))?;
                // one draw for everything already waiting: a start reads thousands of log lines at once
                let Some(msg) = self.rx.recv().await else { break };
                self.on_msg(msg);
                while let Ok(msg) = self.rx.try_recv() {
                    self.on_msg(msg);
                }
                if let Some(text) = self.monitors[self.at].take_copy() {
                    self.copy(&text);
                }
            }
            anyhow::Ok(())
        }
        .await;
        ratatui::restore();
        res
    }

    /// The keyboard is read on a thread of its own: crossterm's read blocks.
    fn spawn_input(&self) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            while let Ok(ev) = event::read() {
                let msg = match ev {
                    Event::Key(k) if k.kind == KeyEventKind::Press => Msg::Key(k),
                    Event::Resize(..) => Msg::Wake,
                    _ => continue,
                };
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });
    }

    fn refresh(&mut self) {
        // before the monitors are read: anything sent after this asks for another draw
        self.pending.store(false, Ordering::Release);
        let now = now();
        for m in &mut self.monitors {
            m.refresh(now);
        }
    }

    /// Whether it landed is the terminal's business: one with OSC 52 off drops it silently, so the notice says what was sent, not that it arrived.
    fn copy(&mut self, text: &str) {
        use std::io::Write;
        let said = match osc52(text) {
            Some(seq) => {
                let mut out = std::io::stdout();
                match out.write_all(seq.as_bytes()).and_then(|()| out.flush()) {
                    Ok(()) => format!(" sent {} characters to the clipboard (OSC 52: a terminal with it off ignores it)", text.chars().count()),
                    Err(e) => format!(" could not reach the terminal: {e}"),
                }
            }
            None => format!(" not copied: over {MAX_COPY} bytes"),
        };
        self.notice = Some((said, now() + 4));
    }

    fn on_msg(&mut self, msg: Msg) {
        if let Msg::Key(k) = msg {
            self.on_key(k);
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if self.asking {
            // only a yes quits; Esc again, or anything else, goes back to where it was
            self.asking = false;
            self.quit = matches!(k.code, KeyCode::Char('y' | 'Y' | 'q') | KeyCode::Enter);
            return;
        }
        let pages = self.monitors[self.at].pages().len();
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => self.asking = true,
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Char(c @ '1'..='9') if (c as usize - '1' as usize) < pages => self.page = c as usize - '1' as usize,
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.page = (self.page + 1) % pages,
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.page = (self.page + pages - 1) % pages,
            KeyCode::Char('m') if self.monitors.len() > 1 => {
                self.at = (self.at + 1) % self.monitors.len();
                self.page = 0;
            }
            _ => self.monitors[self.at].on_key(self.page, k),
        }
    }

    fn draw(&self, f: &mut Frame) {
        let (th, m) = (&self.theme, &self.monitors[self.at]);
        let many = self.monitors.len() > 1;
        let [bar, head, tabs, body, foot] = Layout::vertical([
            Constraint::Length(u16::from(many)),
            Constraint::Length(m.header_height(f.area().width.saturating_sub(2), th) + 2),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(f.area());
        let picked = Style::new().fg(th.accent).add_modifier(Modifier::BOLD | Modifier::REVERSED);
        if many {
            f.render_widget(Tabs::new(self.monitors.iter().map(|m| m.title())).select(self.at).style(th.dim()).highlight_style(picked), bar);
        }
        let title = Line::from(vec![
            Span::styled(" tacmon ", Style::new().fg(th.accent).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} ", m.title().to_lowercase()), th.dim()),
            Span::raw(format!("{} ", m.subject())),
        ]);
        let frame = Block::bordered().border_style(Style::new().fg(th.accent)).title(title);
        m.draw_header(f, frame.inner(head), th);
        f.render_widget(frame, head);
        let titles = m.pages().iter().enumerate().map(|(i, p)| format!("{} {p}", i + 1));
        f.render_widget(Tabs::new(titles).select(self.page).style(th.dim()).highlight_style(picked), tabs);
        m.draw_page(f, body, self.page, th);
        if self.asking {
            let ask = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("  Quit tacmon?", Style::new().fg(th.warn).add_modifier(Modifier::BOLD))),
                Line::from(""),
                Line::from(Span::styled("  y, q or Enter quits · Esc or any other key goes back", th.dim())),
            ])
            .block(Block::bordered().border_style(Style::new().fg(th.warn)));
            let [_, mid, _] = Layout::vertical([Constraint::Fill(1), Constraint::Length(6), Constraint::Fill(1)]).areas(f.area());
            let [_, popup, _] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(58), Constraint::Fill(1)]).areas(mid);
            f.render_widget(Clear, popup);
            f.render_widget(ask, popup);
        }
        match &self.notice {
            Some((said, until)) if now() < *until => f.render_widget(Paragraph::new(said.as_str()).style(Style::new().fg(th.good)), foot),
            _ => {
                let keys = format!(" 1-{}/tab page{} · {} · q quit", m.pages().len(), if many { " · m monitor" } else { "" }, m.hints(self.page));
                f.render_widget(Paragraph::new(keys).style(th.dim()), foot);
            }
        }
    }
}

fn text_of(buf: &ratatui::buffer::Buffer) -> String {
    let lines =
        (0..buf.area.height).map(|y| (0..buf.area.width).filter_map(|x| buf.cell((x, y)).map(|c| c.symbol())).collect::<String>().trim_end().to_string());
    lines.collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell() -> Shell {
        struct Blank;
        impl Monitor for Blank {
            fn id(&self) -> &'static str {
                "blank"
            }
            fn title(&self) -> &'static str {
                "Blank"
            }
            fn subject(&self) -> String {
                String::new()
            }
            fn pages(&self) -> &'static [&'static str] {
                &["One"]
            }
            fn start(&mut self, _: Wake) {}
            fn refresh(&mut self, _: u64) {}
            fn on_key(&mut self, _: usize, _: KeyEvent) {}
            fn header_height(&self, _: u16, _: &Theme) -> u16 {
                1
            }
            fn draw_header(&self, _: &mut Frame, _: ratatui::layout::Rect, _: &Theme) {}
            fn draw_page(&self, _: &mut Frame, _: ratatui::layout::Rect, _: usize, _: &Theme) {}
            fn hints(&self, _: usize) -> &'static str {
                ""
            }
        }
        Shell::new(vec![Box::new(Blank)], Theme::from_config(&crate::config::ThemeConfig::default())).expect("a shell")
    }

    #[test]
    fn esc_asks_and_only_a_yes_quits() {
        let mut sh = shell();
        let press = |sh: &mut Shell, c| sh.on_key(KeyEvent::from(c));

        press(&mut sh, KeyCode::Esc);
        let asked = sh.asking;
        press(&mut sh, KeyCode::Esc);
        let after_second_esc = (sh.asking, sh.quit);
        press(&mut sh, KeyCode::Esc);
        press(&mut sh, KeyCode::Char('y'));

        assert!(asked);
        assert_eq!(after_second_esc, (false, false));
        assert!(sh.quit);
    }

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        let inputs: [&[u8]; 4] = [b"", b"f", b"fo", b"foobar"];

        let out = inputs.map(base64);

        assert_eq!(out, ["", "Zg==", "Zm8=", "Zm9vYmFy"]);
    }

    #[test]
    fn a_copy_keeps_its_newlines_and_drops_every_other_control_character() {
        let text = "fund \\\n  --coin a\x1b]0;x\x07b";

        let seq = osc52(text).expect("short enough");

        assert_eq!(seq, format!("\x1b]52;c;{}\x07", base64("fund \\\n  --coin a]0;xb".as_bytes())));
        assert_eq!(osc52(&"a".repeat(MAX_COPY + 1)), None);
    }
}
