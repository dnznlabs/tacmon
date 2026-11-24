//! Drawing: the header, and one function per page.

use super::{
    model::*,
    state::{Page, State},
};
use crate::{
    fmt::{dur, pct, short},
    shell::{
        theme::Theme,
        widgets::{light, panel, table, window},
    },
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row},
};

fn level_color(l: Level, th: &Theme) -> Color {
    match l {
        Level::Info => th.dim,
        Level::Warn => th.warn,
        Level::Alert => th.bad,
    }
}

fn right(s: String) -> Cell<'static> {
    Cell::from(Line::from(s).right_aligned())
}

/// Fills lines with groups, starting a new line when the next group would not fit. A group is never split:
/// a figure cut off at the edge of the terminal reads as a smaller figure.
fn flow(groups: Vec<Vec<Span<'static>>>, width: u16) -> Vec<Line<'static>> {
    const GAP: &str = "   ";
    const DIVIDER: &str = "│  ";
    let mut lines: Vec<Line> = Vec::new();
    for group in groups.into_iter().filter(|g| !g.is_empty()) {
        let wide = Line::from(group.clone()).width();
        match lines.last_mut() {
            Some(line) if line.width() + GAP.len() + wide <= usize::from(width) => {
                line.spans.push(Span::raw(GAP));
                line.spans.extend(group);
            }
            _ => {
                // a divider separates two groups on one line; at the start of a line it separates nothing
                let mut group = group;
                if let Some(first) = group.first_mut()
                    && let Some(rest) = first.content.strip_prefix(DIVIDER)
                {
                    first.content = rest.to_string().into();
                }
                lines.push(Line::from(group));
            }
        }
    }
    lines
}

/// The header at this width: money, then status, then where the provider is. Each of the three starts its own line
/// and takes more lines on a narrow terminal.
pub fn header_lines(st: &State, th: &Theme, width: u16) -> Vec<Line<'static>> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let label = |s: &'static str| Span::styled(s, th.dim());
    let dash = || Span::styled("—", th.dim());
    let free = st.free();
    let money = vec![
        vec![
            label("free "),
            // a figure nothing has given yet is a dash, never a zero
            match free {
                Some(f) => Span::styled(tac(f), bold.fg(if f == 0 || st.free_from_log().is_some() { th.warn } else { th.good })),
                None => dash(),
            },
            // the provider's own answer is not behind this number: say whose word it is
            if free.is_some() && st.free_from_log().is_some() { Span::styled(" (from the log)", Style::new().fg(th.warn)) } else { Span::raw("") },
        ],
        vec![label("promised "), if st.cap_seen { Span::styled(tac(st.cap.promised), bold) } else { dash() }],
        vec![
            label("held "),
            if st.cap_seen { Span::styled(tac(st.cap.held), bold) } else { dash() },
            // promised and held are always the log's last word, so they always carry its age
            Span::styled(
                match (st.cap_seen, st.cap_age()) {
                    (false, _) => "  (no capacity line in the log yet)".to_string(),
                    (true, Some(age)) => format!("  ({} open, {} holds · log, {} ago)", st.cap.open, st.cap.holds, dur(age)),
                    (true, None) => format!("  ({} open, {} holds · log)", st.cap.open, st.cap.holds),
                },
                th.dim(),
            ),
        ],
        // the same rule for what comes from the swap records
        vec![label("│  on Bitcoin: received "), if st.swaps_known() { Span::styled(tac(st.received()), bold) } else { dash() }],
        vec![label("incoming "), if st.swaps_known() { Span::styled(tac(st.incoming()), bold) } else { dash() }],
        vec![label("│  fees earned "), if st.swaps_known() { Span::styled(tac(st.earned()), bold.fg(th.accent)) } else { dash() }],
    ];
    let status = vec![
        vec![light(st.healthy, th), label(" health")],
        // unknown, not red, while the board itself is not answering
        vec![light(st.on_board(), th), label(" on board")],
        if st.ad_contested() { vec![Span::styled("an ad that is not yours uses your key", Style::new().fg(th.bad))] } else { vec![] },
        vec![light(Some(st.swaps_ok), th), label(" records")],
        vec![match &st.terms {
            Some(t) => Span::raw(format!("fee {} + {} · takes {} to {} TAC", pct(t.fee_bps), tac(t.flat), tac(t.min), tac(t.max))),
            None => Span::styled("the provider does not answer /info", Style::new().fg(th.bad)),
        }],
        if st.id.dry_run { vec![Span::styled("DRY RUN", Style::new().fg(th.warn))] } else { vec![] },
    ];
    let places = vec![
        vec![label("pool "), Span::raw(short(&st.id.pool_account, 8, 6))],
        vec![label("bitcoin "), Span::raw(short(&st.id.bitcoin_address, 10, 6))],
        vec![label("claim "), Span::raw(short(&st.id.claim_pub, 8, 6))],
        if st.shows_eth {
            vec![
                label("ETH "),
                match st.eth {
                    Some(e) => Span::styled(format!("{e:.4}"), bold.fg(if e < 0.002 { th.bad } else { Color::Reset })),
                    None => Span::styled("?", th.dim()),
                },
            ]
        } else {
            vec![]
        },
        if st.healthy == Some(false) && !st.health_error.is_empty() { vec![Span::styled(st.health_error.clone(), Style::new().fg(th.bad))] } else { vec![] },
    ];
    [money, status, places].into_iter().flat_map(|row| flow(row, width)).collect()
}

