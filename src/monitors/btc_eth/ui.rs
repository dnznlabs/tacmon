//! Drawing: the header, and one function per page.

use super::{
    advisor::{Advice, At, Even, Standing},
    ledger::{POOL_SETTLED, Place, SETTLED, Status},
    model::*,
    state::{Page, REF_ETH, REF_SATS, State, shown_name},
    upkeep::LOW_GAS_ETH,
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
                Span::styled(format!("  (its books, {} swap{})", b.swaps, if b.swaps == 1 { "" } else { "s" }), th.dim()),
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
        // the app takes the quote that pays the most, so where the fee stands is whether this provider is picked
        {
            let ranks: Vec<String> = st
                .rank_sizes()
                .into_iter()
                .filter_map(|size| {
                    let (place, of) = st.fee_rank(size)?;
                    Some(match (place, of) {
                        (_, 1) => format!("the only one at {}", sats(size)),
                        (1, of) => format!("cheapest of {of} at {}", sats(size)),
                        (place, of) => format!("#{place} of {of} at {}", sats(size)),
                    })
                })
                .collect();
            if ranks.is_empty() || !st.ads_ok { vec![] } else { vec![label("fee "), Span::raw(ranks.join(" · "))] }
        },
        // the newest quote the provider keeps a file of: one a user held or locked for, or an ETH→BTC one given. Not
        // the last quote given, which the provider writes nowhere: none kept says nothing of whether it is asked
        match st.quotes {
            Some(q) => vec![
                label("last quote kept "),
                match q.last {
                    Some(at) => Span::raw(format!("{} ago", dur(st.now.saturating_sub(at)))),
                    None => Span::styled("none", th.dim()),
                },
                if st.quotes_ok { Span::raw("") } else { bad(" (not read: last known)") },
            ],
            None => vec![],
        },
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
                    Some(e) => Span::styled(format!("{e:.4}"), bold.fg(if e < LOW_GAS_ETH { th.bad } else { Color::Reset })),
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
        Page::Advisor => advisor(f, area, st, th),
        Page::Maintenance => maintenance(f, area, st, th),
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
            ([REF_SATS, 10 * REF_SATS], ["  NAME", "    FEE", "  FLAT SATS", " @100k sats", "   @1M sats", "             TAKES (SATS)", "  AVAILABLE", "ALSO"])
        }
        Direction::Reverse => {
            ([REF_ETH, 10 * REF_ETH], ["  NAME", "    FEE", "   FLAT ETH", "  @0.05 ETH", "   @0.5 ETH", "              TAKES (ETH)", "  AVAILABLE", "ALSO"])
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
            let name = shown_name(a);
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
        format!(" Providers on the board · {} · {} · cheapest first at {}{others} ", dir.label(), listed.len(), heads[3].trim().trim_start_matches('@'))
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
        f.render_widget(Paragraph::new(why).wrap(Wrap { trim: true }).block(panel(" Books ".into(), th)), area);
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

/// A figure that can be under zero, in the direction's own unit.
fn signed(n: i128, show: fn(u128) -> String) -> String {
    format!("{}{}", if n < 0 { "−" } else { "" }, show(n.unsigned_abs()))
}

/// One direction's fee among the others on the board, as lines for the Advisor's right pane.
fn fee_lines(own: &Terms, adv: &Advice, reference: u128, show: fn(u128) -> String, unit: &str, th: &Theme) -> Vec<Line<'static>> {
    let who = |name: &str| name.chars().take(24).collect::<String>();
    let mut out = vec![Line::from(format!(
        "  You charge {} all in on a swap of {} {unit} ({} + {} flat).",
        pct(effective_bps(reference, own.fee_bps, own.flat)),
        show(reference),
        pct(own.fee_bps),
        show(own.flat)
    ))];
    let at = |a: &At| match &a.standing {
        Standing::Unknown => {
            Line::from(Span::styled("  The board's ads have not been read yet, so there is nothing to compare with.", Style::new().fg(th.warn)))
        }
        Standing::Alone => Line::from(format!("  At {}: you take {}, and no other provider on the board takes that size now.", show(a.size), show(a.fee))),
        Standing::Cheapest { next, name } => Line::from(Span::styled(
            format!(
                "  At {}: you take {}, the cheapest on the board (the next, {}, takes {}). You are the one picked.",
                show(a.size),
                show(a.fee),
                who(name),
                show(*next)
            ),
            Style::new().fg(th.good),
        )),
        Standing::Dearer { cheapest, name, flat } => Line::from(Span::styled(
            format!(
                "  At {}: you take {}; {} takes {} and is picked. {}",
                show(a.size),
                show(a.fee),
                who(name),
                show(*cheapest),
                match flat {
                    Some(f) => format!("A flat fee of {} would match it.", show(*f)),
                    None => format!("Your {} alone is over it.", pct(own.fee_bps)),
                }
            ),
            Style::new().fg(th.warn),
        )),
    };
    out.push(at(&adv.low));
    // unread is said once, not once a size
    if let Some(high) = adv.high.as_ref().filter(|h| h.standing != Standing::Unknown) {
        out.push(at(high));
    }
    let marked = match adv.flag {
        Flag::Never => None,
        Flag::Always => Some("every swap you take".to_string()),
        Flag::Under(size) => Some(format!("your swaps under {} {unit}", show(size))),
    };
    if let Some(which) = marked {
        out.push(Line::from(Span::styled(
            format!("  The app marks a fee over {} of a swap as high: at yours, that is {which}.", pct(HIGH_FEE_BPS)),
            Style::new().fg(th.bad),
        )));
    }
    out
}

