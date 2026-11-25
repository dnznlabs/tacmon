//! What keeping the provider stocked takes now, and the commands that do it. Arithmetic only: no state, no I/O.
//!
//! The commands are for `scripts/helpers/fast-bridge`, which holds the key; tacmon only prints them. Every value put
//! in one is a transaction id, an index, an amount or an address the wire readers have already held to a character
//! set no shell reads anything into.

use super::{
    ledger::{LockWait, Place, TxRow},
    model::{Coin, Direction, Level, ReverseSide, Swap, UNIT, rate, tac},
};
use crate::fmt::dur;

/// Run from the tacmon checkout.
pub const HELPER: &str = "scripts/helpers/fast-bridge";
/// What `fund` moves at most in one run: the tool's own limit ("a small funding, not a move of stock").
pub const FUND_MAX: u128 = 25 * UNIT;
/// The plain coins each tool will spend, in sats.
pub const FUND_COIN: std::ops::RangeInclusive<u64> = 3_000..=20_000;
pub const TOPUP_COIN: std::ops::RangeInclusive<u64> = 10_000..=200_000;
/// About what one reverse swap costs in Bitcoin fees, and how many swaps' worth is too little.
pub const SATS_PER_SWAP: u64 = 3_650;
pub const LOW_SWAPS: u64 = 4;

pub struct Facts<'a> {
    pub swaps: &'a [Swap],
    /// `None`: not read, because no explorer is configured or it has not answered.
    pub main: Option<&'a [Coin]>,
    pub reverse_coins: Option<&'a [Coin]>,
    /// The TAC at the reverse address, where a tool holding the key has read it.
    pub reverse_tac: Option<u128>,
    /// `None`: the provider has not answered.
    pub reverse: Option<&'a ReverseSide>,
    /// Empty while not known.
    pub main_address: &'a str,
    pub reverse_address: &'a str,
    pub pool_account: &'a str,
    pub free: Option<u128>,
    pub forward_min: Option<u128>,
    pub explorer: bool,
    /// The Transactions page's stuck rows, the going fee rate where it is known, and the time.
    pub stuck: &'a [TxRow],
    /// Transactions that paid the reverse key silently, by the key's own word, with the TAC that still waits to be moved.
    pub silent: &'a [(String, u128)],
    /// The transaction `normalize_cmd` last gave no answer for. `None`: it answered, or is not set.
    pub silent_unread: Option<&'a str>,
    /// Forward swaps whose pool lock the pool's log does not show after a long wait.
    pub lock_waits: &'a [LockWait],
    pub hour_milli: Option<u64>,
    pub now: u64,
}

/// The TAC a finished forward swap left at the main address. It is in one of `vouts`: which one takes the key to tell,
/// so all are named and `fund` picks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeNote {
    pub txid: String,
    pub vouts: Vec<u32>,
    pub tac: u128,
    pub created: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub level: Level,
    pub text: String,
    /// The command, one line per argument group, joined with ` \` so it pastes as one.
    pub cmd: Vec<String>,
}

pub struct Upkeep {
    /// Largest first.
    pub notes: Vec<TakeNote>,
    /// The plain coins at the main address a tool may spend: confirmed, and named by no swap under way. Largest first.
    pub coins: Vec<Coin>,
    /// Confirmed plain sats, and note-sized outputs, at the reverse address. `None` while not read.
    pub reverse_sats: Option<u64>,
    pub reverse_notes: Option<usize>,
    pub steps: Vec<Step>,
}

/// "23", "2.5", "0.00000001": what `--tac` takes.
pub fn tac_arg(units: u128) -> String {
    let frac = format!("{:08}", units % UNIT);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() { (units / UNIT).to_string() } else { format!("{}.{frac}", units / UNIT) }
}

fn step(level: Level, text: impl Into<String>, cmd: Vec<String>) -> Step {
    Step { level, text: text.into(), cmd }
}

