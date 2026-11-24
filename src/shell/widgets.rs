//! Drawing pieces any monitor's pages can use, so they look like one tool.

use super::theme::Theme;
use ratatui::{
    layout::Constraint,
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Cell, Row, Table},
};

/// A status light: a green dot, a red cross, or hollow when the answer is not known. Three shapes, so it reads without colour.
pub fn light(ok: Option<bool>, th: &Theme) -> Span<'static> {
    match ok {
        Some(true) => Span::styled("●", Style::new().fg(th.good)),
        Some(false) => Span::styled("✕", Style::new().fg(th.bad)),
        None => Span::styled("○", th.dim()),
    }
}

pub fn panel(title: String, th: &Theme) -> Block<'static> {
    Block::bordered().border_style(th.dim()).title(title)
}

pub fn table<'a>(title: String, head: &[&'static str], rows: Vec<Row<'a>>, widths: &[Constraint], th: &Theme) -> Table<'a> {
    Table::new(rows, widths.to_vec())
        .header(Row::new(head.iter().map(|h| Cell::from(*h)).collect::<Vec<_>>()).style(th.dim().add_modifier(Modifier::BOLD)))
        .column_spacing(2)
        .block(panel(title, th))
}

/// The rows a table shows after scrolling: never past the last one.
pub fn window<T>(rows: Vec<T>, scroll: usize) -> Vec<T> {
    let skip = scroll.min(rows.len().saturating_sub(1));
    rows.into_iter().skip(skip).collect()
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, widgets::Paragraph};

    /// Every page draws text from the board and the log through ratatui. This holds ratatui to what SECURITY.md
    /// claims of it: if a release stops dropping control characters, this fails and not an operator's terminal.
    #[test]
    fn control_characters_in_drawn_text_never_reach_the_terminal() {
        let hostile = "name\x1b[2J\x1b]0;owned\x07 \u{9b}31m bell\x07 back\x08 del\x7f";
        let mut term = Terminal::new(TestBackend::new(80, 3)).expect("a terminal");

        term.draw(|f| f.render_widget(Paragraph::new(hostile), f.area())).expect("a draw");

        let drawn: String = term.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(drawn.contains("name") && drawn.contains("bell"));
        assert!(!drawn.chars().any(char::is_control), "{drawn:?}");
    }
}
