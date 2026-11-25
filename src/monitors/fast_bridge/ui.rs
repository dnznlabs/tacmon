//! Drawing: the header, and one function per page.

use super::{advisor, upkeep};
use super::{
    advisor::{Flag, HIGH_FEE_BPS, Standing, all_in},
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
    widgets::{Cell, Paragraph, Row, Wrap},
};

fn level_color(l: Level, th: &Theme) -> Color {
    match l {
        Level::Info => th.dim,
        Level::Warn => th.warn,
        Level::Alert => th.bad,
    }
}

/// What the board lists for pairs this provider does not serve: counted in a title, never drawn as this market.
fn others(n: usize) -> String {
    if n == 0 { String::new() } else { format!(" · {n} for other assets not shown") }
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
            Span::styled("forward  ", bold),
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
        vec![label("│  on Bitcoin: received "), if st.swaps_known() { Span::styled(tac(st.received(Direction::Forward)), bold) } else { dash() }],
        vec![label("incoming "), if st.swaps_known() { Span::styled(tac(st.incoming(Direction::Forward)), bold) } else { dash() }],
        vec![label("│  fees earned "), if st.swaps_known() { Span::styled(tac(st.earned(Direction::Forward)), bold.fg(th.accent)) } else { dash() }],
        // the records could be read once and cannot be now: the three figures before this are the last read
        if st.swaps_known() && !st.swaps_ok { vec![Span::styled("(records not read: last known)", Style::new().fg(th.bad))] } else { vec![] },
    ];
    let on = st.reverse_terms();
    let mut reverse = vec![vec![
        Span::styled("reverse  ", bold),
        // off and not answering are each said as such: neither is a side with nothing to sell
        match &st.reverse {
            Some(ReverseSide::On { .. }) => Span::styled("on", Style::new().fg(th.good)),
            Some(ReverseSide::Off) => Span::styled("off", th.dim()),
            None if st.reverse_ok => dash(),
            None => Span::styled("?", th.dim()),
        },
        if !st.reverse_ok && st.healthy.is_some() {
            Span::styled(
                if st.reverse.is_some() { " (/reverse/info not answering: last known)" } else { " (/reverse/info does not answer)" },
                Style::new().fg(th.bad),
            )
        } else {
            Span::raw("")
        },
    ]];
    if let (Some(t), Some(ReverseSide::On { open, max_open, .. })) = (on, &st.reverse) {
        reverse.push(vec![
            label("can take "),
            match t.available {
                Some(a) => Span::styled(tac(a), bold.fg(if a < t.min { th.warn } else { th.good })),
                // the provider could not read the notes at its address
                None => dash(),
            },
            label(" pool TAC"),
        ]);
        reverse.push(vec![label("open "), Span::raw(format!("{}/{max_open}", open.map_or_else(|| "?".into(), |o| o.to_string())))]);
    }
    if on.is_some() || st.has_reverse_swaps() {
        reverse
            .push(vec![label("│  in the pool: received "), if st.swaps_known() { Span::styled(tac(st.received(Direction::Reverse)), bold) } else { dash() }]);
        reverse.push(vec![label("incoming "), if st.swaps_known() { Span::styled(tac(st.incoming(Direction::Reverse)), bold) } else { dash() }]);
        reverse
            .push(vec![label("│  fees earned "), if st.swaps_known() { Span::styled(tac(st.earned(Direction::Reverse)), bold.fg(th.accent)) } else { dash() }]);
    }
    let status = vec![
        vec![light(st.healthy, th), label(" health")],
        // unknown, not red, while the board itself is not answering
        vec![light(st.on_board(), th), label(" on board")],
        if st.ad_contested() { vec![Span::styled("an ad that is not yours uses your key", Style::new().fg(th.bad))] } else { vec![] },
        vec![light(Some(st.swaps_ok), th), label(" records")],
        // the terms are on the Advisor and Maintenance pages; here only whether they can be read
        if st.terms.is_none() { vec![Span::styled("the provider does not answer /info", Style::new().fg(th.bad))] } else { vec![] },
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
    [money, reverse, status, places].into_iter().flat_map(|row| flow(row, width)).collect()
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
        Page::Maintenance => maintenance(f, area, st, th),
    }
}

