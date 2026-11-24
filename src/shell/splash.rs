//! The start screen: the Tacit mark drawn on in braille dots, then the name. It plays while the monitors
//! take their first readings, and any key ends it.

use super::theme::Theme;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::{f32::consts::TAU, time::Duration};

pub const LENGTH: Duration = Duration::from_millis(1900);
/// How long the finished screen stays up, so the line under the name can be read.
pub const HOLD: Duration = Duration::from_millis(1500);
pub const FRAME: Duration = Duration::from_millis(33);

// the brand's own colours, whatever the theme
const ORANGE: Color = Color::Rgb(0xf7, 0x93, 0x1a);
const CREAM: Color = Color::Rgb(0xf4, 0xef, 0xe6);
const WORD: &str = "T A C M O N";
const LINE: &str = "activity monitor for tacit services";
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

type P = (f32, f32);

/// How far along `from..to` the animation's clock `t` (0 to 1) is.
fn phase(t: f32, from: f32, to: f32) -> f32 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

fn seg_dist(p: P, a: P, b: P) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let u = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy).max(f32::EPSILON)).clamp(0.0, 1.0);
    ((p.0 - a.0 - u * dx).powi(2) + (p.1 - a.1 - u * dy).powi(2)).sqrt()
}

/// Distance to the first `shown` (0 to 1) of a path's length, or `None` while none of it is drawn.
fn path_dist(p: P, pts: &[P], shown: f32) -> Option<f32> {
    let len = |w: &[P]| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
    let mut left = pts.windows(2).map(len).sum::<f32>() * shown;
    let mut best = None::<f32>;
    for w in pts.windows(2) {
        if left <= 0.0 {
            break;
        }
        let part = (left / len(w)).min(1.0);
        let end = (w[0].0 + (w[1].0 - w[0].0) * part, w[0].1 + (w[1].1 - w[0].1) * part);
        let d = seg_dist(p, w[0], end);
        best = Some(best.map_or(d, |b| b.min(d)));
        left -= len(w);
    }
    best
}

/// The mark's shapes, in the brand SVG's 32 x 32 box: the "t" is a slanted stem that hooks at the foot, and a crossbar.
fn stem() -> Vec<P> {
    let mut pts = vec![(17.5, 7.0), (13.0, 22.5)];
    // the hook: a quadratic from the stem's foot, control (13, 25), to (18, 24)
    for i in 1..=8 {
        let (u, v) = (i as f32 / 8.0, 1.0 - i as f32 / 8.0);
        pts.push((v * v * 13.0 + 2.0 * v * u * 13.0 + u * u * 18.0, v * v * 22.5 + 2.0 * v * u * 25.0 + u * u * 24.0));
    }
    pts
}

/// The mark at `rows` high and twice as wide (a braille cell is 2 x 4 dots, so dots come out about square).
fn mark(rows: u16, t: f32) -> Vec<Line<'static>> {
    let step = 32.0 / f32::from(rows * 4);
    // a stroke is never thinner than a dot, or a small mark loses its ring
    let half = |w: f32| (w / 2.0).max(step * 0.55);
    let (ring_p, stem_p, bar_p) = (phase(t, 0.0, 0.45), phase(t, 0.3, 0.62), phase(t, 0.55, 0.72));
    let (stem, bar) = (stem(), [(10.0, 11.0), (21.0, 11.0)]);
    (0..rows)
        .map(|row| {
            let cells = (0..rows * 2).map(|col| {
                let (mut bits, mut ring, mut letter) = (0u32, 0, 0);
                for dy in 0..4u16 {
                    for dx in 0..2u16 {
                        let p = ((f32::from(col * 2 + dx) + 0.5) * step, (f32::from(row * 4 + dy) + 0.5) * step);
                        let (x, y) = (p.0 - 16.0, p.1 - 16.0);
                        // the ring is drawn clockwise from the top
                        let turn = x.atan2(-y).rem_euclid(TAU) / TAU;
                        let on_ring = ((x * x + y * y).sqrt() - 13.0).abs() <= half(2.5) && turn <= ring_p && ring_p > 0.0;
                        let on_t = path_dist(p, &stem, stem_p).is_some_and(|d| d <= half(2.8)) || path_dist(p, &bar, bar_p).is_some_and(|d| d <= half(2.4));
                        if on_ring || on_t {
                            // braille dot numbering: 1 2 3 7 down the left column, 4 5 6 8 down the right
                            bits |= 1 << [[0, 3], [1, 4], [2, 5], [6, 7]][dy as usize][dx as usize];
                            if on_t { letter += 1 } else { ring += 1 }
                        }
                    }
                }
                // a cell has one colour: the letter's wherever it has the most dots
                Span::styled(
                    char::from_u32(0x2800 + bits).unwrap_or(' ').to_string(),
                    Style::new().fg(if letter >= ring && letter > 0 { ORANGE } else { CREAM }),
                )
            });
            Line::from(cells.collect::<Vec<_>>())
        })
        .collect()
}

/// `t` runs from 0 to 1 over the animation.
pub fn draw(f: &mut Frame, t: f32, th: &Theme) {
    let area = f.area();
    // the mark takes what the terminal has, less the three lines of text and some air
    let rows = area.height.saturating_sub(7).min(area.width / 2).min(22);
    if rows < 6 {
        return;
    }
    let [_, mark_area, _, word, line, version, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(rows),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    let centred = Rect { x: mark_area.x + mark_area.width.saturating_sub(rows * 2) / 2, width: (rows * 2).min(mark_area.width), ..mark_area };
    f.render_widget(Paragraph::new(mark(rows, t)), centred);

    // the name is typed out, then the line under it appears
    let typed = (phase(t, 0.68, 0.86) * WORD.len() as f32) as usize;
    f.render_widget(
        Paragraph::new(Span::styled(&WORD[..typed], Style::new().fg(ORANGE).add_modifier(Modifier::BOLD))).centered(),
        Rect { x: word.x + word.width.saturating_sub(WORD.len() as u16) / 2, width: WORD.len() as u16, ..word }.intersection(word),
    );
    if t >= 0.88 {
        f.render_widget(Paragraph::new(Span::styled(LINE, th.dim())).centered(), line);
        f.render_widget(Paragraph::new(Span::styled(VERSION, th.dim())).centered(), version);
    }
}