/// Whether one direction's fee covers what a swap costs, as lines.
fn even_lines(own: &Terms, adv: &Advice, way: &str, show: fn(u128) -> String, unit: &str, th: &Theme) -> Vec<Line<'static>> {
    let Some(even) = adv.even else { return Vec::new() };
    let line = |text: String, color: Color| Line::from(Span::styled(text, Style::new().fg(color)));
    let mut out = vec![match even {
        Even::Always => line(format!("  {way}: your fee covers it at every size you take."), th.good),
        Even::From(size) => line(
            format!(
                "  {way}: your fee covers it from {} {unit}. A smaller swap, down to your min of {}, costs more than it brings.",
                show(size),
                show(own.min)
            ),
            th.warn,
        ),
        Even::OverMax(size) => line(
            format!(
                "  {way}: your fee would cover it from {} {unit}, over your max of {}: every swap you take costs more than it brings.",
                show(size),
                show(own.max)
            ),
            th.bad,
        ),
        Even::Never => line(format!("  {way}: your flat fee is under it and you take no share: every swap costs more than it brings."), th.bad),
    }];
    if let (Some(flat), false) = (adv.cover_flat, even == Even::Always) {
        // covering the cost at the least size makes the fee there the cost itself
        let fee = fee_on(own.min, own.fee_bps, flat);
        let then = match &adv.low.standing {
            Standing::Cheapest { next, .. } if fee <= *next => ", which would still be the cheapest on the board there".to_string(),
            Standing::Cheapest { next, name } | Standing::Dearer { cheapest: next, name, .. } => {
                format!(", which is over {} there, who takes {}", name.chars().take(24).collect::<String>(), show(*next))
            }
            _ => String::new(),
        };
        // said as what it would take, never as what to do: being picked comes before earning more per swap
        out.push(Line::from(format!("  Covering it at your min would take a flat fee of {}{then}.", show(flat))));
    }
    out
}