pub fn header(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    f.render_widget(Paragraph::new(header_lines(st, th, area.width)), area);
}

pub fn page(f: &mut Frame, area: Rect, page: Page, st: &State, th: &Theme) {
    match page {
        Page::Overview => overview(f, area, st, th),
        Page::Activity => activity(f, area, st, th, true),
        Page::Demand => demand(f, area, st, th),
        Page::Market => market(f, area, st, th),
        Page::Advisor => advisor(f, area, st, th),
    }
}

/// "●●●○○○ locked": how far along the six steps a swap is.
fn pipeline(s: &Swap, th: &Theme) -> Line<'static> {
    let (dots, color) = match PHASES.iter().position(|p| *p == s.phase) {
        Some(i) => (format!("{}{}", "●".repeat(i + 1), "○".repeat(PHASES.len() - i - 1)), if s.done() { th.good } else { th.accent }),
        // refunding, refunded, rejected, aborted: off the happy path
        None => ("✕".repeat(PHASES.len()), th.warn),
    };
    Line::from(vec![Span::styled(dots, Style::new().fg(color)), Span::raw(format!(" {}", s.phase))])
}

fn overview(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let [top, bottom] = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);
    let rows: Vec<Row> = st
        .swaps
        .iter()
        .map(|s| {
            let left = s.deadline.saturating_sub(st.now);
            let deadline = if s.closed() || s.deadline == 0 {
                Cell::from("—")
            } else {
                Cell::from(dur(left)).style(Style::new().fg(if left < st.deadline_warn_secs { th.bad } else { Color::Reset }))
            };
            Row::new(vec![
                Cell::from(short(&s.id, 8, 4)),
                Cell::from(pipeline(s, th)),
                right(tac(s.pays)),
                right(tac(s.gets)),
                right(tac(s.fee())).style(Style::new().fg(th.accent)),
                // a record that does not say when it began has no age, not one counted from 1970
                if s.created == 0 { Cell::from("—") } else { Cell::from(dur(st.now.saturating_sub(s.created))) },
                deadline,
                Cell::from(short(&s.lock_txid, 8, 4)).style(th.dim()),
            ])
        })
        .collect();
    let open = st.swaps.iter().filter(|s| !s.closed()).count();
    let title = format!(" Swaps · {open} open · {} done ", st.swaps.iter().filter(|s| s.done()).count());
    let widths = [
        Constraint::Length(13),
        Constraint::Length(16),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(9),
        Constraint::Length(7),
        Constraint::Length(9),
        Constraint::Fill(1),
    ];
    f.render_widget(
        table(
            title,
            &["SWAP", "PHASE", "PAYS (BTC)", "GETS (POOL)", "FEE", "AGE", "DEADLINE", "LOCK TX"],
            window(rows, st.scroll[Page::Overview as usize]),
            &widths,
            th,
        ),
        top,
    );
    activity(f, bottom, st, th, false);
}

/// The log as a feed. `full` is the page (scrolls, filters); otherwise the overview's pane of the latest warnings.
fn activity(f: &mut Frame, area: Rect, st: &State, th: &Theme, full: bool) {
    let only_warn = if full { st.only_warn } else { true };
    let lines: Vec<_> = st.log.iter().filter(|l| !only_warn || l.entry.level != Level::Info).collect();
    let height = area.height.saturating_sub(2) as usize;
    let back = if full { st.scroll[Page::Activity as usize].min(lines.len().saturating_sub(height)) } else { 0 };
    let end = lines.len() - back;
    let text: Vec<Line> = lines[end.saturating_sub(height)..end]
        .iter()
        .map(|l| {
            let (e, c) = (&l.entry, level_color(l.entry.level, th));
            let mark = match e.level {
                Level::Info => "· ",
                Level::Warn => "▲ ",
                Level::Alert => "✖ ",
            };
            let mut spans = vec![
                Span::styled(format!("{} ", e.time), th.dim()),
                Span::styled(mark, Style::new().fg(c)),
                Span::styled(e.msg.clone(), if e.level == Level::Info { Style::new() } else { Style::new().fg(c) }),
            ];
            if l.count > 1 {
                spans.push(Span::styled(format!(" ×{}", l.count), Style::new().fg(th.accent)));
            }
            if !e.detail.is_empty() {
                spans.push(Span::styled(format!("  {}", e.detail), th.dim()));
            }
            Line::from(spans)
        })
        .collect();
    let filter = if only_warn { " · warnings only" } else { "" };
    let title = match (full, back) {
        (false, _) => " Warnings and alerts ".to_string(),
        (true, 0) => format!(" Activity{filter} · following "),
        (true, b) => format!(" Activity{filter} · {b} lines back "),
    };
    f.render_widget(Paragraph::new(text).block(panel(title, th)), area);
}