/// "●●●○○○ locked": how far along the six steps a swap is.
fn pipeline(s: &Swap, th: &Theme) -> Line<'static> {
    let phases = s.direction.phases();
    let (dots, color) = match phases.iter().position(|p| *p == s.phase) {
        Some(i) => (format!("{}{}", "●".repeat(i + 1), "○".repeat(phases.len() - i - 1)), if i + 1 == phases.len() { th.good } else { th.accent }),
        // refunding, refunded, rejected, aborted, refund-wait, counter-took: off the happy path
        None => ("✕".repeat(phases.len()), th.warn),
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
                Cell::from(s.direction.label()).style(if s.direction == Direction::Reverse { Style::new().fg(th.accent) } else { th.dim() }),
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
    let done = |d| st.swaps.iter().filter(|s| s.direction == d && s.paid()).count();
    let forward = format!("forward {} open · {} done", st.open(Direction::Forward), done(Direction::Forward));
    let title = if st.has_reverse_swaps() || st.reverse_terms().is_some() {
        format!(" Swaps · {forward} · reverse {} open · {} paid ", st.open(Direction::Reverse), done(Direction::Reverse))
    } else {
        format!(" Swaps · {forward} ")
    };
    let widths = [
        Constraint::Length(13),
        Constraint::Length(8),
        Constraint::Length(17),
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
            &["SWAP", "WAY", "PHASE", "        PAYS", "        GETS", "FEE", "AGE", "DEADLINE", "LOCK TX"],
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
        format!(" Bridgers waiting, Bitcoin to pool · {} · {} TAC · {takeable} you can take{} ", st.wants.len(), tac(total), others(st.other_wants))
    } else {
        " Bridgers waiting · the board does not answer ".to_string()
    };
    let widths = [Constraint::Length(14), Constraint::Length(9), Constraint::Length(10), Constraint::Fill(1)];
    f.render_widget(table(title, &["           TAC", "  MAX FEE", "EXPIRES IN", "FOR YOU"], window(rows, st.scroll[Page::Demand as usize]), &widths, th), area);
}

