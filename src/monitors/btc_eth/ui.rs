//! Drawing: the header, and one function per page.

use super::{
    ledger::{Place, SETTLED, Status},
    model::*,
    state::{Page, REF_ETH, REF_SATS, State},
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

/// "31.020": ETH per BTC.
fn price(p: f64) -> String {
    format!("{p:.3}")
}

/// "+0.04%": a lean, which can be either way.
fn lean(bps: i64) -> String {
    format!("{}{}", if bps < 0 { "−" } else { "+" }, pct(bps.unsigned_abs()))
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

/// The header at this width: each direction, then the books, then status, then where the provider is. Each starts
/// its own line and takes more lines on a narrow terminal.
pub fn header_lines(st: &State, th: &Theme, width: u16) -> Vec<Line<'static>> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let label = |s: &'static str| Span::styled(s, th.dim());
    let dash = || Span::styled("—", th.dim());
    let bad = |s: &'static str| Span::styled(s, Style::new().fg(th.bad));
    let ago = |at: Option<u64>| at.map_or_else(String::new, |at| format!(", {} ago", dur(st.now.saturating_sub(at))));
    let books = st.books.as_ref();

    let forward = vec![
        vec![
            Span::styled("BTC→ETH  ", bold),
            label("can take "),
            // a figure nothing has given yet is a dash, never a zero
            match st.terms.as_ref().map(|t| (t.available, t.min)) {
                Some((Some(a), min)) => Span::styled(sats(a), bold.fg(if a < min { th.warn } else { th.good })),
                _ => dash(),
            },
            label(" sats"),
            // under the least it takes, the app lists it as full
            match st.terms.as_ref() {
                Some(t) if t.available.is_some_and(|a| a < t.min) => Span::styled(" (full: under its minimum)", Style::new().fg(th.warn)),
                _ => Span::raw(""),
            },
        ],
        vec![label("│  pool ETH free "), st.free_eth().map_or_else(dash, |f| Span::styled(eth(f), bold))],
        vec![label("promised "), if st.cap_seen { Span::styled(eth(st.cap.promised), bold) } else { dash() }],
        vec![
            label("held "),
            if st.cap_seen { Span::styled(eth(st.cap.held), bold) } else { dash() },
            // these three are always the log's last word, so they always carry its age
            Span::styled(
                match (st.cap_seen, st.cap_age()) {
                    (false, _) => "  (no capacity line in the log yet)".to_string(),
                    (true, Some(age)) => format!("  ({} open, {} holds · log, {} ago)", st.cap.open, st.cap.holds, dur(age)),
                    (true, None) => format!("  ({} open, {} holds · log)", st.cap.open, st.cap.holds),
                },
                th.dim(),
            ),
        ],
        // one lock draws from the largest sixteen notes: this, not the total, is the most one swap can be given
        match st.notes() {
            Some(Ok(n)) => vec![
                label("│  one swap can draw "),
                Span::styled(eth(n.usable), bold.fg(if n.usable < n.total { th.warn } else { Color::Reset })),
                label(" ETH"),
                Span::styled(format!("  ({} notes)", n.count), th.dim()),
            ],
            // a count of no notes beside ETH held is not a provider that can fund nothing
            Some(Err(_)) => vec![label("│  one swap can draw "), dash(), Span::styled("  (it has not counted its notes)", th.dim())],
            None => vec![],
        },
    ];

    let mut reverse = vec![vec![
        Span::styled("ETH→BTC  ", bold),
        // off and not answering are each said as such: neither is a side with nothing to sell
        match &st.reverse {
            Some(ReverseSide::On { .. }) => Span::styled("on", Style::new().fg(th.good)),
            Some(ReverseSide::Off) => Span::styled("off", th.dim()),
            None => Span::styled("?", th.dim()),
        },
        if !st.reverse_ok && st.healthy.is_some() {
            bad(if st.reverse.is_some() { " (/reverse/info not answering: last known)" } else { " (/reverse/info does not answer)" })
        } else {
            Span::raw("")
        },
    ]];
    if let Some(ReverseSide::On { terms: t, open, max_open }) = &st.reverse {
        reverse.push(vec![
            label("can take "),
            match t.available {
                Some(a) => Span::styled(eth(a), bold.fg(if a < t.min { th.warn } else { th.good })),
                // the provider could not read the coins at its address
                None => dash(),
            },
            label(" ETH"),
        ]);
        reverse.push(vec![label("open "), Span::raw(format!("{}/{max_open}", open.map_or_else(|| "?".into(), |o| o.to_string())))]);
    }
    // the sats it sells from, as its upkeep last counted them
    if let Some((s, at)) = st.sats_to_sell() {
        reverse.push(vec![
            label("│  sats to sell "),
            Span::styled(sats(s), bold),
            Span::styled(format!("  (upkeep{})", ago(Some(at).filter(|a| *a > 0))), th.dim()),
        ]);
    }

    let (stake_sats, stake_eth) = st.at_stake();
    let mut money = vec![vec![
        Span::styled("swaps    ", bold),
        label("under way "),
        if st.swaps_known() { Span::styled(format!("{} sats · {} ETH", sats(stake_sats), eth(stake_eth)), bold) } else { dash() },
    ]];
    match books {
        Some(b) => {
            money.push(vec![label("│  fees earned "), Span::styled(format!("{} sats · {} ETH", sats(b.fees_sats), eth(b.fees_eth)), bold.fg(th.accent))]);
            money.push(vec![
                label("net "),
                b.net_eth.map_or_else(dash, |n| Span::styled(eth_signed(n), bold.fg(if n < 0 { th.bad } else { th.accent }))),
                label(" ETH"),
                Span::styled(format!("  (its books, {} swaps)", b.swaps), th.dim()),
            ]);
            if b.loss_sats > 0 || b.loss_eth > 0 {
                money.push(vec![Span::styled(format!("LOST {} sats · {} ETH", sats(b.loss_sats), eth(b.loss_eth)), bold.fg(th.bad))]);
            }
        }
        // a provider that answers without books has not made its first look: said, not drawn as nothing earned
        None if st.info_ok => money.push(vec![label("│  no books yet: the provider's upkeep has not looked")]),
        None => {}
    }
    if st.swaps_known() && !st.swaps_ok {
        money.push(vec![bad("(records not read: last known)")]);
    }

    let status = vec![
        vec![light(st.healthy, th), label(" health")],
        // unknown, not red, while the board itself is not answering
        vec![light(st.on_board(), th), label(" on board")],
        if st.ad_contested() { vec![bad("an ad that is not yours uses your key")] } else { vec![] },
        vec![light(Some(st.swaps_ok), th), label(" records")],
        vec![
            light(st.priced, th),
            label(" price"),
            match st.price() {
                Some((p, at, whose)) => Span::styled(format!(" {} ETH/BTC ({whose}{})", price(p), ago(at)), th.dim()),
                None => Span::raw(""),
            },
        ],
        if st.priced == Some(false) { vec![bad("no price: it quotes nothing")] } else { vec![] },
        match books.and_then(|b| b.lean_bps) {
            Some(l) if l != 0 => vec![label("lean "), Span::raw(lean(l))],
            _ => vec![],
        },
        match st.stuck().len() {
            0 => vec![],
            n => vec![Span::styled(format!("{n} of its transactions STUCK: see Tx"), Style::new().fg(th.bad).add_modifier(Modifier::BOLD))],
        },
        if !st.info_ok && st.terms.is_some() { vec![bad("/info not answering: terms and books are the last known")] } else { vec![] },
        if st.terms.is_none() { vec![bad("the provider does not answer /info")] } else { vec![] },
        if st.id.dry_run { vec![Span::styled("DRY RUN", Style::new().fg(th.warn))] } else { vec![] },
    ];
    let places = vec![
        vec![label("pool "), Span::raw(short(&st.id.pool_account, 8, 6))],
        vec![label("bitcoin "), Span::raw(short(&st.id.bitcoin_address, 10, 6))],
        if st.reverse_address.is_empty() { vec![] } else { vec![label("reverse "), Span::raw(short(&st.reverse_address, 10, 6))] },
        vec![label("claim "), Span::raw(short(&st.id.claim_pub, 8, 6))],
        if st.shows_eth {
            vec![
                label("gas ETH "),
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
    [forward, reverse, money, status, places].into_iter().flat_map(|row| flow(row, width)).collect()
}

pub fn header(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    f.render_widget(Paragraph::new(header_lines(st, th, area.width)), area);
}

pub fn page(f: &mut Frame, area: Rect, page: Page, st: &State, th: &Theme) {
    match page {
        Page::Overview => overview(f, area, st, th),
        Page::Activity => activity(f, area, st, th, true),
        Page::Market => market(f, area, st, th),
        Page::Books => books(f, area, st, th),
        Page::Transactions => transactions(f, area, st, th),
    }
}

/// "●●●○○○ locked": how far along the six steps a swap is.
fn pipeline(s: &Swap, th: &Theme) -> Line<'static> {
    let phases = s.direction.phases();
    let (dots, color) = match phases.iter().position(|p| *p == s.phase) {
        Some(i) => (format!("{}{}", "●".repeat(i + 1), "○".repeat(phases.len() - i - 1)), if i + 1 == phases.len() { th.good } else { th.accent }),
        // refunding, refunded, rejected, aborted, refund-wait, counter-took, lost: off the happy path
        None => ("✕".repeat(phases.len()), if s.lost() { th.bad } else { th.warn }),
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
            // what the swap itself trades at, the provider's fee and lean included: both sides count to 8 places
            let at = if s.sats == 0 { "—".to_string() } else { price(s.eth as f64 / s.sats as f64) };
            let fee = match st.fee(s) {
                Some(Fee::Sats(v)) => format!("{} sats", sats(v)),
                Some(Fee::Eth(v)) => format!("{} ETH", eth(v)),
                None => "—".into(),
            };
            Row::new(vec![
                Cell::from(short(&s.id, 8, 4)),
                Cell::from(s.direction.label()).style(if s.direction == Direction::Reverse { Style::new().fg(th.accent) } else { th.dim() }),
                Cell::from(pipeline(s, th)),
                right(sats(s.sats)),
                right(eth(s.eth)),
                right(at).style(th.dim()),
                right(fee).style(Style::new().fg(th.accent)),
                // a record that does not say when it began has no age, not one counted from 1970
                if s.created == 0 { Cell::from("—") } else { Cell::from(dur(st.now.saturating_sub(s.created))) },
                deadline,
                Cell::from(short(&s.lock_txid, 8, 4)).style(th.dim()),
            ])
        })
        .collect();
    let way = |d: Direction, word: &str| format!("{} {} open · {} {word}", d.label(), st.open(d), st.paid(d));
    let lost = if st.lost() > 0 { format!(" · {} LOST", st.lost()) } else { String::new() };
    let title = match (st.swaps_known(), st.has_reverse_swaps() || st.reverse_terms().is_some()) {
        (false, _) => " Swaps · the records have not been read ".to_string(),
        (true, true) => format!(" Swaps · {} · {}{lost} ", way(Direction::Forward, "done"), way(Direction::Reverse, "paid")),
        (true, false) => format!(" Swaps · {}{lost} ", way(Direction::Forward, "done")),
    };
    let widths = [
        Constraint::Length(13),
        Constraint::Length(7),
        Constraint::Length(19),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(13),
        Constraint::Length(7),
        Constraint::Length(9),
        Constraint::Fill(1),
    ];
    f.render_widget(
        table(
            title,
            &["SWAP", "WAY", "PHASE", "       SATS", "       ETH", " ETH/BTC", "   FEE EARNED", "AGE", "DEADLINE", "LOCK TX"],
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

fn market(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let dir = st.market_dir;
    // each direction counts in what its user locks: sats one way, pool ETH the other
    let show: fn(u128) -> String = if dir == Direction::Forward { sats } else { eth };
    let (sizes, heads) = match dir {
        Direction::Forward => {
            ([REF_SATS, 10 * REF_SATS], ["  NAME", "    FEE", "  FLAT SATS", " @100k sats", "   @1M sats", "             TAKES (SATS)", "   CAN TAKE", "ALSO"])
        }
        Direction::Reverse => {
            ([REF_ETH, 10 * REF_ETH], ["  NAME", "    FEE", "   FLAT ETH", "  @0.05 ETH", "   @0.5 ETH", "              TAKES (ETH)", "   CAN TAKE", "ALSO"])
        }
    };
    let listed = st.market(dir);
    let rows: Vec<Row> = listed
        .iter()
        .map(|&(a, t)| {
            // the ad this provider's own terms pick out, not every ad that names its key
            let me = st.my_ad().is_some_and(|mine| std::ptr::eq(mine, a));
            let avail = match t.available {
                Some(v) if v < t.min => right("full".into()).style(th.dim()),
                Some(v) => right(show(v)),
                None => right("?".into()),
            };
            // the fee a swap of this size would pay; dimmed where the provider does not take one that size
            let at = |size: u128| {
                let cell = right(pct(effective_bps(size, t.fee_bps, t.flat)));
                if (t.min..=t.max).contains(&size) { cell } else { cell.style(th.dim()) }
            };
            // the provider's own default name: "Provider" and the first six hex digits after the key's prefix byte
            let name = if a.name.is_empty() { format!("Provider {}", a.claim_pub.get(2..8).unwrap_or("")) } else { a.name.clone() };
            let also = match (dir, a.reverse) {
                (Direction::Forward, true) => "ETH→BTC",
                (Direction::Reverse, _) => "BTC→ETH",
                _ => "",
            };
            let row = Row::new(vec![
                Cell::from(format!("{}{name}", if me { "▶ " } else { "  " })),
                right(pct(t.fee_bps)),
                right(show(t.flat)),
                at(sizes[0]),
                at(sizes[1]),
                right(format!("{} – {}", show(t.min), show(t.max))),
                avail,
                Cell::from(also),
            ]);
            if me { row.style(Style::new().fg(th.accent).add_modifier(Modifier::BOLD)) } else { row }
        })
        .collect();
    let others = if st.other_ads == 0 { String::new() } else { format!(" · {} for other pairs not shown", st.other_ads) };
    let title = if st.ads_ok {
        format!(" Providers on the board · {} · {} · cheapest first at the smaller size{others} ", dir.label(), listed.len())
    } else {
        format!(" Providers on the board · {} · {} · THE BOARD DOES NOT ANSWER: last known ", dir.label(), listed.len())
    };
    let widths = [
        Constraint::Length(24),
        Constraint::Length(7),
        Constraint::Length(11),
        Constraint::Length(11),
        Constraint::Length(11),
        Constraint::Length(25),
        Constraint::Length(11),
        Constraint::Fill(1),
    ];
    f.render_widget(table(title, &heads, window(rows, st.scroll[Page::Market as usize]), &widths, th), area);
}

/// The provider's own accounting, as `/info` reports it. Every figure is its count, not this monitor's.
fn books(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let Some(b) = st.books.as_ref() else {
        let why = if st.terms.is_none() {
            "Waiting for the provider (/info)."
        } else {
            "The provider answers, and its upkeep has reported nothing yet. It makes its first look about twenty seconds after it starts, and keeps no books in a dry run."
        };
        f.render_widget(Paragraph::new(why).wrap(ratatui::widgets::Wrap { trim: true }).block(panel(" Books ".into(), th)), area);
        return;
    };
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let head = |s: &'static str| Row::new(vec![Cell::from(s).style(bold.fg(th.accent)), Cell::from(""), Cell::from("")]);
    let row = |name: &'static str, value: String, note: String| Row::new(vec![Cell::from(format!("  {name}")), right(value), Cell::from(note).style(th.dim())]);
    let blank = || Row::new(vec![Cell::from("")]);
    let both = |s: u128, e: u128| format!("{} sats · {} ETH", sats(s), eth(e));
    let since = if b.since == 0 { String::new() } else { format!("since {} ago", dur(st.now.saturating_sub(b.since))) };

    let mut rows = vec![
        head("Swaps counted"),
        row("ended", b.swaps.to_string(), since),
        row("BTC→ETH", b.forward.to_string(), String::new()),
        row("ETH→BTC", b.reverse.to_string(), String::new()),
        row("refunded", b.refunds.to_string(), "ended with both sides back where they began".into()),
        blank(),
        head("Money"),
        row("fees earned", both(b.fees_sats, b.fees_eth), "over the market price each quote was made at".into()),
        row("network costs", both(b.cost_sats, b.cost_eth), "Bitcoin fees of its takes, locks and refunds; the relay's fees".into()),
    ];
    let lost = b.loss_sats > 0 || b.loss_eth > 0;
    rows.push(
        row("losses", both(b.loss_sats, b.loss_eth), if lost { "A SWAP ENDED WITH NOTHING RECEIVED: read its record".into() } else { String::new() })
            .style(if lost { Style::new().fg(th.bad).add_modifier(Modifier::BOLD) } else { Style::new() }),
    );
    rows.push(row("net", b.net_eth.map_or_else(|| "—".into(), |n| format!("{} ETH", eth_signed(n))), "fees less costs and losses, as ETH".into()));
    rows.push(row("kept as capacity", format!("{} ETH", eth(b.reinvested_eth)), String::new()));
    rows.push(row("owed to buybacks", format!("{} ETH", eth(b.owed_eth)), String::new()));

    rows.extend([blank(), head("Inventory")]);
    match &b.stock {
        Some(s) => {
            let looked = if s.at == 0 { String::new() } else { format!("looked {} ago", dur(st.now.saturating_sub(s.at))) };
            rows.push(row("pool ETH", format!("{} ETH", eth(s.eth)), looked));
            let to_sell = st.sats_to_sell();
            rows.push(row(
                "sats to sell",
                to_sell.map_or_else(|| "—".into(), |(v, _)| format!("{} sats", sats(v))),
                if to_sell.is_none() { "ETH→BTC is not on: nothing counts them".into() } else { "at the reverse address".into() },
            ));
            rows.push(row(
                "worth",
                format!("{} ETH", eth(s.value_eth)),
                s.price().map_or_else(String::new, |p| format!("its sats valued at {} ETH/BTC", price(p))),
            ));
            // the two looks are priced apart: the change is the provider's and the market's together
            let change = i128::try_from(s.value_eth).ok().zip(i128::try_from(s.first_value_eth).ok()).map(|(now, first)| now - first);
            if let (Some(c), true) = (change, s.first_at > 0) {
                rows.push(row(
                    "change",
                    format!("{} ETH", eth_signed(c)),
                    format!("since its first look, {} ago; price moves included", dur(st.now.saturating_sub(s.first_at))),
                ));
            }
        }
        None => rows.push(row("pool ETH", "—".into(), "the upkeep has not sampled it yet".into())),
    }
    if let Some(Err(held)) = st.notes() {
        rows.push(row("ETH notes", "—".into(), format!("its count says none while it holds {} ETH: not counted yet", eth(held))));
    }
    if let Some(Ok(n)) = st.notes() {
        rows.push(row("ETH notes", n.count.to_string(), format!("{} of them dust", n.dust)));
        rows.push(row("in them", format!("{} ETH", eth(n.total)), format!("the largest {} ETH", eth(n.largest))));
        rows.push(row(
            "one swap can draw",
            format!("{} ETH", eth(n.usable)),
            if n.usable < n.total { "less than the total: a lock takes sixteen notes at most, until a merge".into() } else { String::new() },
        ));
    }

    rows.extend([blank(), head("Upkeep")]);
    if let Some(t) = &b.target {
        rows.push(row(
            "target",
            format!("{} ETH", eth(t.eth)),
            format!("and Bitcoin worth {} ETH · {}", eth(t.btc_as_eth), if t.reached { "reached" } else { "not reached: every net stays as capacity" }),
        ));
    }
    if let Some(l) = b.lean_bps {
        let note = match l {
            0 => "quoting at the market",
            l if l > 0 => "raised: a user bringing BTC gets a little more ETH",
            _ => "lowered: a user bringing ETH gets a little more BTC",
        };
        rows.push(row("lean", lean(l), note.into()));
    }
    if let Some(x) = &b.buyback {
        rows.push(row("buybacks", if x.on { "on".into() } else { "off".into() }, format!("{} ETH sent in {} sends", eth(x.routed_eth), x.sends)));
    }

    let stale = if st.info_ok { "" } else { " · /INFO DOES NOT ANSWER: last known" };
    let widths = [Constraint::Length(22), Constraint::Length(34), Constraint::Fill(1)];
    f.render_widget(
        ratatui::widgets::Table::new(window(rows, st.scroll[Page::Books as usize]), widths)
            .column_spacing(2)
            .block(panel(format!(" Books · the provider's own count{stale} "), th)),
        area,
    );
}

fn read_note(ok: bool, ever: bool) -> &'static str {
    match (ok, ever) {
        (true, _) => "",
        (false, true) => " · NOT ANSWERING: last known",
        (false, false) => " · not read yet",
    }
}

fn transactions(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let rows = st.ledger();
    let waiting = rows.iter().filter(|r| r.standing.status == Status::Unconfirmed).count();
    let stuck = rows.iter().filter(|r| r.standing.stuck).count();
    let picked = st.tx_picked(&rows);
    let cells: Vec<Row> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let place = match r.place {
                Place::Main => "main",
                Place::Reverse => "reverse",
                Place::Both => "main+reverse",
                Place::Elsewhere => "—",
            };
            // one no swap names is told by which way the sats went
            let what = match (r.role, r.sats) {
                (Some(role), _) => role.label(),
                (None, _) if r.place == Place::Both => "transfer",
                (None, Some(n)) if n >= 0 => "received",
                (None, _) => "sent",
            };
            let swap = r.swap.as_ref().map_or_else(|| Cell::from("—").style(th.dim()), |(id, way)| Cell::from(format!("{} {}", short(id, 8, 0), way.label())));
            // what an explorer saw move at the provider's addresses, fee and all; else the swap record's word for it
            let amount = match (r.sats, r.swap_sats) {
                (Some(n), _) => format!("{}{} sats", if n < 0 { "−" } else { "+" }, sats(n.unsigned_abs())),
                (None, Some(s)) => format!("{} sats", sats(s)),
                (None, None) => "—".into(),
            };
            let s = &r.standing;
            let status = match (s.status, s.stuck) {
                (Status::Unconfirmed, true) => Cell::from("▲ unconfirmed · stuck").style(Style::new().fg(th.bad).add_modifier(Modifier::BOLD)),
                (Status::Unconfirmed, false) => Cell::from("unconfirmed").style(Style::new().fg(th.warn)),
                (Status::Confirmed(Some(n)), _) if n >= SETTLED => Cell::from(format!("● confirmed · {SETTLED}+")).style(Style::new().fg(th.good)),
                (Status::Confirmed(Some(n)), _) => Cell::from(format!("● confirmed · {n}")).style(Style::new().fg(th.good)),
                (Status::Confirmed(None), _) => Cell::from("● confirmed").style(Style::new().fg(th.good)),
                (Status::Missing, _) => Cell::from("✕ not found").style(Style::new().fg(th.warn)),
                (Status::Unknown, _) => Cell::from("?").style(th.dim()),
            };
            let fee = s.rate_milli.map_or_else(|| right("—".into()), |m| right(rate(m)).style(if s.slow { Style::new().fg(th.bad) } else { Style::new() }));
            // an unconfirmed one is timed from when this run first saw it, which is all tacmon can know
            let age = match (s.status, s.at) {
                (Status::Unconfirmed, Some(at)) => format!("seen {}", dur(st.now.saturating_sub(at))),
                (_, Some(at)) => dur(st.now.saturating_sub(at)),
                _ => "—".into(),
            };
            // the picked row is marked by shape as well as colour, so it reads in a snapshot too
            let this = picked == Some(i);
            let id = Cell::from(format!("{}{}", if this { "▶ " } else { "  " }, short(&r.txid, 8, 4)));
            let row = Row::new(vec![id, Cell::from(place), Cell::from(what), swap, right(amount), status, fee, Cell::from(age)]);
            if this { row.style(Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)) } else { row }
        })
        .collect();
    // counts and what is not answering first, the going rate last: a narrow terminal cuts the title from the right
    let title = if st.explorer {
        let going = match (st.outlook, st.outlook_ok) {
            (Some(o), true) => format!(" · about {} sat/vB confirms within the hour", rate(o.hour_milli)),
            (Some(o), false) => format!(" · about {} sat/vB confirmed within the hour when last read", rate(o.hour_milli)),
            (None, _) => String::new(),
        };
        // any of the explorer's answers failing leaves a part of the page as it last was
        let read = st.main_txs.is_some() || st.reverse_txs.is_some();
        let failing = !(st.main_txs_ok || st.id.bitcoin_address.is_empty()) || !(st.reverse_txs_ok || st.reverse_address.is_empty()) || !st.lookups_ok;
        format!(
            " Bitcoin transactions · {} · {waiting} unconfirmed{}{}{going} ",
            rows.len(),
            if stuck > 0 { format!(" · {stuck} STUCK") } else { String::new() },
            if failing { read_note(false, read) } else { "" }
        )
    } else {
        format!(" Bitcoin transactions · {} from the swap records · set btc_eth.btc_explorer for their status and your addresses' ", rows.len())
    };
    let widths = [
        Constraint::Length(15),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(17),
        Constraint::Length(17),
        Constraint::Length(21),
        Constraint::Length(7),
        Constraint::Fill(1),
    ];
    f.render_widget(
        table(
            title,
            &["  TX", "AT", "WHAT", "SWAP", "           AMOUNT", "STATUS", " SAT/VB", "AGE"],
            // the window follows the pick: two lines of border and one of headings are not rows
            window(cells, picked.unwrap_or(0).saturating_sub(usize::from(area.height.saturating_sub(4)))),
            &widths,
            th,
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::ThemeConfig,
        monitors::btc_eth::{config::BtcEthConfig, feed::Event},
    };
    use ratatui::{Terminal, backend::TestBackend};

    fn theme() -> Theme {
        Theme::from_config(&ThemeConfig::default())
    }
    fn text(lines: &[Line]) -> String {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }
    fn drawn(st: &State, p: Page) -> String {
        let mut term = Terminal::new(TestBackend::new(150, 40)).expect("a terminal");
        term.draw(|f| page(f, f.area(), p, st, &theme())).expect("a draw");
        let buf = term.backend().buffer();
        (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_header_with_nothing_read_shows_dashes_and_never_a_zero() {
        let st = State::new(&BtcEthConfig::default(), true, false);

        let out = text(&header_lines(&st, &theme(), 200));

        assert!(out.contains("can take — sats") && out.contains("pool ETH free —") && out.contains("held —") && out.contains("under way —"), "{out}");
        assert!(out.contains("the provider does not answer /info") && out.contains("gas ETH ?"), "{out}");
        assert!(!out.contains('0'), "{out}");
    }

    #[test]
    fn a_provider_under_its_minimum_is_said_to_be_full_and_one_without_books_says_so() {
        let mut st = State::new(&BtcEthConfig::default(), false, false);
        let terms = Terms { min: 50_000, max: 100_000, fee_bps: 30, flat: 1000, available: Some(20_000), ..Default::default() };

        st.apply(Event::Info(Some(Box::new(Info { terms, books: None }))));

        let out = text(&header_lines(&st, &theme(), 200));
        assert!(out.contains("can take 20,000 sats (full: under its minimum)") && out.contains("no books yet"), "{out}");
        assert!(drawn(&st, Page::Books).contains("its upkeep has reported nothing yet"));
    }

    #[test]
    fn a_loss_is_named_in_the_header_and_on_the_books() {
        let mut st = State::new(&BtcEthConfig::default(), false, false);
        let books = Books { swaps: 3, loss_eth: 3_100_000, net_eth: Some(-3_000_000), ..Default::default() };

        st.apply(Event::Info(Some(Box::new(Info { terms: Terms::default(), books: Some(books) }))));

        let (head, page) = (text(&header_lines(&st, &theme(), 200)), drawn(&st, Page::Books));
        assert!(head.contains("LOST 0 sats · 0.03100 ETH") && head.contains("net −0.03000 ETH"), "{head}");
        assert!(page.contains("A SWAP ENDED WITH NOTHING RECEIVED"), "{page}");
    }

    #[test]
    fn a_swap_row_shows_both_sides_what_it_trades_at_and_a_fee_only_once_counted() {
        let mut st = State::new(&BtcEthConfig::default(), false, false);
        let swap = |id: &str, phase: &str| Swap { id: id.into(), phase: phase.into(), sats: 100_000, eth: 3_100_000, ..Default::default() };
        st.apply(Event::Swaps(Some(vec![swap("aaaa1", "done"), swap("bbbb2", "taking")])));

        st.apply(Event::Log(LogEntry { signal: Some(Signal::Counted { id: "aaaa1".into(), fee: Fee::Sats(1_300) }), ..LogEntry::plain(Level::Info, "x") }));

        let out = drawn(&st, Page::Overview);
        let row = |id: &str| out.lines().find(|l| l.contains(id)).unwrap_or_default().to_string();
        assert!(
            row("aaaa1").contains("100,000") && row("aaaa1").contains("0.03100") && row("aaaa1").contains("31.000") && row("aaaa1").contains("1,300 sats"),
            "{out}"
        );
        assert!(row("bbbb2").contains('—') && !row("bbbb2").contains("sats"), "{out}");
    }

    #[test]
    fn the_market_lists_each_direction_in_its_own_unit() {
        let mut st = State::new(&BtcEthConfig::default(), false, false);
        let reverse = Terms { min: 1_000_000, max: 100_000_000, fee_bps: 30, flat: 20_000, available: Some(500_000), ..Default::default() };
        let terms = Terms { min: 50_000, max: 500_000, fee_bps: 30, flat: 1_000, available: Some(400_000), ..Default::default() };
        st.apply(Event::Ads(Some(vec![Ad { name: "north".into(), claim_pub: "02aa".into(), terms, reverse: true, reverse_terms: Some(reverse) }])));

        let forward = drawn(&st, Page::Market);
        st.market_dir = Direction::Reverse;
        let back = drawn(&st, Page::Market);

        // 0.30% + 1,000 sats on 100,000 sats; 0.30% + 0.0002 ETH on 0.05 ETH; and under its least, it is full
        assert!(forward.contains("north") && forward.contains("1.30%") && forward.contains("50,000 – 500,000") && forward.contains("400,000"), "{forward}");
        assert!(back.contains("0.70%") && back.contains("0.01000 – 1.00000") && back.contains("full"), "{back}");
    }

    #[test]
    fn a_side_that_is_off_shows_no_sats_to_sell_and_notes_not_counted_are_not_drawn_as_none() {
        let mut st = State::new(&BtcEthConfig::default(), false, false);
        let books = Books {
            stock: Some(Stock { eth: 6_974_000, sats: Some(0), value_eth: 6_974_000, ..Default::default() }),
            notes: Some(Notes::default()),
            ..Default::default()
        };
        st.apply(Event::Reverse(Some(ReverseSide::Off)));

        st.apply(Event::Info(Some(Box::new(Info { terms: Terms::default(), books: Some(books) }))));

        let (head, page) = (text(&header_lines(&st, &theme(), 200)), drawn(&st, Page::Books));
        assert!(
            head.contains("one swap can draw —  (it has not counted its notes)") && !head.contains("sats to sell") && !head.contains("can draw 0"),
            "{head}"
        );
        assert!(page.contains("ETH→BTC is not on") && page.contains("its count says none while it holds 0.06974 ETH"), "{page}");
    }

    #[test]
    fn the_transactions_page_says_what_each_is_to_its_swap_and_the_header_names_a_stuck_one_of_the_providers() {
        let id = |c: char| c.to_string().repeat(64);
        let mut st = State::new(&BtcEthConfig::default(), false, true);
        let swap = Swap {
            id: "aaaa1".into(),
            phase: "taking".into(),
            sats: 100_000,
            eth: 3_100_000,
            linked: vec![(Role::UserLock, id('a')), (Role::Take, id('b'))],
            ..Default::default()
        };
        st.apply(Event::Swaps(Some(vec![swap])));
        st.apply(Event::Outlook(Some(Outlook { tip: 105, hour_milli: 2_400 })));
        st.tick(1_000);
        st.apply(Event::MainTxs(Some(vec![ChainTx { txid: id('b'), fee: 100, vsize: 100, received: 99_500, ..Default::default() }])));
        st.apply(Event::Lookups {
            found: vec![(id('a'), Some(ChainTx { txid: id('a'), fee: 500, vsize: 100, height: Some(100), time: Some(400), ..Default::default() }))],
            answered: true,
            wanted: vec![id('a'), id('b')],
        });

        // one sat per virtual byte against 2.4: slow, and past its grace
        st.tick(1_000 + 600);

        let (head, page) = (text(&header_lines(&st, &theme(), 200)), drawn(&st, Page::Transactions));
        let row = |what: &str| page.lines().find(|l| l.contains(what)).unwrap_or_default().to_string();
        assert!(head.contains("1 of its transactions STUCK: see Tx"), "{head}");
        assert!(page.contains("2 · 1 unconfirmed · 1 STUCK") && page.contains("about 2.40 sat/vB"), "{page}");
        assert!(
            row("take").contains("main") && row("take").contains("+99,500 sats") && row("take").contains("unconfirmed · stuck") && row("take").contains("1.00"),
            "{page}"
        );
        assert!(
            row("user's lock").contains("100,000 sats") && row("user's lock").contains("confirmed · 6+") && row("user's lock").contains("BTC→ETH"),
            "{page}"
        );
    }

    #[test]
    fn without_an_explorer_the_transactions_page_lists_what_the_records_name_and_says_how_to_get_more() {
        let mut st = State::new(&BtcEthConfig::default(), false, false);
        let swap = Swap {
            id: "aaaa1".into(),
            phase: "broadcast".into(),
            direction: Direction::Reverse,
            sats: 100_000,
            linked: vec![(Role::Lock, "c".repeat(64))],
            ..Default::default()
        };

        st.apply(Event::Swaps(Some(vec![swap])));

        let page = drawn(&st, Page::Transactions);
        assert!(page.contains("1 from the swap records · set btc_eth.btc_explorer") && page.contains("lock") && page.contains("ETH→BTC"), "{page}");
    }
}