fn demand(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let rows: Vec<Row> = st
        .wants
        .iter()
        .map(|w| {
            let verdict = match st.blocker(w) {
                None => Cell::from("?").style(th.dim()),
                Some(None) => Cell::from("✔ you can take it").style(Style::new().fg(th.good)),
                Some(Some(why)) => Cell::from(format!("✕ {why}")).style(Style::new().fg(th.warn)),
            };
            Row::new(vec![
                right(tac(w.amount)),
                right(w.max_fee_bps.map(pct).unwrap_or_else(|| "any".into())),
                right(if w.until == 0 { "—".into() } else { dur(w.until.saturating_sub(st.now)) }),
                verdict,
            ])
        })
        .collect();
    let total = total(st.wants.iter().map(|w| w.amount));
    let takeable = st.wants.iter().filter(|w| st.blocker(w) == Some(None)).count();
    let title = if st.board_ok {
        format!(" Bridgers waiting · {} · {} TAC · {takeable} you can take ", st.wants.len(), tac(total))
    } else {
        " Bridgers waiting · the board does not answer ".to_string()
    };
    let widths = [Constraint::Length(14), Constraint::Length(9), Constraint::Length(10), Constraint::Fill(1)];
    f.render_widget(table(title, &["           TAC", "  MAX FEE", "EXPIRES IN", "FOR YOU"], window(rows, st.scroll[Page::Demand as usize]), &widths, th), area);
}

fn market(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let rows: Vec<Row> = st
        .ads
        .iter()
        .map(|a| {
            // the ad this provider's own terms pick out, not every ad that names its key
            let me = st.my_ad().is_some_and(|mine| std::ptr::eq(mine, a));
            let t = &a.terms;
            let avail = match t.available {
                Some(0) => right("full".into()).style(th.dim()),
                Some(v) => right(tac(v)),
                None => right("?".into()),
            };
            // the provider's own default name: "Provider" and the first six hex digits after the key's prefix byte
            let name = if a.name.is_empty() { format!("Provider {}", a.claim_pub.get(2..8).unwrap_or("")) } else { a.name.clone() };
            let row = Row::new(vec![
                Cell::from(format!("{}{name}", if me { "▶ " } else { "  " })),
                right(pct(t.fee_bps)),
                right(tac(t.flat)),
                right(pct(effective_bps(100 * UNIT, t.fee_bps, t.flat))),
                right(format!("{} – {}", tac(t.min), tac(t.max))),
                avail,
                Cell::from(format!("{}{}", if a.any_tac { "any TAC" } else { "home only" }, if a.reverse { " · reverse" } else { "" })),
            ]);
            if me { row.style(Style::new().fg(th.accent).add_modifier(Modifier::BOLD)) } else { row }
        })
        .collect();
    let title = if st.ads_ok {
        format!(" Providers on the board · {} · cheapest first at 100 TAC ", st.ads.len())
    } else {
        format!(" Providers on the board · {} · THE BOARD DOES NOT ANSWER: last known ", st.ads.len())
    };
    let widths = [
        Constraint::Length(24),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(26),
        Constraint::Length(12),
        Constraint::Fill(1),
    ];
    f.render_widget(
        table(
            title,
            &["  NAME", "    FEE", "    FLAT", " @100 TAC", "               TAKES (TAC)", "   AVAILABLE", "ACCEPTS"],
            window(rows, st.scroll[Page::Market as usize]),
            &widths,
            th,
        ),
        area,
    );
}