fn market(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let dir = st.market_dir;
    let listed = st.market(dir);
    let rows: Vec<Row> = listed
        .iter()
        .map(|&(a, t)| {
            // the ad this provider's own terms pick out, not every ad that names its key
            let me = st.my_ad().is_some_and(|mine| std::ptr::eq(mine, a));
            let avail = match t.available {
                Some(0) => right("full".into()).style(th.dim()),
                Some(v) => right(tac(v)),
                None => right("?".into()),
            };
            // the provider's own default name: "Provider" and the first six hex digits after the key's prefix byte
            // the fee a bridge of this size would pay; dimmed where the provider does not take one that size
            let at = |size: u128| {
                let cell = right(pct(effective_bps(size, t.fee_bps, t.flat)));
                if (t.min..=t.max).contains(&size) { cell } else { cell.style(th.dim()) }
            };
            let name = if a.name.is_empty() { format!("Provider {}", a.claim_pub.get(2..8).unwrap_or("")) } else { a.name.clone() };
            let row = Row::new(vec![
                Cell::from(format!("{}{name}", if me { "▶ " } else { "  " })),
                right(pct(t.fee_bps)),
                right(tac(t.flat)),
                at(100 * UNIT),
                at(1000 * UNIT),
                right(format!("{} – {}", tac(t.min), tac(t.max))),
                avail,
                Cell::from(format!("{}{}", if a.any_tac { "any TAC" } else { "home only" }, if a.reverse { " · reverse" } else { "" })),
            ]);
            if me { row.style(Style::new().fg(th.accent).add_modifier(Modifier::BOLD)) } else { row }
        })
        .collect();
    let way = match dir {
        Direction::Forward => "Bitcoin to pool",
        Direction::Reverse => "pool to Bitcoin",
    };
    let title = if st.ads_ok {
        format!(" Providers on the board · {way} · {} · cheapest first at 100 TAC{} ", listed.len(), others(st.other_ads))
    } else {
        format!(" Providers on the board · {way} · {} · THE BOARD DOES NOT ANSWER: last known ", listed.len())
    };
    let widths = [
        Constraint::Length(24),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(10),
        Constraint::Length(26),
        Constraint::Length(12),
        Constraint::Fill(1),
    ];
    f.render_widget(
        table(
            title,
            &["  NAME", "    FEE", "    FLAT", " @100 TAC", "@1000 TAC", "               TAKES (TAC)", "   AVAILABLE", "ACCEPTS"],
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
    let [left, right_pane] = Layout::horizontal([Constraint::Length(78), Constraint::Min(30)]).areas(area);
    let rows: Vec<Row> = adv
        .ladder
        .iter()
        .map(|r| {
            // the mark is for the fee that gets chosen, never for the one that earns most: the rows show what each
            // fee would look like, and a high one is named as what the app would call it
            let matches = Some(r.bps) == adv.competitive_bps;
            let high = match r.high {
                Flag::Never => None,
                Flag::Always => Some("high fee".to_string()),
                Flag::Under(size) => Some(format!("high fee under {}", tac(size))),
            };
            let mark = match (r.current, matches, &high) {
                (true, _, Some(h)) => format!("▶ yours · {h}"),
                (true, true, None) => "▶ yours · competitive".into(),
                (true, false, None) => "▶ yours".into(),
                (false, _, Some(h)) => h.clone(),
                (false, true, None) => "★ matches the cheapest".into(),
                _ => String::new(),
            };
            let row = Row::new(vec![right(pct(r.bps)), right(r.count.to_string()), right(tac(r.volume)), right(tac(r.revenue)), Cell::from(mark)]);
            // dimmed only when every bridge it would take is marked: one marked on small bridges is still a fee to weigh
            match (r.current, matches, r.high) {
                (true, _, _) => row.style(Style::new().fg(if high.is_some() { th.warn } else { th.accent }).add_modifier(Modifier::BOLD)),
                (_, _, Flag::Always) => row.style(th.dim()),
                (_, true, _) => row.style(Style::new().fg(th.good)),
                _ => row,
            }
        })
        .collect();
    let widths = [Constraint::Length(7), Constraint::Length(7), Constraint::Length(12), Constraint::Length(11), Constraint::Fill(1)];
    f.render_widget(
        table(
            if st.board_ok && st.ads_ok {
                " Forward fee against what is waiting now ".into()
            } else {
                " Forward fee against what was waiting · THE BOARD DOES NOT ANSWER ".into()
            },
            &["    FEE", "BRIDGES", "      VOLUME", " WOULD EARN", ""],
            rows,
            &widths,
            th,
        ),
        left,
    );

    let free = st.free();
    let head = |s: &'static str| Line::from(Span::styled(s, Style::new().fg(th.accent).add_modifier(Modifier::BOLD)));
    let faint = |s: &'static str| Line::from(Span::styled(s, th.dim()));
    let mut out = vec![head("Forward fee (Bitcoin to pool)")];
    let own = all_in(t.fee_bps, t.flat);
    out.push(Line::from(format!("  You charge {} all in on a 100 TAC bridge ({} + {} flat).", pct(own), pct(t.fee_bps), tac(t.flat))));
    // whose word the comparison rests on: the board signs nothing, so the name is there for the operator to judge
    let who = adv.cheapest_name.as_deref().filter(|n| !n.is_empty()).map(|n| format!(" ({})", n.chars().take(24).collect::<String>())).unwrap_or_default();
    out.push(match adv.standing {
        Standing::Unknown => {
            Line::from(Span::styled("  The board's ads have not been read yet, so there is nothing to compare with.", Style::new().fg(th.warn)))
        }
        Standing::Alone => Line::from("  No other provider that could take the same bridges is on the board."),
        Standing::Cheapest { next } => {
            Line::from(Span::styled(format!("  That is the cheapest on the board (the next{who} is {}). Keep it.", pct(next)), Style::new().fg(th.good)))
        }
        Standing::Dearer { cheapest, matching: Some(b) } => Line::from(Span::styled(
            format!("  The cheapest other{who} charges {}. A fee of {} would match it.", pct(cheapest), pct(b)),
            Style::new().fg(th.warn),
        )),
        Standing::Dearer { cheapest, matching: None } => {
            Line::from(Span::styled(format!("  The cheapest other{who} charges {}, under every fee compared here.", pct(cheapest)), Style::new().fg(th.warn)))
        }
    });
    if st.ads_seen && !st.ads_ok {
        out.push(Line::from(Span::styled("  The board's ads could not be read: this is against the last known ones.", Style::new().fg(th.bad))));
    }
    out.push(faint("  Ads are unsigned: anyone can post one. Check a rival is real on the Market page."));
    out.push(faint(
        "  A bridger is offered the cheaper provider, so being chosen comes before earning more per bridge. The table shows what each fee would earn.",
    ));
    let marked = match advisor::flag(t.min, t.max, t.fee_bps, t.flat) {
        Flag::Never => None,
        Flag::Always => Some("every bridge you take".to_string()),
        Flag::Under(size) => Some(format!("your bridges under {} TAC", tac(size))),
    };
    if let Some(which) = marked {
        out.push(Line::from(Span::styled(
            format!("  The app marks a fee over {} of a bridge as high: at yours, that is {which}.", pct(HIGH_FEE_BPS)),
            Style::new().fg(th.bad),
        )));
    }
    out.extend([Line::from(""), head("Reverse fee (pool to Bitcoin)")]);
    match (st.reverse_terms(), st.reverse_standing()) {
        (Some(rt), Some((standing, name))) => {
            out.push(Line::from(format!(
                "  You charge {} all in on a 100 TAC send ({} + {} flat).",
                pct(all_in(rt.fee_bps, rt.flat)),
                pct(rt.fee_bps),
                tac(rt.flat)
            )));
            let who = name.filter(|n| !n.is_empty()).map(|n| format!(" ({})", n.chars().take(24).collect::<String>())).unwrap_or_default();
            out.push(match standing {
                Standing::Unknown => {
                    Line::from(Span::styled("  The board's ads have not been read yet, so there is nothing to compare with.", Style::new().fg(th.warn)))
                }
                Standing::Alone => Line::from("  No other provider that could take the same sends lists reverse terms."),
                Standing::Cheapest { next } => Line::from(Span::styled(
                    format!("  That is the cheapest on the board (the next{who} is {}). Keep it.", pct(next)),
                    Style::new().fg(th.good),
                )),
                Standing::Dearer { cheapest, .. } => {
                    Line::from(Span::styled(format!("  The cheapest other{who} charges {}.", pct(cheapest)), Style::new().fg(th.warn)))
                }
            });
        }
        _ => out.push(faint(match st.reverse {
            Some(ReverseSide::Off) => "  The reverse side is off.",
            _ => "  The reverse side's terms are not known (/reverse/info).",
        })),
    }
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
    // wrapped, never cut: a figure that runs off the edge of the pane reads as a smaller one
    f.render_widget(Paragraph::new(out).wrap(Wrap { trim: false }).block(panel(" Suggestions ".into(), th)), right_pane);
}

/// `last` when the last read failed: what is drawn is the last known.
fn read_note(ok: bool, ever: bool) -> &'static str {
    match (ok, ever) {
        (true, _) => "",
        (false, true) => " · NOT ANSWERING: last known",
        (false, false) => " · not read yet",
    }
}

fn maintenance(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let up = st.upkeep();
    let [left, right_pane] = Layout::horizontal([Constraint::Min(60), Constraint::Length(52)]).areas(area);
    let head = |s: &'static str| Line::from(Span::styled(s, Style::new().fg(th.accent).add_modifier(Modifier::BOLD)));
    let label = |s: &'static str| Span::styled(s, th.dim());
    let dash = || Span::styled("—", th.dim());

    let mut out = vec![head("Forward side (Bitcoin to pool)")];
    out.push(Line::from(vec![
        label("  status "),
        match st.healthy {
            Some(true) => Span::styled("on", Style::new().fg(th.good)),
            Some(false) => Span::styled("its last tick failed", Style::new().fg(th.bad)),
            None => Span::styled("the provider does not answer /health", Style::new().fg(th.bad)),
        },
        label("   pool account "),
        if st.id.pool_account.is_empty() { dash() } else { Span::raw(st.id.pool_account.clone()) },
    ]));
    out.push(Line::from(match &st.terms {
        Some(t) => vec![
            label("  can lend "),
            st.free().map_or_else(dash, |f| Span::styled(tac(f), Style::new().add_modifier(Modifier::BOLD).fg(if f < t.min { th.warn } else { th.good }))),
            label(" pool TAC now · takes "),
            Span::raw(format!("{} to {}", tac(t.min), tac(t.max))),
            label(" · fee "),
            Span::raw(format!("{} + {}", pct(t.fee_bps), tac(t.flat))),
            label(" · open "),
            Span::raw(st.open(Direction::Forward).to_string()),
        ],
        None => vec![label("  terms "), Span::styled("the provider does not answer /info", Style::new().fg(th.bad))],
    }));
    out.push(Line::from(vec![label("  main address "), if st.id.bitcoin_address.is_empty() { dash() } else { Span::raw(st.id.bitcoin_address.clone()) }]));
    out.push(Line::from(if st.explorer {
        vec![
            label("  at the address "),
            if st.main_coins.is_some() {
                Span::raw(format!("{} TAC from {} forward takes", tac(total(up.notes.iter().map(|n| n.tac))), up.notes.len()))
            } else {
                dash()
            },
            label(" · "),
            if st.main_coins.is_some() { Span::raw(format!("{} sats in plain coins", up.coins.iter().map(|c| c.sats).sum::<u64>())) } else { dash() },
            Span::styled(read_note(st.main_coins_ok, st.main_coins.is_some()), Style::new().fg(th.bad)),
        ]
    } else {
        vec![label("  at the address "), dash(), label(" · set fast_bridge.btc_explorer to list its coins")]
    }));
    out.extend([Line::from(""), head("Reverse side (pool to Bitcoin)")]);
    out.push(Line::from(vec![
        label("  status "),
        match (&st.reverse, st.reverse_ok) {
            (Some(ReverseSide::On { .. }), _) => Span::styled("on", Style::new().fg(th.good)),
            (Some(ReverseSide::Off), _) => Span::styled("off", Style::new().fg(th.warn)),
            (None, _) => Span::styled("the provider does not answer /reverse/info", Style::new().fg(th.bad)),
        },
        if st.reverse.is_some() && !st.reverse_ok { Span::styled(" (last known)", Style::new().fg(th.bad)) } else { Span::raw("") },
        label("   address "),
        if st.reverse_address.is_empty() { dash() } else { Span::raw(st.reverse_address.clone()) },
    ]));
    if let Some(ReverseSide::On { terms, open, max_open }) = &st.reverse {
        out.push(Line::from(vec![
            label("  can take "),
            terms.available.map_or_else(dash, |a| Span::styled(tac(a), Style::new().add_modifier(Modifier::BOLD))),
            label(" pool TAC now · takes "),
            Span::raw(format!("{} to {}", tac(terms.min), tac(terms.max))),
            label(" · fee "),
            Span::raw(format!("{} + {}", pct(terms.fee_bps), tac(terms.flat))),
            label(" · open "),
            Span::raw(format!("{}/{max_open}", open.map_or_else(|| "?".into(), |o| o.to_string()))),
        ]));
    }
    out.push(Line::from(vec![
        label("  at the address "),
        up.reverse_sats.map_or_else(dash, |s| Span::raw(format!("{s} sats"))),
        label(" · "),
        up.reverse_notes.map_or_else(dash, |n| Span::raw(format!("{n} note-sized output{}", if n == 1 { "" } else { "s" }))),
        label(if st.shows_stock { "" } else { " · their TAC takes the key: set fast_bridge.status_cmd, or run scripts/helpers/fast-bridge status" }),
        Span::styled(
            if st.explorer && !st.reverse_address.is_empty() { read_note(st.reverse_coins_ok, st.reverse_coins.is_some()) } else { "" },
            Style::new().fg(th.bad),
        ),
    ]));
    if st.shows_stock {
        let mut line = vec![label("  TAC on Bitcoin ")];
        match st.reverse_stock() {
            Some(s) => {
                line.push(Span::styled(tac(s.tac), Style::new().add_modifier(Modifier::BOLD)));
                line.push(Span::styled(format!(" in {} note{}", s.notes, if s.notes == 1 { "" } else { "s" }), th.dim()));
                let age = dur(st.now.saturating_sub(st.stock_at));
                line.push(if st.stock_ok {
                    Span::styled(format!(" (status, {age} ago)"), th.dim())
                } else {
                    Span::styled(format!(" (status, {age} ago: last known, the last run failed)"), Style::new().fg(th.bad))
                });
            }
            None => {
                line.push(dash());
                line.push(match (&st.stock, st.stock_ok) {
                    (Some(_), _) => Span::styled(" (status read another address)", Style::new().fg(th.warn)),
                    (None, true) => Span::raw(""),
                    (None, false) => Span::styled(" (status has not answered yet: a run takes about a minute)", th.dim()),
                });
            }
        }
        out.push(Line::from(line));
    }
    out.extend([Line::from(""), head("To do")]);
    let picked = st.picked();
    let mut nth = 0;
    for s in &up.steps {
        let (mark, color) = match s.level {
            Level::Info => ("· ", th.dim),
            Level::Warn => ("▲ ", th.warn),
            Level::Alert => ("✖ ", th.bad),
        };
        out.push(Line::from(vec![Span::styled(mark, Style::new().fg(color)), Span::raw(s.text.clone())]));
        let last = s.cmd.len().saturating_sub(1);
        // the picked command is marked by shape as well as colour, so it reads in a snapshot too
        let this = !s.cmd.is_empty() && picked == Some(nth);
        nth += usize::from(!s.cmd.is_empty());
        let style = if this { Style::new().fg(th.accent).add_modifier(Modifier::BOLD | Modifier::REVERSED) } else { Style::new().fg(th.accent) };
        for (i, c) in s.cmd.iter().enumerate() {
            let lead = if this && i == 0 { "▶ " } else { "  " };
            let line = if i < last { format!("{lead}{c} \\") } else { format!("{lead}{c}") };
            out.push(Line::from(Span::styled(line, style)));
        }
        out.push(Line::from(""));
    }
    out.push(Line::from(Span::styled(
        "  shift+↑↓ (or K and J) picks a command and c copies it. Every command that sends is a dry run first: run it again with --send --confirm <the token it printed>.",
        th.dim(),
    )));
    let scroll = u16::try_from(st.scroll[Page::Maintenance as usize]).unwrap_or(u16::MAX);
    // wrapped, never cut: a figure or an id that runs off the edge reads as a different one
    f.render_widget(Paragraph::new(out).wrap(Wrap { trim: false }).scroll((scroll, 0)).block(panel(" Maintenance ".into(), th)), left);

    let [notes_area, coins_area] = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(right_pane);
    let unread = |what: &str| format!(" {what} · set fast_bridge.btc_explorer ");
    let notes: Vec<Row> = up
        .notes
        .iter()
        .map(|n| {
            Row::new(vec![
                right(tac(n.tac)),
                if n.created == 0 { Cell::from("—") } else { Cell::from(dur(st.now.saturating_sub(n.created))) },
                Cell::from(format!("{}:{}", short(&n.txid, 8, 4), n.vouts.iter().map(u32::to_string).collect::<Vec<_>>().join("/"))),
            ])
        })
        .collect();
    let title = if st.explorer {
        format!(" TAC from forward takes · {} TAC{} ", tac(total(up.notes.iter().map(|n| n.tac))), read_note(st.main_coins_ok, st.main_coins.is_some()))
    } else {
        unread("TAC from forward takes")
    };
    let widths = [Constraint::Length(12), Constraint::Length(7), Constraint::Fill(1)];
    f.render_widget(table(title, &["         TAC", "AGE", "NOTE (ONE OF)"], notes, &widths, th), notes_area);

    let coins: Vec<Row> = up
        .coins
        .iter()
        .map(|c| {
            let fits = match (upkeep::FUND_COIN.contains(&c.sats), upkeep::TOPUP_COIN.contains(&c.sats)) {
                (true, true) => "fund, topup",
                (true, false) => "fund",
                (false, true) => "topup",
                (false, false) => "",
            };
            Row::new(vec![right(c.sats.to_string()), Cell::from(format!("{}:{}", short(&c.txid, 8, 4), c.vout)), Cell::from(fits)])
        })
        .collect();
    let title =
        if st.explorer { format!(" Plain sats at the main address{} ", read_note(st.main_coins_ok, st.main_coins.is_some())) } else { unread("Plain sats") };
    let widths = [Constraint::Length(9), Constraint::Length(18), Constraint::Fill(1)];
    f.render_widget(table(title, &["     SATS", "COIN", "FOR"], coins, &widths, th), coins_area);
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
        let (st, th) = (State::new(&FastBridgeConfig::default(), true, false, false), Theme::from_config(&ThemeConfig::default()));

        let (wide, narrow) = (header_lines(&st, &th, 200), header_lines(&st, &th, 60));

        assert_eq!(wide.len(), 4);
        assert!(narrow.len() > 4);
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
