//! Drawing pieces any monitor's pages can use, so they look like one tool.

use super::theme::Theme;
use ratatui::{
    layout::Constraint,
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Cell, Row, Table},
};

/// A status light: green, red, or hollow when the answer is not known.
pub fn light(ok: Option<bool>, th: &Theme) -> Span<'static> {
    match ok {
        Some(true) => Span::styled("●", Style::new().fg(th.good)),
        Some(false) => Span::styled("●", Style::new().fg(th.bad)),
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