fn advisor(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let (Some(adv), Some(t)) = (st.advice(Direction::Forward), st.terms.as_ref()) else {
        f.render_widget(Paragraph::new("Waiting for the provider's terms (/info).").block(panel(" Advisor ".into(), th)), area);
        return;
    };
    let [left, right_pane] = Layout::horizontal([Constraint::Length(88), Constraint::Min(30)]).areas(area);
    let two = adv.high.is_some();
    let rows: Vec<Row> = adv
        .rows
        .iter()
        .map(|r| {
            // alone at a size is picked at it
            let picks = match (r.picked_low, r.picked_high) {
                (None, None) => "",
                (low, high) => match (low.unwrap_or(true), high.unwrap_or(true) || !two, two) {
                    (true, true, _) => "cheapest",
                    (true, false, _) => "cheapest at the smaller",
                    (false, true, true) => "cheapest at the larger",
                    (false, _, _) => "dearer",
                },
            };
            let high = match r.high {
                Flag::Never => None,
                Flag::Always => Some("high fee".to_string()),
                Flag::Under(size) => Some(format!("high fee under {}", sats(size))),
            };
            let parts: Vec<&str> =
                [if r.current { "▶ yours" } else { "" }, picks, high.as_deref().unwrap_or("")].into_iter().filter(|p| !p.is_empty()).collect();
            let net = |n: Option<i128>| match n {
                Some(n) => right(signed(n, sats)).style(Style::new().fg(if n < 0 { th.bad } else { th.good })),
                None => right("—".into()).style(th.dim()),
            };
            let row = Row::new(vec![
                right(pct(r.bps)),
                right(sats(r.fee_low)),
                if two { right(sats(r.fee_high)) } else { Cell::from("") },
                net(r.net_low),
                if two { net(r.net_high) } else { Cell::from("") },
                Cell::from(parts.join(" · ")),
            ]);
            match (r.current, r.high) {
                (true, _) => row.style(Style::new().fg(if high.is_some() { th.warn } else { th.accent }).add_modifier(Modifier::BOLD)),
                (_, Flag::Always) => row.style(th.dim()),
                _ => row,
            }
        })
        .collect();
    let sizes = match &adv.high {
        Some(h) => format!("{} and {} sats", sats(adv.low.size), sats(h.size)),
        None => format!("{} sats", sats(adv.low.size)),
    };
    let widths = [Constraint::Length(7), Constraint::Length(9), Constraint::Length(9), Constraint::Length(9), Constraint::Length(9), Constraint::Fill(1)];
    f.render_widget(
        table(
            if st.ads_ok {
                format!(" BTC→ETH fee on a swap of {sizes} · with your flat fee of {} ", sats(t.flat))
            } else {
                format!(" BTC→ETH fee on a swap of {sizes} · THE BOARD DOES NOT ANSWER ")
            },
            &["    FEE", "  SMALLER", "   LARGER", "NET SMALL", "NET LARGE", ""],
            rows,
            &widths,
            th,
        ),
        left,
    );

    let head = |s: &'static str| Line::from(Span::styled(s, Style::new().fg(th.accent).add_modifier(Modifier::BOLD)));
    let faint = |s: &'static str| Line::from(Span::styled(s, th.dim()));
    let mut out = vec![head("BTC→ETH fee (a user's sats for your pool ETH)")];
    out.extend(fee_lines(t, &adv, REF_SATS, sats, "sats", th));
    if st.ads_seen && !st.ads_ok {
        out.push(Line::from(Span::styled("  The board's ads could not be read: this is against the last known ones.", Style::new().fg(th.bad))));
    }
    out.push(faint("  Ads are unsigned: anyone can post one. Check a rival is real on the Market page."));
    out.push(faint("  A page takes the quote that pays the user most, so being the cheapest at a size comes before earning more per swap."));

    out.extend([Line::from(""), head("ETH→BTC fee (a user's pool ETH for your sats)")]);
    let back = st.reverse_terms().zip(st.advice(Direction::Reverse));
    match &back {
        Some((rt, radv)) => out.extend(fee_lines(rt, radv, REF_ETH, eth, "ETH", th)),
        None => out.push(faint(match st.reverse {
            Some(ReverseSide::Off) => "  That side is off.",
            _ => "  That side's terms are not known (/reverse/info).",
        })),
    }

    out.extend([Line::from(""), head("What a swap costs you")]);
    match st.swap_cost().and_then(|c| Some((c, c.as_sats?))) {
        Some((c, whole)) => {
            out.push(Line::from(format!(
                "  About {} sats a swap: {} sats of Bitcoin fees and {} ETH to the relay, {} your books have counted, both ways together.",
                sats(whole),
                sats(c.sats),
                eth(c.eth),
                if c.swaps == 1 { "by the one swap".to_string() } else { format!("the average of the {} swaps", c.swaps) }
            )));
            out.extend(even_lines(t, &adv, "BTC→ETH", sats, "sats", th));
            if let Some((rt, radv)) = &back {
                out.extend(even_lines(rt, radv, "ETH→BTC", eth, "ETH", th));
            }
            out.push(faint("  The NET columns are a fee less this cost. A cost is the past's: the next take pays the going rate of its hour."));
        }
        None => out.push(Line::from(Span::styled(
            match &st.books {
                Some(b) if b.swaps > 0 => "  Not known: there is no price to put the Bitcoin fees and the relay's fees together at.",
                Some(_) => "  Not known yet: your books have counted no swap.",
                None => "  Not known yet: the provider has reported no books.",
            },
            Style::new().fg(th.warn),
        ))),
    }

    out.extend([Line::from(""), head("Size")]);
    match adv.others {
        Some((least, most)) => {
            if t.min < least {
                out.push(Line::from(format!(
                    "  Under {} sats no other provider takes a swap: down to your least of {}, you are the only one.",
                    sats(least),
                    sats(t.min)
                )));
            }
            if t.min > least {
                out.push(Line::from(format!("  Others take swaps from {} sats; your min is {}.", sats(least), sats(t.min))));
            }
            if t.max < most {
                out.push(Line::from(format!("  Others take swaps up to {} sats; your max is {}.", sats(most), sats(t.max))));
            }
            if t.min <= least && t.max >= most {
                out.push(Line::from("  You take every size another provider takes."));
            }
        }
        None => out.push(faint("  No other provider with something to give is on the board to compare sizes with.")),
    }
    if let Some(a) = t.available.filter(|a| *a < t.max) {
        out.push(Line::from(Span::styled(
            format!("  You can take {} sats now, under your max of {}: what you hold free bounds it.", sats(a), sats(t.max)),
            Style::new().fg(th.warn),
        )));
    }

    out.extend([Line::from(""), head("Funding")]);
    // a sat is worth the price in pool units: both count to eight places
    let most = st.price().map(|(p, ..)| (t.max as f64 * p).ceil() as u128).filter(|m| *m > 0);
    match (st.free_eth(), most) {
        (Some(free), Some(most)) => {
            out.push(Line::from(format!("  A swap of your max takes about {} ETH. You have {} free, by the log's last word.", eth(most), eth(free))));
            out.push(match free / most {
                0 => Line::from(Span::styled("  That does not cover one swap of your max.", Style::new().fg(th.warn))),
                n => Line::from(Span::styled(format!("  That covers {n} of them at once."), Style::new().fg(th.good))),
            });
            if let Some(Ok(n)) = st.notes()
                && n.usable < most
            {
                out.push(Line::from(Span::styled(
                    format!("  One lock can draw {} ETH of it, under a swap of your max, until the provider merges its notes.", eth(n.usable)),
                    Style::new().fg(th.warn),
                )));
            }
        }
        (Some(free), None) => {
            out.push(Line::from(format!("  You have {} ETH free, by the log's last word. With no price, what a swap takes is not known.", eth(free))));
        }
        (None, _) => {
            out.push(Line::from(Span::styled("  How much pool ETH is free is not known yet: the log has given no capacity line.", Style::new().fg(th.warn))))
        }
    }
    if let Some(target) = st.books.as_ref().and_then(|b| b.target.as_ref()) {
        out.push(faint(if target.reached {
            "  The provider is at its target: a swap's net is owed to buybacks where they are on."
        } else {
            "  The provider is under its target: every swap's net stays as capacity."
        }));
    }
    // wrapped, never cut: a figure that runs off the edge of the pane reads as a smaller one
    f.render_widget(Paragraph::new(out).wrap(Wrap { trim: false }).block(panel(" Suggestions ".into(), th)), right_pane);
}