fn advisor(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let (Some(adv), Some(t)) = (st.advice(), st.terms.as_ref()) else {
        f.render_widget(Paragraph::new("Waiting for the provider's terms (/info).").block(panel(" Advisor ".into(), th)), area);
        return;
    };
    let [left, right_pane] = Layout::horizontal([Constraint::Length(58), Constraint::Min(30)]).areas(area);
    let rows: Vec<Row> = adv
        .ladder
        .iter()
        .map(|r| {
            let best = Some(r.bps) == adv.best_bps;
            let mark = match (r.current, best) {
                (true, true) => "▶ yours · best",
                (true, false) => "▶ yours",
                (false, true) => "★ best",
                _ => "",
            };
            let row = Row::new(vec![right(pct(r.bps)), right(r.count.to_string()), right(tac(r.volume)), right(tac(r.revenue)), Cell::from(mark)]);
            match (r.current, best) {
                (true, _) => row.style(Style::new().fg(th.accent).add_modifier(Modifier::BOLD)),
                (_, true) => row.style(Style::new().fg(th.good)),
                _ => row,
            }
        })
        .collect();
    let widths = [Constraint::Length(7), Constraint::Length(7), Constraint::Length(12), Constraint::Length(10), Constraint::Fill(1)];
    f.render_widget(
        table(
            if st.board_ok { " Fee against what is waiting now ".into() } else { " Fee against what was waiting · THE BOARD DOES NOT ANSWER ".into() },
            &["    FEE", "BRIDGES", "      VOLUME", "  YOU EARN", ""],
            rows,
            &widths,
            th,
        ),
        left,
    );

    let free = st.free();
    let head = |s: &'static str| Line::from(Span::styled(s, Style::new().fg(th.accent).add_modifier(Modifier::BOLD)));
    let faint = |s: &'static str| Line::from(Span::styled(s, th.dim()));
    let mut out = vec![head("Fee")];
    out.push(match adv.best_bps {
        Some(b) if b == t.fee_bps => Line::from(format!("  {} earns the most on what is waiting. Keep it.", pct(b))),
        Some(b) => Line::from(format!("  {} would earn the most on what is waiting (you are at {}).", pct(b), pct(t.fee_bps))),
        None => Line::from("  Nothing waiting fits your size range, so no fee earns anything now."),
    });
    out.extend([Line::from(""), head("Size")]);
    if adv.over_count > 0 {
        out.push(Line::from(format!("  {} waiting ({} TAC) are over your max of {}.", adv.over_count, tac(adv.over_volume), tac(t.max))));
        out.push(Line::from(format!("  A max of {} would put all of them in range.", tac(adv.largest))));
    } else {
        out.push(Line::from(format!("  Nothing waiting is over your max of {}.", tac(t.max))));
    }
    if adv.under_count > 0 {
        out.push(Line::from(format!("  {} waiting are under your min of {}.", adv.under_count, tac(t.min))));
    }
    out.extend([Line::from(""), head("Funding")]);
    match free {
        Some(free) => {
            out.push(Line::from(format!("  In range now: {} TAC waiting. You have {} free.", tac(adv.in_range_volume), tac(free))));
            out.push(if adv.in_range_volume > free {
                Line::from(Span::styled(format!("  {} more TAC would cover all of it at once.", tac(adv.in_range_volume - free)), Style::new().fg(th.warn)))
            } else {
                Line::from(Span::styled("  You can cover all of it.", Style::new().fg(th.good)))
            });
        }
        None => {
            out.push(Line::from(format!("  In range now: {} TAC waiting.", tac(adv.in_range_volume))));
            out.push(Line::from(Span::styled("  How much TAC is free is not known yet, so cover cannot be judged.", Style::new().fg(th.warn))));
        }
    }
    out.extend([
        Line::from(""),
        faint("  A waiting bridge is one no provider online could take when it was"),
        faint("  posted. This is a snapshot of that demand, not a forecast: bridges"),
        faint("  that found a provider at once never appear here."),
    ]);
    f.render_widget(Paragraph::new(out).block(panel(" Suggestions ".into(), th)), right_pane);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::ThemeConfig, monitors::fast_bridge::config::FastBridgeConfig};

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect()
    }

    #[test]
    fn a_narrow_header_takes_more_lines_and_never_cuts_a_figure() {
        let (st, th) = (State::new(&FastBridgeConfig::default(), true), Theme::from_config(&ThemeConfig::default()));

        let (wide, narrow) = (header_lines(&st, &th, 200), header_lines(&st, &th, 60));

        assert_eq!(wide.len(), 3);
        assert!(narrow.len() > 3);
        assert!(narrow.iter().all(|l| l.width() <= 60), "{:?}", text(&narrow));
        let figures = |lines: &[Line]| text(lines).concat().replace([' ', '│'], "");
        assert_eq!(figures(&wide), figures(&narrow));
        assert!(text(&narrow).iter().all(|l| !l.starts_with('│')));
    }

    #[test]
    fn a_group_wider_than_the_terminal_gets_a_line_to_itself() {
        let groups = vec![vec![Span::raw("ab")], vec![Span::raw("a group far too wide")], vec![Span::raw("cd")]];

        let lines = flow(groups, 8);

        assert_eq!(text(&lines), ["ab", "a group far too wide", "cd"]);
    }
}