fn take_notes(swaps: &[Swap], main: &[Coin]) -> Vec<TakeNote> {
    let mut notes: Vec<TakeNote> = swaps
        .iter()
        .filter(|s| s.direction == Direction::Forward && s.paid() && !s.take_txid.is_empty())
        .filter_map(|s| {
            let vouts: Vec<u32> = main.iter().filter(|c| c.txid == s.take_txid && c.note_sized() && c.confirmed).map(|c| c.vout).collect();
            // a take pays the TAC and a zero padding output: with one of them spent, whether the TAC is still there takes the
            // key to tell, so it is not counted. A take with no padding is left out too: understated, never overstated.
            (vouts.len() >= 2).then(|| TakeNote { txid: s.take_txid.clone(), vouts, tac: s.pays, created: s.created })
        })
        .collect();
    notes.sort_by(|a, b| b.tac.cmp(&a.tac).then(b.created.cmp(&a.created)));
    notes
}

/// The largest confirmed coin in `range` that is not `except`.
fn pick<'a>(coins: &'a [Coin], range: &std::ops::RangeInclusive<u64>, except: Option<&Coin>) -> Option<&'a Coin> {
    coins.iter().filter(|c| range.contains(&c.sats) && Some(*c) != except).max_by_key(|c| c.sats)
}