fn maintenance(f: &mut Frame, area: Rect, st: &State, th: &Theme) {
    let head = |s: &'static str| Line::from(Span::styled(s, Style::new().fg(th.accent).add_modifier(Modifier::BOLD)));
    let label = |s: &'static str| Span::styled(s, th.dim());
    let dash = || Span::styled("—", th.dim());
    let or_dash = |s: &str| if s.is_empty() { Span::styled("—", th.dim()) } else { Span::raw(s.to_string()) };
    let bold = Style::new().add_modifier(Modifier::BOLD);

    let mut out = vec![head("BTC→ETH (a user's sats for your pool ETH)")];
    out.push(Line::from(vec![
        label("  status "),
        match st.healthy {
            Some(true) => Span::styled("on", Style::new().fg(th.good)),
            Some(false) => Span::styled("its last tick failed", Style::new().fg(th.bad)),
            None => Span::styled("the provider does not answer /health", Style::new().fg(th.bad)),
        },
        label("   pool account "),
        or_dash(&st.id.pool_account),
    ]));
    out.push(Line::from(match &st.terms {
        Some(t) => vec![
            label("  can take "),
            t.available.map_or_else(dash, |a| Span::styled(sats(a), bold.fg(if a < t.min { th.warn } else { th.good }))),
            label(" sats now · takes "),
            Span::raw(format!("{} to {}", sats(t.min), sats(t.max))),
            label(" · fee "),
            Span::raw(format!("{} + {}", pct(t.fee_bps), sats(t.flat))),
            label(" · open "),
            Span::raw(st.open(Direction::Forward).to_string()),
            Span::styled(if st.info_ok { "" } else { " · /INFO NOT ANSWERING: last known" }, Style::new().fg(th.bad)),
        ],
        None => vec![label("  terms "), Span::styled("the provider does not answer /info", Style::new().fg(th.bad))],
    }));
    out.push(Line::from(vec![
        label("  pool ETH "),
        st.free_eth().map_or_else(dash, |f| Span::styled(eth(f), bold)),
        label(" free · "),
        if st.cap_seen { Span::raw(eth(st.cap.promised)) } else { dash() },
        label(" promised · one swap can draw "),
        match st.notes() {
            Some(Ok(n)) => Span::raw(eth(n.usable)),
            _ => dash(),
        },
        if st.shows_eth { label(" · gas ETH ") } else { Span::raw("") },
        match (st.shows_eth, st.eth) {
            (false, _) => Span::raw(""),
            (true, Some(e)) => Span::styled(format!("{e:.4}"), Style::new().fg(if e < LOW_GAS_ETH { th.bad } else { Color::Reset })),
            (true, None) => dash(),
        },
    ]));
    out.push(Line::from(vec![label("  main address "), or_dash(&st.id.bitcoin_address)]));
    // what an address holds is the explorer's to say: without one it is a dash and how to get it, never a zero
    let holds = |address: &str, coins: Option<&[Coin]>, ok: bool| {
        let mut line = vec![label("  at the address ")];
        match (st.explorer, address.is_empty(), State::held(coins)) {
            (false, _, _) => line.extend([dash(), label(" · set btc_eth.btc_explorer to read what it holds")]),
            (true, true, _) => line.extend([dash(), label(" · the address is not known")]),
            (true, false, None) => line.extend([dash(), Span::styled(read_note(ok, false), Style::new().fg(th.bad))]),
            (true, false, Some(h)) => {
                line.push(Span::styled(format!("{} sats", sats(u128::from(h.confirmed))), bold));
                line.push(label(if h.coins == 1 { " confirmed in 1 coin" } else { " confirmed" }));
                if h.coins != 1 {
                    line.push(label(" · "));
                    line.push(Span::raw(format!("{} coins", h.coins)));
                }
                if h.unconfirmed > 0 {
                    line.push(label(" · "));
                    line.push(Span::styled(format!("{} sats not yet confirmed", sats(u128::from(h.unconfirmed))), Style::new().fg(th.warn)));
                }
                line.push(Span::styled(read_note(ok, true), Style::new().fg(th.bad)));
            }
        }
        Line::from(line)
    };
    out.push(holds(&st.id.bitcoin_address, st.main_coins.as_deref(), st.main_coins_ok));

    out.extend([Line::from(""), head("ETH→BTC (a user's pool ETH for your sats)")]);
    out.push(Line::from(vec![
        label("  status "),
        match &st.reverse {
            Some(ReverseSide::On { .. }) => Span::styled("on", Style::new().fg(th.good)),
            Some(ReverseSide::Off) => Span::styled("off", Style::new().fg(th.warn)),
            None => Span::styled("the provider does not answer /reverse/info", Style::new().fg(th.bad)),
        },
        if st.reverse.is_some() && !st.reverse_ok { Span::styled(" (last known)", Style::new().fg(th.bad)) } else { Span::raw("") },
        label("   address "),
        or_dash(&st.reverse_address),
    ]));
    out.push(holds(&st.reverse_address, st.reverse_coins.as_deref(), st.reverse_coins_ok));
    if let Some(ReverseSide::On { terms, open, max_open }) = &st.reverse {
        out.push(Line::from(vec![
            label("  can take "),
            terms.available.map_or_else(dash, |a| Span::styled(eth(a), bold.fg(if a < terms.min { th.warn } else { th.good }))),
            label(" ETH now · takes "),
            Span::raw(format!("{} to {}", eth(terms.min), eth(terms.max))),
            label(" · fee "),
            Span::raw(format!("{} + {}", pct(terms.fee_bps), eth(terms.flat))),
            label(" · open "),
            Span::raw(format!("{}/{max_open}", open.map_or_else(|| "?".into(), |o| o.to_string()))),
        ]));
        out.push(Line::from(vec![
            label("  sats to sell "),
            st.sats_to_sell().map_or_else(dash, |(s, _)| Span::styled(sats(s), bold)),
            label(" at the address, as its upkeep last counted them"),
        ]));
    }

    out.extend([Line::from(""), head("To do")]);
    let picked = st.picked();
    let mut nth = 0;
    for s in &st.upkeep() {
        let mark = match s.level {
            Level::Info => "· ",
            Level::Warn => "▲ ",
            Level::Alert => "✖ ",
        };
        out.push(Line::from(vec![Span::styled(mark, Style::new().fg(level_color(s.level, th))), Span::raw(s.text.clone())]));
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
        "  shift+↑↓ (or K and J) picks a command and c copies it. tacmon runs none of them, and holds no key: each is yours to read and run.",
        th.dim(),
    )));
    let scroll = u16::try_from(st.scroll[Page::Maintenance as usize]).unwrap_or(u16::MAX);
    // wrapped, never cut: a figure or an id that runs off the edge reads as a different one
    f.render_widget(Paragraph::new(out).wrap(Wrap { trim: false }).scroll((scroll, 0)).block(panel(" Maintenance ".into(), th)), area);
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
                Place::Pool => "pool · eth",
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
                // what the provider locked for the user, by the swap record: the pool hides amounts
                _ if r.place == Place::Pool => {
                    st.swaps.iter().find(|s| r.swap.as_ref().is_some_and(|(id, _)| *id == s.id)).map_or_else(|| "—".into(), |s| format!("{} ETH", eth(s.eth)))
                }
                (Some(n), _) => format!("{}{} sats", if n < 0 { "−" } else { "+" }, sats(n.unsigned_abs())),
                (None, Some(s)) => format!("{} sats", sats(s)),
                (None, None) => "—".into(),
            };
            let s = &r.standing;
            // each chain's own depth past which a transaction stays
            let settled = if r.place == Place::Pool { POOL_SETTLED } else { SETTLED };
            let status = match (s.status, s.stuck) {
                (Status::Unconfirmed, true) => Cell::from("▲ unconfirmed · stuck").style(Style::new().fg(th.bad).add_modifier(Modifier::BOLD)),
                (Status::Unconfirmed, false) => Cell::from("unconfirmed").style(Style::new().fg(th.warn)),
                // the provider saw it in a block: its record's word, which carries no count of confirmations
                (Status::Confirmed(_), _) if r.by_record => Cell::from("● in a block").style(Style::new().fg(th.good)),
                (Status::Confirmed(Some(n)), _) if n >= settled => Cell::from(format!("● confirmed · {settled}+")).style(Style::new().fg(th.good)),
                (Status::Confirmed(Some(n)), _) => Cell::from(format!("● confirmed · {n}")).style(Style::new().fg(th.good)),
                (Status::Confirmed(None), _) => Cell::from("● confirmed").style(Style::new().fg(th.good)),
                (Status::Missing, _) => Cell::from("✕ not found").style(Style::new().fg(th.warn)),
                // what the provider wrote of its own transaction is that it sent it: whether it confirmed is the chain's to say
                (Status::Unknown, _) if r.by_record => Cell::from("sent · status unknown").style(th.dim()),
                (Status::Unknown, _) => Cell::from("?").style(th.dim()),
            };
            let fee = s.rate_milli.map_or_else(|| right("—".into()), |m| right(rate(m)).style(if s.slow { Style::new().fg(th.bad) } else { Style::new() }));
            // an unconfirmed one is timed from when this run first saw it, which is all tacmon can know
            let age = match (s.status, s.at) {
                // the provider's own word for when it sent it, where the record gives one
                (Status::Unknown, Some(at)) => format!("sent {}", dur(st.now.saturating_sub(at))),
                (Status::Unconfirmed, Some(at)) => format!("seen {}", dur(st.now.saturating_sub(at))),
                (_, Some(at)) => dur(st.now.saturating_sub(at)),
                _ => "—".into(),
            };
            let age = if r.bumps > 0 { format!("{age} · bumped {}", r.bumps) } else { age };
            // the picked row is marked by shape as well as colour, so it reads in a snapshot too
            let this = picked == Some(i);
            let id = Cell::from(format!("{}{}", if this { "▶ " } else { "  " }, short(&r.txid, 8, 4)));
            let row = Row::new(vec![id, Cell::from(place), Cell::from(what), swap, right(amount), status, fee, Cell::from(age)]);
            if this { row.style(Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)) } else { row }
        })
        .collect();
    let bitcoin = rows.iter().filter(|r| r.place != Place::Pool).count();
    // the pool's log is read so far back and no further: a lock older than that has no row, and the title says how far
    let pool = match (&st.pool, st.shows_pool) {
        (Some(scan), _) => {
            format!(
                " · pool locks · {} in {}{}",
                rows.len() - bitcoin,
                dur(scan.tip.saturating_sub(scan.since).saturating_mul(12)),
                read_note(st.pool_ok, true)
            )
        }
        (None, true) => format!(" · pool locks{}", read_note(st.pool_ok, false)),
        (None, false) => String::new(),
    };
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
            " Bitcoin transactions · {bitcoin} · {waiting} unconfirmed{}{}{pool}{going} ",
            if stuck > 0 { format!(" · {stuck} STUCK") } else { String::new() },
            if failing { read_note(false, read) } else { "" }
        )
    } else {
        format!(" Bitcoin transactions · {bitcoin} by the swap records' word{pool} · set btc_eth.btc_explorer for confirmations and your addresses' ")
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
        drawn_at(st, p, 150)
    }
    /// At a width of the test's choosing: a pane of sentences wraps them, and a test reads them whole.
    fn drawn_at(st: &State, p: Page, width: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(width, 40)).expect("a terminal");
        term.draw(|f| page(f, f.area(), p, st, &theme())).expect("a draw");
        let buf = term.backend().buffer();
        (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_header_with_nothing_read_shows_dashes_and_never_a_zero() {
        let st = State::new(&BtcEthConfig::default(), true, false, false);

        let out = text(&header_lines(&st, &theme(), 200));

        assert!(out.contains("can take — sats") && out.contains("pool ETH free —") && out.contains("held —") && out.contains("under way —"), "{out}");
        assert!(out.contains("the provider does not answer /info") && out.contains("gas ETH ?"), "{out}");
        assert!(!out.contains('0'), "{out}");
    }

    #[test]
    fn a_provider_under_its_minimum_is_said_to_be_full_and_one_without_books_says_so() {
        let mut st = State::new(&BtcEthConfig::default(), false, false, false);
        let terms = Terms { min: 50_000, max: 100_000, fee_bps: 30, flat: 1000, available: Some(20_000), ..Default::default() };

        st.apply(Event::Info(Some(Box::new(Info { terms, books: None }))));

        let out = text(&header_lines(&st, &theme(), 200));
        assert!(out.contains("can take 20,000 sats (full: under its minimum)") && out.contains("no books yet"), "{out}");
        assert!(drawn(&st, Page::Books).contains("its upkeep has reported nothing yet"));
    }

    #[test]
    fn a_loss_is_named_in_the_header_and_on_the_books() {
        let mut st = State::new(&BtcEthConfig::default(), false, false, false);
        let books = Books { swaps: 3, loss_eth: 3_100_000, net_eth: Some(-3_000_000), ..Default::default() };

        st.apply(Event::Info(Some(Box::new(Info { terms: Terms::default(), books: Some(books) }))));

        let (head, page) = (text(&header_lines(&st, &theme(), 200)), drawn(&st, Page::Books));
        assert!(head.contains("LOST 0 sats · 0.03100 ETH") && head.contains("net −0.03000 ETH"), "{head}");
        assert!(page.contains("A SWAP ENDED WITH NOTHING RECEIVED"), "{page}");
    }

    #[test]
    fn a_swap_row_shows_both_sides_what_it_trades_at_and_a_fee_only_once_counted() {
        let mut st = State::new(&BtcEthConfig::default(), false, false, false);
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
        let mut st = State::new(&BtcEthConfig::default(), false, false, false);
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
        let mut st = State::new(&BtcEthConfig::default(), false, false, false);
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
        let mut st = State::new(&BtcEthConfig::default(), false, true, false);
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
        let mut st = State::new(&BtcEthConfig::default(), false, false, false);
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
        assert!(page.contains("1 by the swap records' word · set btc_eth.btc_explorer") && page.contains("lock") && page.contains("ETH→BTC"), "{page}");
    }

    #[test]
    fn without_an_explorer_a_take_shows_what_the_provider_wrote_of_it_and_the_header_says_when_it_last_quoted() {
        let id = |c: char| c.to_string().repeat(64);
        let mut st = State::new(&BtcEthConfig::default(), false, false, false);
        let swap = Swap {
            id: "aaaa1".into(),
            phase: "taking".into(),
            sats: 25_000,
            linked: vec![(Role::UserLock, id('a')), (Role::Take, id('b'))],
            said: vec![(id('b'), Said { fee: Some(500), vsize: Some(99), at: Some(940), bumps: 2 })],
            lock_height: Some(977_636),
            ..Default::default()
        };
        st.tick(1_000);
        st.apply(Event::Swaps(Some(vec![swap])));
        let unasked = text(&header_lines(&st, &theme(), 200));

        st.apply(Event::Quotes(Some(Quotes { count: 0, last: None })));
        let never = text(&header_lines(&st, &theme(), 200));
        st.apply(Event::Quotes(Some(Quotes { count: 4, last: Some(700) })));
        st.apply(Event::Quotes(None));

        let (head, page) = (text(&header_lines(&st, &theme(), 200)), drawn(&st, Page::Transactions));
        let row = |what: &str| page.lines().find(|l| l.contains(what)).unwrap_or_default().to_string();
        assert!(!unasked.contains("last quote") && never.contains("last quote kept none") && !never.contains("not read"), "{never}");
        // the count that failed keeps the last figure, and says it is the last known
        assert!(head.contains("last quote kept 5m00s ago (not read: last known)"), "{head}");
        assert!(row("take").contains("sent · status unknown") && row("take").contains("5.05") && row("take").contains("sent 1m00s · bumped 2"), "{page}");
        assert!(row("user's lock").contains("● in a block"), "{page}");
    }

    #[test]
    fn a_pool_lock_has_a_row_with_the_swaps_eth_and_enter_copies_its_link_on_the_ethereum_site() {
        let cfg = BtcEthConfig { eth_explorer: "https://eth.example/".into(), ..Default::default() };
        let mut st = State::new(&cfg, false, false, true);
        let leaf = "5".repeat(64);
        let swap = Swap { id: "aaaa1".into(), phase: "offered".into(), sats: 25_000, eth: 878_384, pool_leaf: leaf.clone(), ..Default::default() };
        st.apply(Event::Swaps(Some(vec![swap])));
        let unread = drawn(&st, Page::Transactions);

        st.apply(Event::Pool(Some(PoolScan {
            tip: 110,
            since: 10,
            locks: vec![PoolLock { leaf, tx: format!("0x{}", "6".repeat(64)), block: 100, time: None }],
        })));
        st.apply(Event::Pool(None));
        st.on_key(Page::Transactions, ratatui::crossterm::event::KeyEvent::from(ratatui::crossterm::event::KeyCode::Enter));

        let page = drawn(&st, Page::Transactions);
        let row = page.lines().find(|l| l.contains("pool · eth")).unwrap_or_default().to_string();
        assert!(unread.contains("pool locks · not read yet"), "{unread}");
        assert!(page.contains("pool locks · 1 in 20m00s · NOT ANSWERING: last known"), "{page}");
        assert!(row.contains("pool lock") && row.contains("0.00878 ETH") && row.contains("confirmed · 11") && row.contains("BTC→ETH"), "{page}");
        assert_eq!(st.copy, Some(format!("https://eth.example/tx/0x{}", "6".repeat(64))));
    }
    fn ad(name: &str, key: &str, min: u128, max: u128, bps: u64, flat: u128) -> Ad {
        Ad {
            name: name.into(),
            claim_pub: key.into(),
            terms: Terms { min, max, fee_bps: bps, flat, available: Some(max), ..Default::default() },
            reverse: false,
            reverse_terms: None,
        }
    }

    #[test]
    fn the_advisor_says_where_the_fee_stands_and_that_no_swap_it_takes_covers_its_cost() {
        let mut st = State::new(&BtcEthConfig::default(), false, false, false);
        let mine = ad("mine", "02me", 25_000, 97_758, 20, 399);
        st.apply(Event::Log(LogEntry {
            signal: Some(Signal::Started(Identity { claim_pub: "02me".into(), ..Default::default() })),
            ..LogEntry::plain(Level::Info, "x")
        }));
        st.apply(Event::Log(LogEntry { signal: Some(Signal::StartPrice(Some(35.9))), ..LogEntry::plain(Level::Info, "x") }));
        let waiting = drawn_at(&st, Page::Advisor, 280);
        let books = Books { swaps: 1, cost_sats: 500, cost_eth: 26_000, ..Default::default() };
        st.apply(Event::Info(Some(Box::new(Info { terms: mine.terms.clone(), books: Some(books) }))));
        let unread = drawn_at(&st, Page::Advisor, 280);

        st.apply(Event::Ads(Some(vec![mine, ad("other", "03aa", 25_000, 1_000_000, 20, 800)])));

        let page = drawn_at(&st, Page::Advisor, 280);
        let row = |fee: &str| page.lines().find(|l| l.contains(fee)).unwrap_or_default().to_string();
        assert!(waiting.contains("Waiting for the provider's terms") && unread.contains("ads have not been read yet"), "{unread}");
        // 0.20% + 399 on 25,000 is 449; a swap costs 500 sats and 0.00026 ETH, which is 1,225 sats at 35.9
        assert!(row(" 0.20% ").contains("449") && row(" 0.20% ").contains("−776") && row(" 0.20% ").contains("▶ yours · cheapest"), "{page}");
        assert!(page.contains("At 25,000: you take 449, the cheapest on the board (the next, other, takes 850)"), "{page}");
        assert!(page.contains("About 1,225 sats a swap") && page.contains("from 413,000 sats, over your max of 97,758"), "{page}");
        assert!(page.contains("Covering it at your min would take a flat fee of 1,175, which is over other there, who takes 850"), "{page}");
        assert!(page.contains("Others take swaps up to 1,000,000 sats; your max is 97,758"), "{page}");
    }

    #[test]
    fn the_maintenance_page_lists_both_sides_and_marks_the_picked_command() {
        let mut st = State::new(&BtcEthConfig::default(), true, true, false);
        let terms = Terms { min: 25_000, max: 97_758, fee_bps: 20, flat: 399, available: Some(9_000), ..Default::default() };
        st.apply(Event::Info(Some(Box::new(Info { terms, books: None }))));
        st.apply(Event::Health(Some(Health { ok: true, error: String::new() })));
        st.apply(Event::Reverse(Some(ReverseSide::Off)));
        st.apply(Event::Eth(Some(0.0004)));
        // an address nobody has named has nothing read at it; one the config names is read before the side is on
        let unnamed = drawn(&st, Page::Maintenance);
        st.reverse_address = "bc1qrev".into();
        let unread = drawn(&st, Page::Maintenance);
        let coin = |sats, confirmed| Coin { txid: "c".repeat(64), vout: 0, sats, confirmed };
        st.apply(Event::ReverseCoins(Some(vec![coin(100_000, true), coin(40_000, false)])));
        st.apply(Event::ReverseCoins(None));

        st.on_key(Page::Maintenance, ratatui::crossterm::event::KeyEvent::from(ratatui::crossterm::event::KeyCode::Char('J')));
        st.on_key(Page::Maintenance, ratatui::crossterm::event::KeyEvent::from(ratatui::crossterm::event::KeyCode::Char('c')));

        let page = drawn(&st, Page::Maintenance);
        assert!(page.contains("can take 9,000 sats now · takes 25,000 to 97,758 · fee 0.20% + 399") && page.contains("gas ETH 0.0004"), "{page}");
        assert!(page.contains("▲ It can take 9,000 sats now, under its min of 25,000") && page.contains("status off"), "{page}");
        assert!(unnamed.contains("at the address — · the address is not known") && unread.contains("at the address — · not read yet"), "{unread}");
        assert!(page.contains("at the address 100,000 sats confirmed · 2 coins · 40,000 sats not yet confirmed · NOT ANSWERING: last known"), "{page}");
        assert!(page.contains("It holds 100,000 sats confirmed now, and 40,000 more not yet confirmed: under that."), "{page}");
        // the second command is the picked one: marked on the page, and copied whole
        let picked = page.lines().find(|l| l.contains('▶')).unwrap_or_default().to_string();
        assert!(picked.contains("sudo systemctl stop tacit-fastbridge-lp-btc && \\"), "{page}");
        assert_eq!(st.picked(), Some(1));
        assert!(st.copy.as_deref().is_some_and(|c| c.lines().count() == 3 && c.ends_with("sudo systemctl start tacit-fastbridge-lp-btc; }")), "{:?}", st.copy);
    }

    #[test]
    fn a_users_lock_known_by_the_record_alone_is_in_a_block_and_never_a_count_of_confirmations() {
        let id = |c: char| c.to_string().repeat(64);
        // an explorer is set and knows the tip, and has not been asked about this lock yet
        let mut st = State::new(&BtcEthConfig::default(), false, true, false);
        let swap = Swap {
            id: "aaaa1".into(),
            phase: "locked".into(),
            sats: 25_000,
            linked: vec![(Role::UserLock, id('a'))],
            lock_height: Some(100),
            ..Default::default()
        };
        st.apply(Event::Outlook(Some(Outlook { tip: 200, hour_milli: 1_000 })));

        st.apply(Event::Swaps(Some(vec![swap])));

        let page = drawn(&st, Page::Transactions);
        let row = page.lines().find(|l| l.contains("user's lock")).unwrap_or_default().to_string();
        assert!(row.contains("● in a block") && !row.contains("confirmed"), "{page}");
    }
}
