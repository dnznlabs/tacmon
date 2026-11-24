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
    widgets::{Block, Paragraph, Tabs},
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
        Ok(Self { monitors, theme, at: 0, page: 0, quit: false, tx, rx, pending })
    }

    /// `page` or `monitor/page`, by name, as `--snapshot` takes it.
    pub fn show(&mut self, target: &str) -> Result<()> {
        let (monitor, page) = target.split_once('/').map_or((None, target), |(m, p)| (Some(m), p));
        if let Some(id) = monitor {
            self.at = self.monitors.iter().position(|m| m.id().eq_ignore_ascii_case(id)).ok_or_else(|| anyhow!("no monitor called \"{id}\""))?;
        }
        let pages = self.monitors[self.at].pages();
        self.page = pages
            .iter()
            .position(|p| p.eq_ignore_ascii_case(page))
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

    fn on_msg(&mut self, msg: Msg) {
        if let Msg::Key(k) = msg {
            self.on_key(k);
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        let pages = self.monitors[self.at].pages().len();
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
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
        let keys = format!(" 1-{}/tab page{} · {} · q quit", m.pages().len(), if many { " · m monitor" } else { "" }, m.hints(self.page));
        f.render_widget(Paragraph::new(keys).style(th.dim()), foot);
    }
}

fn text_of(buf: &ratatui::buffer::Buffer) -> String {
    let lines =
        (0..buf.area.height).map(|y| (0..buf.area.width).filter_map(|x| buf.cell((x, y)).map(|c| c.symbol())).collect::<String>().trim_end().to_string());
    lines.collect::<Vec<_>>().join("\n")
}