pub fn upkeep(f: &Facts) -> Upkeep {
    let notes = f.main.map(|m| take_notes(f.swaps, m)).unwrap_or_default();
    // a coin a swap under way names is that swap's to spend
    let busy = |c: &Coin| f.swaps.iter().any(|s| !s.closed() && (s.lock_txid == c.txid || s.take_txid == c.txid));
    let mut coins: Vec<Coin> = f.main.unwrap_or_default().iter().filter(|c| !c.note_sized() && c.confirmed && !busy(c)).cloned().collect();
    coins.sort_by_key(|c| std::cmp::Reverse(c.sats));
    let reverse_sats = f.reverse_coins.map(|r| r.iter().filter(|c| !c.note_sized() && c.confirmed).map(|c| c.sats).sum::<u64>());
    let reverse_notes = f.reverse_coins.map(|r| r.iter().filter(|c| c.note_sized() && c.confirmed).count());

    let mut steps = Vec::new();
    if !f.explorer {
        steps.push(step(
            Level::Info,
            "Set fast_bridge.btc_explorer (an Esplora API, such as https://mempool.space/api) to list the coins at your addresses. Your addresses are sent to it.",
            vec![],
        ));
    }
    if f.reverse_address.is_empty() {
        steps.push(step(
            Level::Warn,
            "The reverse side's address is not known. Print it (reads only), then set fast_bridge.reverse_address:",
            vec![format!("{HELPER} status")],
        ));
    }

    for (txid, amount) in f.silent {
        steps.push(step(
            Level::Warn,
            format!(
                "{} TAC was sent to the reverse side silently, in {}: it sits at a one-time address of the reverse key, and the reverse side cannot \
                 sell it until it is moved onto the reverse address (a dry run first):",
                tac(*amount),
                crate::fmt::short(txid, 8, 4)
            ),
            vec![format!("{HELPER} normalize --txid {txid}")],
        ));
    }

    if let Some(txid) = f.silent_unread {
        steps.push(step(
            Level::Warn,
            format!(
                "normalize_cmd gave no answer for {}, so TAC sent silently to the reverse side is not being looked for. It is tried again every ten \
                 minutes. Run it yourself to see what it says:",
                crate::fmt::short(txid, 8, 4)
            ),
            vec![format!("{HELPER} normalize --txid {txid}")],
        ));
    }

    for w in f.lock_waits {
        steps.push(step(
            Level::Warn,
            format!(
                "Swap {} has been making its pool lock for {}, and the pool's log on Ethereum does not show it. A relay sends the lock, not your account: \
                 the Log page has what the relay answered.",
                crate::fmt::short(&w.swap, 8, 4),
                dur(w.secs)
            ),
            vec![],
        ));
    }

    for t in f.stuck {
        let what = match (t.role, &t.swap, t.place) {
            (Some(role), Some((id, _)), _) => format!("The {} of swap {}", role.label(), crate::fmt::short(id, 8, 4)),
            (_, _, Place::Both) => "A transfer between your two addresses".to_string(),
            (_, _, Place::Reverse) => "A transaction at the reverse address".to_string(),
            _ => "A transaction at the main address".to_string(),
        };
        let waited = t.at.map_or_else(String::new, |at| format!(" for {}", dur(f.now.saturating_sub(at))));
        let paying = match (t.rate_milli, f.hour_milli) {
            (Some(r), Some(h)) if t.slow => format!(", paying {} sat/vB where about {} confirms within the hour", rate(r), rate(h)),
            (Some(r), _) => format!(", paying {} sat/vB", rate(r)),
            _ => String::new(),
        };
        let text = format!("{what} has been unconfirmed{waited}{paying}: {}.", t.txid);
        // the helper's bump replaces what the reverse key sent, and nothing else
        steps.push(if t.spends_reverse {
            step(
                Level::Warn,
                format!("{text} The reverse key sent it, so it can be bumped (a dry run first):"),
                vec![format!("{HELPER} bump --txid {}", t.txid)],
            )
        } else {
            step(Level::Warn, format!("{text} Only the wallet that sent it can bump it."), vec![])
        });
    }

    let topup = pick(&coins, &TOPUP_COIN, None);
    let low_sats = reverse_sats.filter(|s| *s < SATS_PER_SWAP * LOW_SWAPS);
    if let Some(sats) = low_sats {
        let text = format!("The reverse address has {sats} sats, under {LOW_SWAPS} reverse swaps' fees (about {SATS_PER_SWAP} each).");
        steps.push(match topup {
            Some(c) => step(
                Level::Warn,
                format!("{text} Move a plain coin of the main address there:"),
                vec![format!("{HELPER} topup"), format!("  --coin {}", c.outpoint())],
            ),
            None => step(Level::Warn, format!("{text} Send sats to it from any Bitcoin wallet: {}", f.reverse_address), vec![]),
        });
    }

    let empty = match f.reverse {
        Some(ReverseSide::On { terms, .. }) => terms.available.is_some_and(|a| a < terms.min),
        Some(ReverseSide::Off) => f.reverse_tac.map_or(reverse_notes == Some(0), |t| t == 0),
        None => false,
    };
    // the TAC the key read, or failing that whether there are notes it could be in
    let holds = f.reverse_tac.map_or(reverse_notes.is_some_and(|n| n > 0), |t| t > 0);
    // with notes at the address the side has TAC, only too little of it for its smallest send
    let why = match f.reverse {
        Some(ReverseSide::On { terms, .. }) if holds => format!(
            "The reverse side {}, which does not cover its smallest send of {}: it can take {} pool TAC now. Lower FB_LP_REVERSE_MIN_AMOUNT, or move more TAC there.",
            f.reverse_tac.map_or_else(|| "has some TAC at its address".to_string(), |t| format!("holds {} TAC on Bitcoin", tac(t))),
            tac(terms.min),
            tac(terms.available.unwrap_or(0)),
        ),
        _ => "The reverse side has no TAC to sell.".to_string(),
    };
    if empty && f.main.is_some() {
        // the smallest note that fills a whole run, or failing that the largest there is
        let note = notes.iter().filter(|n| n.tac >= FUND_MAX).min_by_key(|n| n.tac).or_else(|| notes.first());
        let fee = pick(&coins, &FUND_COIN, if low_sats.is_some() { topup } else { None });
        steps.push(match (note, fee) {
            (Some(n), Some(c)) => {
                let mut cmd = vec![format!("{HELPER} fund --tac {} --mode plain", tac_arg(n.tac.min(FUND_MAX)))];
                cmd.extend(n.vouts.iter().map(|v| format!("  --note {}:{v}", n.txid)));
                cmd.push(format!("  --coin {}", c.outpoint()));
                step(Level::Warn, format!("{why} Move up to {} TAC of a forward take to it (a dry run: it prints the plan and a token):", tac(FUND_MAX)), cmd)
            }
            (Some(_), None) => step(
                Level::Warn,
                format!(
                    "{why} Moving TAC there needs a plain coin of {} to {} sats at the main address for the fee. Send one there: {}",
                    FUND_COIN.start(),
                    FUND_COIN.end(),
                    f.main_address
                ),
                vec![],
            ),
            (None, _) => step(
                Level::Warn,
                format!("{why} No forward take's TAC is unspent at the main address: send TAC on Bitcoin to the reverse tacit1 address (status prints it)."),
                vec![],
            ),
        });
    }

    if f.reverse == Some(&ReverseSide::Off) && holds && reverse_sats.is_some_and(|s| s > 0) {
        steps.push(step(
            Level::Info,
            "The reverse address holds TAC and sats, and the side is off. To switch it on, add FB_LP_REVERSE=on to the provider's environment and restart it:",
            vec!["sudoedit /etc/tacit-fastbridge-lp/env".into(), "sudo systemctl restart tacit-fastbridge-lp".into()],
        ));
    }

    if let (Some(free), Some(min)) = (f.free, f.forward_min)
        && free < min
    {
        let to = if f.pool_account.is_empty() { "the pool account".to_string() } else { format!("the pool account {}", f.pool_account) };
        steps.push(step(
            Level::Warn,
            format!("The forward side has {} TAC free to lend, under its min of {}. Send TAC (the ERC-20, on Ethereum) to {to}: with refill on, the provider shields it.", tac(free), tac(min)),
            vec![],
        ));
    }

    steps.push(step(Level::Info, "Back up both directions' swap records to another disk:", vec!["make backup DEST=<another disk>".into()]));
    Upkeep { notes, coins, reverse_sats, reverse_notes, steps }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::fast_bridge::model::{DUST, Terms};

    const MAIN: &str = "bc1qmain";
    fn id(c: char) -> String {
        c.to_string().repeat(64)
    }
    fn coin(c: char, vout: u32, sats: u64) -> Coin {
        Coin { txid: id(c), vout, sats, confirmed: true }
    }
    fn done(take: char, pays: u128) -> Swap {
        Swap { id: take.into(), phase: "done".into(), created: 1, pays, gets: pays, take_txid: id(take), ..Default::default() }
    }
    fn facts<'a>(swaps: &'a [Swap], main: &'a [Coin], rev: &'a [Coin], side: &'a ReverseSide) -> Facts<'a> {
        Facts {
            swaps,
            main: Some(main),
            reverse_coins: Some(rev),
            reverse_tac: None,
            reverse: Some(side),
            main_address: MAIN,
            reverse_address: "bc1qrev",
            pool_account: "",
            free: None,
            forward_min: None,
            explorer: true,
            stuck: &[],
            lock_waits: &[],
            silent: &[],
            silent_unread: None,
            hour_milli: None,
            now: 0,
        }
    }
    fn cmds(u: &Upkeep, verb: &str) -> Vec<String> {
        u.steps.iter().filter(|s| s.cmd.first().is_some_and(|c| c.contains(verb))).flat_map(|s| s.cmd.clone()).collect()
    }

    #[test]
    fn an_empty_reverse_side_is_funded_from_the_take_that_fills_a_run_with_a_fee_coin_in_range() {
        let swaps = [done('a', 23 * UNIT), done('b', 500 * UNIT), done('c', 100 * UNIT)];
        let main = [
            coin('a', 0, DUST),
            coin('a', 1, DUST),
            coin('b', 0, DUST),
            coin('b', 1, DUST),
            coin('c', 0, DUST),
            coin('c', 1, DUST),
            coin('d', 1, 19_500),
            coin('e', 0, 2_046),
        ];
        let side = ReverseSide::On { terms: Terms { min: 10 * UNIT, available: Some(0), ..Default::default() }, open: Some(0), max_open: 2 };

        let u = upkeep(&facts(&swaps, &main, &[coin('f', 0, 30_000)], &side));

        assert_eq!(u.notes.iter().map(|n| n.tac / UNIT).collect::<Vec<_>>(), [500, 100, 23]);
        assert_eq!(
            cmds(&u, " fund "),
            [
                format!("{HELPER} fund --tac 25 --mode plain"),
                format!("  --note {}:0", id('c')),
                format!("  --note {}:1", id('c')),
                format!("  --coin {}:1", id('d'))
            ]
        );
    }

    #[test]
    fn low_sats_take_the_topup_coin_and_fund_does_not_reuse_it() {
        let swaps = [done('a', 5 * UNIT)];
        let main = [coin('a', 0, DUST), coin('a', 1, DUST), coin('b', 0, 15_000), coin('c', 0, 12_000)];

        let u = upkeep(&facts(&swaps, &main, &[coin('f', 0, 1_000)], &ReverseSide::Off));

        assert_eq!(cmds(&u, " topup"), [format!("{HELPER} topup"), format!("  --coin {}:0", id('b'))]);
        assert_eq!(cmds(&u, " fund ").last(), Some(&format!("  --coin {}:0", id('c'))));
        assert_eq!(cmds(&u, " fund ").first(), Some(&format!("{HELPER} fund --tac 5 --mode plain")));
    }

    #[test]
    fn a_take_with_one_of_its_outputs_spent_is_not_counted_as_still_holding_its_tac() {
        let swaps = [done('a', 23 * UNIT), done('b', 9 * UNIT)];
        let main = [coin('a', 1, DUST), coin('b', 0, DUST), coin('b', 1, DUST)];

        let u = upkeep(&facts(&swaps, &main, &[], &ReverseSide::Off));

        assert_eq!(u.notes.iter().map(|n| n.tac / UNIT).collect::<Vec<_>>(), [9]);
    }

    #[test]
    fn a_coin_a_swap_under_way_names_is_never_offered() {
        let open = Swap { phase: "taking".into(), lock_txid: id('b'), ..done('z', UNIT) };
        let main = [coin('b', 3, 15_000), coin('c', 0, 15_000), Coin { confirmed: false, ..coin('d', 0, 15_000) }];

        let u = upkeep(&facts(&[open], &main, &[], &ReverseSide::Off));

        assert_eq!(u.coins.iter().map(Coin::outpoint).collect::<Vec<_>>(), [format!("{}:0", id('c'))]);
    }

    #[test]
    fn nothing_is_suggested_from_coins_that_were_not_read() {
        let swaps = [done('a', 5 * UNIT)];
        let side = ReverseSide::On { terms: Terms { min: 10 * UNIT, available: Some(0), ..Default::default() }, open: None, max_open: 2 };
        let f = Facts { main: None, reverse_coins: None, explorer: false, ..facts(&swaps, &[], &[], &side) };

        let u = upkeep(&f);

        assert!(cmds(&u, " fund ").is_empty() && cmds(&u, " topup").is_empty());
        assert_eq!((u.reverse_sats, u.reverse_notes), (None, None));
        assert!(u.steps[0].text.contains("btc_explorer"));
    }

    #[test]
    fn a_funded_side_that_is_off_says_how_to_switch_it_on() {
        let rev = [coin('f', 0, 30_000), coin('f', 1, DUST)];

        let u = upkeep(&facts(&[], &[], &rev, &ReverseSide::Off));

        assert!(u.steps.iter().any(|s| s.cmd.iter().any(|c| c.contains("systemctl restart"))));
        assert!(cmds(&u, " fund ").is_empty() && cmds(&u, " topup").is_empty());
    }

    #[test]
    fn a_side_with_tac_under_its_smallest_send_says_so_and_not_that_it_has_none() {
        let side = ReverseSide::On { terms: Terms { min: 150 * UNIT, available: Some(0), ..Default::default() }, open: Some(0), max_open: 2 };
        let (with, without) = ([coin('f', 0, 30_000), coin('f', 1, DUST)], [coin('f', 0, 30_000)]);

        let say = |rev: &[Coin]| upkeep(&facts(&[], &[], rev, &side)).steps.iter().find(|s| s.level == Level::Warn).map(|s| s.text.clone()).unwrap_or_default();

        assert!(say(&with).contains("smallest send of 150.00"), "{}", say(&with));
        assert!(say(&without).starts_with("The reverse side has no TAC to sell."));
    }

    #[test]
    fn the_tac_the_key_read_wins_over_counting_notes() {
        let side = ReverseSide::On { terms: Terms { min: 150 * UNIT, available: Some(0), ..Default::default() }, open: Some(0), max_open: 2 };
        let rev = [coin('f', 0, 30_000), coin('f', 1, DUST)];
        let with = |tac| Facts { reverse_tac: Some(tac), ..facts(&[], &[], &rev, &side) };

        let say = |f: &Facts| upkeep(f).steps.iter().find(|s| s.level == Level::Warn).map(|s| s.text.clone()).unwrap_or_default();

        assert!(say(&with(20 * UNIT)).starts_with("The reverse side holds 20.00 TAC on Bitcoin"), "{}", say(&with(20 * UNIT)));
        assert!(say(&with(0)).starts_with("The reverse side has no TAC to sell."));
    }

    #[test]
    fn tac_arg_is_the_shortest_exact_decimal() {
        let amounts = [23 * UNIT, 25 * UNIT / 10, 25_289_340_101, 1];

        let out = amounts.map(tac_arg);

        assert_eq!(out, ["23", "2.5", "252.89340101", "0.00000001"]);
    }

    #[test]
    fn a_stuck_transaction_is_a_step_and_only_one_the_reverse_key_sent_gets_the_bump_command() {
        use crate::monitors::fast_bridge::ledger::{Place, Status, TxRow};
        let row = |c: char, spends_reverse| TxRow {
            txid: id(c),
            place: Place::Reverse,
            role: None,
            swap: None,
            tac: None,
            sats: Some(546),
            note: true,
            status: Status::Unconfirmed,
            rate_milli: Some(1_160),
            at: Some(400),
            slow: true,
            stuck: true,
            spends_reverse,
        };
        let (stuck, side) = ([row('a', false), row('b', true)], ReverseSide::Off);

        let up = upkeep(&Facts { stuck: &stuck, hour_milli: Some(1_690), now: 1_000, ..facts(&[], &[], &[], &side) });

        let steps: Vec<&Step> = up.steps.iter().filter(|s| s.text.contains("unconfirmed")).collect();
        assert_eq!(steps.len(), 2);
        assert!(
            steps[0].text.contains("for 10m00s, paying 1.16 sat/vB where about 1.69 confirms within the hour") && steps[0].cmd.is_empty(),
            "{}",
            steps[0].text
        );
        assert_eq!(steps[1].cmd, [format!("{HELPER} bump --txid {}", id('b'))]);
    }

    #[test]
    fn a_pool_lock_the_log_does_not_show_is_a_step_with_no_command() {
        let (waits, side) = ([LockWait { swap: "ab".repeat(32), secs: 2_400 }], ReverseSide::Off);

        let up = upkeep(&Facts { lock_waits: &waits, ..facts(&[], &[], &[], &side) });

        let steps: Vec<&Step> = up.steps.iter().filter(|s| s.text.contains("pool lock")).collect();
        assert_eq!(steps.len(), 1);
        assert!(steps[0].text.contains("for 40m00s") && steps[0].level == Level::Warn && steps[0].cmd.is_empty(), "{}", steps[0].text);
    }

    #[test]
    fn tac_the_key_found_sent_silently_is_a_step_with_the_normalize_command() {
        let (silent, side) = ([(id('a'), 200 * UNIT)], ReverseSide::Off);

        let up = upkeep(&Facts { silent: &silent, ..facts(&[], &[], &[], &side) });

        assert_eq!(cmds(&up, "normalize"), [format!("{HELPER} normalize --txid {}", id('a'))]);
        assert!(up.steps.iter().any(|s| s.text.starts_with("200.00 TAC was sent to the reverse side silently")));
    }
}
