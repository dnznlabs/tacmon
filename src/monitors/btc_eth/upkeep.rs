//! What keeping a provider of Bitcoin for ETH running takes now, and the commands that do it. Arithmetic only: no
//! state, no I/O.
//!
//! tacmon only prints the commands. They are Tacit's own setup and shield tools, systemd, an editor and this checkout's backup,
//! and the only values put in one come from the operator's own configuration: where the provider's data is, the
//! unit it runs as and the port it answers on. Nothing read from the provider, the board or a chain is put in one.

use super::{
    ledger::TxRow,
    model::{Level, Notes, ReverseSide, Step, Terms, eth, eth_cut, rate, sats},
};
use crate::fmt::{dur, short};

/// Tacit's setup tool, run from a tacit checkout as the user the provider runs as: it reads the key itself.
pub const SETUP: &str = "node fastbridge-lp/setup.mjs";
/// What a provider keeps at its pool account for gas, and the least over that it would shield, where it does not
/// say itself: Tacit's own defaults (`FB_LP_SHIELD_RESERVE_ETH`, `FB_LP_SHIELD_MIN_ETH`).
pub const GAS_RESERVE_ETH: f64 = 0.02;
pub const MIN_CHUNK_ETH: f64 = 0.02;
/// Tacit's tool that puts ETH of the pool account into the pool. Without `--send` it only checks.
pub const SHIELD: &str = "node tools/fastbridge-lp-shield.mjs";
/// The least a provider of the pair may set as its smallest swap: under it a take at a busy fee would leave dust.
pub const FLOOR_SATS: u128 = 20_000;

/// What an address holds, by an explorer's list of its unspent coins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    pub confirmed: u64,
    pub unconfirmed: u64,
    pub coins: usize,
}

/// Tacit's setup asks for this much over the max of a swap before it turns ETH→BTC on.
pub const REVERSE_MARGIN_SATS: u128 = 40_000;

/// Where the provider is on this machine, by the operator's configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Place {
    pub data_dir: String,
    /// The systemd unit, where the log command names one.
    pub unit: Option<String>,
    /// The port of its API, where the address names one.
    pub port: Option<u16>,
}

impl Place {
    /// `unit` from a log command's `-u <unit>`, `port` from the API's address.
    pub fn new(data_dir: &str, log_cmd: &[String], lp_url: &str) -> Self {
        let unit = log_cmd.iter().position(|a| a == "-u").and_then(|i| log_cmd.get(i + 1)).filter(|u| !u.is_empty()).cloned();
        let host = lp_url.split("://").last().unwrap_or_default().split('/').next().unwrap_or_default();
        let port = host.rsplit_once(':').and_then(|(_, p)| p.parse().ok());
        Self { data_dir: data_dir.to_string(), unit, port }
    }
    fn setup(&self, verb: &str) -> String {
        let port = self.port.map_or_else(String::new, |p| format!(" --port {p}"));
        format!("{SETUP}{verb} --data {}{port}", arg(&self.data_dir))
    }
    /// Reads only: its health, its terms and its ad.
    fn status(&self) -> Vec<String> {
        vec![self.setup(" status")]
    }
    /// Setup run again with the provider stopped: it shields what has arrived and writes the limits afresh. The
    /// provider is started again whatever setup says: one that failed or was interrupted must not leave it stopped.
    fn again(&self) -> Vec<String> {
        match &self.unit {
            Some(u) => {
                vec![
                    format!("sudo systemctl stop {} &&", arg(u)),
                    format!("  {{ {} --no-start;", self.setup("")),
                    format!("  sudo systemctl start {}; }}", arg(u)),
                ]
            }
            None => vec![format!("{} --no-start", self.setup(""))],
        }
    }
    /// The shield tool's dry run for `amount` ETH (`<ETH>` where it is not known). The tool takes the key in its
    /// environment: the operator's shell reads it from the key file setup made, and tacmon never does.
    fn shield(&self, amount: Option<&str>) -> Vec<String> {
        let key = arg(&format!("{}/provider.key", self.data_dir.trim_end_matches('/')));
        vec![format!("FB_LP_KEY=$(cat {key})"), format!("  {SHIELD} {} --eth", amount.unwrap_or("<ETH>"))]
    }
    fn edit(&self) -> Vec<String> {
        let edit = format!("${{EDITOR:-nano}} {}", arg(&format!("{}/config.json", self.data_dir.trim_end_matches('/'))));
        match &self.unit {
            Some(u) => vec![format!("{edit} &&"), format!("  sudo systemctl restart {}", arg(u))],
            None => vec![edit],
        }
    }
}

/// A value as a shell reads it for one argument: bare where every character is plain, else in single quotes.
pub fn arg(s: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':' | '@' | '+' | ',' | '=');
    // quoting does not stop a leading dash reading as an option: a path that starts with one is named from here
    let s = if s.starts_with('-') { format!("./{s}") } else { s.to_string() };
    if !s.is_empty() && s.chars().all(plain) { s } else { format!("'{}'", s.replace('\'', r"'\''")) }
}

pub struct Facts<'a> {
    pub place: &'a Place,
    /// `Some(false)`: its last tick failed. `None`: it does not answer.
    pub healthy: Option<bool>,
    pub health_error: &'a str,
    /// The BTC→ETH terms. `None`: never read.
    pub terms: Option<&'a Terms>,
    /// Swaps under way, and the pool ETH the log last said is promised to them.
    pub open: usize,
    pub promised: Option<u128>,
    /// The provider's count of its ETH notes, or the ETH it holds beside a count of none.
    pub notes: Option<Result<&'a Notes, u128>>,
    pub reverse: Option<&'a ReverseSide>,
    pub reverse_address: &'a str,
    /// What the reverse address holds, where an explorer has listed its coins.
    pub reverse_held: Option<Held>,
    pub pool_account: &'a str,
    /// The pool account's own ETH, where it is read.
    pub gas: Option<f64>,
    /// What the provider keeps there for gas, and the least over it worth shielding: its own word, or the defaults.
    pub gas_reserve: f64,
    pub min_chunk: f64,
    /// Whether the provider shields what arrives by itself. `None`: it does not say.
    pub auto_shield: Option<bool>,
    pub priced: Option<bool>,
    pub on_board: Option<bool>,
    /// The Tx page's stuck rows that are the provider's own.
    pub stuck: &'a [TxRow],
    /// The ids of the swaps that ended with the provider out of pocket.
    pub lost: &'a [&'a str],
    pub explorer: bool,
    pub hour_milli: Option<u64>,
    pub now: u64,
}

fn step(level: Level, text: impl Into<String>, cmd: Vec<String>) -> Step {
    Step { level, text: text.into(), cmd }
}

pub fn upkeep(f: &Facts) -> Vec<Step> {
    let mut steps = Vec::new();
    let account = if f.pool_account.is_empty() { "its pool account".to_string() } else { format!("its pool account, {}", f.pool_account) };

    match f.healthy {
        None => steps.push(step(
            Level::Alert,
            "The provider does not answer /health: it is not running, or not where btc_eth.lp_url says. A swap under way waits for it, and has about a day.",
            f.place.unit.as_ref().map(|u| vec![format!("systemctl status {}", arg(u))]).unwrap_or_default(),
        )),
        Some(false) => steps.push(step(
            Level::Alert,
            format!(
                "Its last tick failed{}. It tries again on the next one; the Log page has the line.",
                if f.health_error.is_empty() { String::new() } else { format!(": {}", f.health_error) }
            ),
            vec![],
        )),
        Some(true) => {}
    }

    if !f.lost.is_empty() {
        let ids: Vec<String> = f.lost.iter().map(|id| short(id, 8, 4)).collect();
        steps.push(step(
            Level::Alert,
            format!(
                "{} ended with the provider paid nothing for what it gave: {}. Read the swap's record and its claim or lock before anything else. The records are in {}/swaps.",
                if ids.len() == 1 { "A swap".to_string() } else { format!("{} swaps", ids.len()) },
                ids.join(", "),
                f.place.data_dir.trim_end_matches('/')
            ),
            vec![],
        ));
    }

    if f.priced == Some(false) {
        steps.push(step(
            Level::Alert,
            "It has no price, so it quotes nothing: its sources are down, or disagree. Check this machine reaches Ethereum nodes and the exchanges' tickers; it starts quoting again by itself.",
            f.place.status(),
        ));
    }

    for t in f.stuck {
        let what = match (t.role, &t.swap) {
            (Some(role), Some((id, _))) => format!("The {} of swap {}", role.label(), short(id, 8, 4)),
            _ => "A transaction at one of its addresses".to_string(),
        };
        let s = &t.standing;
        let waited = s.at.map_or_else(String::new, |at| format!(" for {}", dur(f.now.saturating_sub(at))));
        let paying = match (s.rate_milli, f.hour_milli) {
            (Some(r), Some(h)) if s.slow => format!(", paying {} sat/vB where about {} confirms within the hour", rate(r), rate(h)),
            (Some(r), _) => format!(", paying {} sat/vB", rate(r)),
            _ => String::new(),
        };
        steps.push(step(
            Level::Warn,
            format!(
                "{what} has been unconfirmed{waited}{paying}: {}. The provider replaces a slow take or lock itself with one that pays more; where it has not, the Log page says why.",
                t.txid
            ),
            vec![],
        ));
    }

    if let Some(t) = f.terms
        && let Some(a) = t.available.filter(|a| *a < t.min)
    {
        let held = f.promised.filter(|p| *p > 0 && f.open > 0);
        steps.push(match held {
            // lent out, not run out: it comes back as the swaps end
            Some(p) => step(
                Level::Info,
                format!(
                    "It can take {} sats now, under its min of {}, while {} swap{} under way hold{} {} ETH: the app lists it as full until they end.",
                    sats(a),
                    sats(t.min),
                    f.open,
                    if f.open == 1 { "" } else { "s" },
                    if f.open == 1 { "s" } else { "" },
                    eth(p)
                ),
                vec![],
            ),
            None => step(
                Level::Warn,
                format!(
                    "It can take {} sats now, under its min of {}, so the app lists it as full. Send ETH (Ethereum mainnet) to {account}: once it has arrived, \
                     a step here says how to shield it.",
                    sats(a),
                    sats(t.min)
                ),
                vec![],
            ),
        });
    }

    // ETH at the account is not lent: only what is shielded into the pool is. Under the provider's reserve it is
    // gas and not to be shielded; over it by a chunk, it waits for a shield
    if let Some(over) = f.gas.map(|g| g - f.gas_reserve).filter(|over| *over + 1e-9 >= f.min_chunk) {
        // cut, never rounded up: the tool refuses more than the account can spare
        let (amount, held) = (eth_cut(over), eth_cut(over + f.gas_reserve));
        let reserve = eth_cut(f.gas_reserve);
        steps.push(if f.auto_shield == Some(true) {
            step(
                Level::Info,
                format!("Its pool account holds {held} ETH of its own, {amount} over the {reserve} it keeps for gas. Nothing to run: the provider shields it by itself, a chunk at a time."),
                vec![],
            )
        } else {
            step(
                Level::Warn,
                format!(
                    "Its pool account holds {held} ETH of its own, {amount} over the {reserve} it keeps for gas. ETH sent there is not lent until it is shielded \
                     into the pool, and this provider does not do that by itself. Tacit's shield tool does (from a tacit checkout, as the provider's user, while \
                     no swap is under way). This is its dry run: it checks and sends nothing; add --send to send it. Then raise `quote.maxAmount` (below). Or \
                     set FB_LP_AUTO_SHIELD=on in the provider's environment, and it shields what arrives from then on."
                ),
                f.place.shield(Some(&amount)),
            )
        });
    }
    if let Some(g) = f.gas.filter(|g| *g < f.gas_reserve) {
        steps.push(step(
            Level::Warn,
            format!(
                "Its pool account holds {} ETH of its own, under the {} it keeps for gas: that pays for the settles and deposits it sends itself. Send ETH to \
                 {account}; it stays there as gas and is not to be shielded.",
                eth_cut(g),
                eth_cut(f.gas_reserve)
            ),
            vec![],
        ));
    }

    match f.reverse {
        Some(ReverseSide::On { terms, .. }) => match terms.available {
            Some(a) if a < terms.min => {
                let to = if f.reverse_address.is_empty() { "its reverse address".to_string() } else { format!("its reverse address, {}", f.reverse_address) };
                steps.push(step(
                    Level::Warn,
                    format!(
                        "ETH→BTC can take {} ETH now, under its min of {}: the sats it sells from do not cover more. Send plain sats to {to}, from any Bitcoin wallet.",
                        eth(a),
                        eth(terms.min)
                    ),
                    vec![],
                ));
            }
            None => steps.push(step(
                Level::Warn,
                "ETH→BTC cannot read the coins at its reverse address, and refuses quotes until it can. It tries again by itself: the trouble is its explorers'.",
                vec![],
            )),
            Some(_) => {}
        },
        Some(ReverseSide::Off) => {
            // only a tool with the key can name the address before the side is on: the operator's config carries it here
            let to = if f.reverse_address.is_empty() {
                "the provider's reverse address (setup prints it; set btc_eth.reverse_address to have it shown here)".to_string()
            } else {
                format!("its reverse address, {}", f.reverse_address)
            };
            // setup stops the provider for minutes: not offered while a swap waits on it
            let busy = f.open > 0;
            let want = f.terms.map(|t| t.max.saturating_add(REVERSE_MARGIN_SATS));
            let much = want.map_or_else(String::new, |w| format!(" Tacit's guide is the max of a swap and {} sats more: {} sats.", sats(REVERSE_MARGIN_SATS), sats(w)));
            // what is there already, where an explorer says: a send that has arrived is not asked for again
            let there = f.reverse_held.map_or_else(String::new, |h| {
                let waiting = if h.unconfirmed > 0 { format!(", and {} more not yet confirmed", sats(u128::from(h.unconfirmed))) } else { String::new() };
                let enough = match want {
                    Some(w) if u128::from(h.confirmed) >= w => ": enough, so setup is all that is left",
                    Some(_) => ": under that",
                    None => "",
                };
                format!(" It holds {} sats confirmed now{waiting}{enough}.", sats(u128::from(h.confirmed)))
            });
            steps.push(step(
                Level::Info,
                format!(
                    "ETH→BTC is off. To turn it on, send Bitcoin to {to}.{much}{there} Wait for one confirmation, then run Tacit's setup again with the provider \
                     stopped (from a tacit checkout, as the provider's user; it writes the limits afresh, and the provider is started again whatever setup says){}",
                    if busy { ". Not while a swap is under way: the command is here once none is." } else { ":" }
                ),
                if busy { vec![] } else { f.place.again() },
            ));
        }
        None => {}
    }

    if f.on_board == Some(false) && f.healthy == Some(true) {
        steps.push(step(
            Level::Warn,
            "Its ad is not on the board, so no page offers it. It posts one about a minute after it starts, and a first start reads the pool for two minutes or \
             so; after that the Log page says why a post failed. Its own check (reads only, from a tacit checkout):",
            f.place.status(),
        ));
    }
    match f.notes {
        Some(Err(held)) => steps.push(step(
            Level::Info,
            format!(
                "The provider reports no count of its ETH notes while it holds {} ETH, so the most one swap can draw is not known here. Nothing to run: the count is its own.",
                eth(held)
            ),
            vec![],
        )),
        Some(Ok(n)) if n.usable < n.total => steps.push(step(
            Level::Info,
            format!(
                "One swap can draw {} of the {} ETH in its {} notes: a lock takes sixteen at most. Nothing to run: the provider merges its smallest notes itself, \
                 a few times a day, while no BTC→ETH swap is under way.",
                eth(n.usable),
                eth(n.total),
                n.count
            ),
            vec![],
        )),
        _ => {}
    }

    if !f.explorer {
        steps.push(step(
            Level::Info,
            "Set btc_eth.btc_explorer (an Esplora API, such as https://mempool.space/api) for the status of its transactions. Its addresses and transaction ids are sent to it.",
            vec![],
        ));
    }

    steps.push(step(
        Level::Info,
        format!(
            "To change the fee or the sizes it takes, edit `quote` in its config and restart it: a swap under way is read back and finished. It refuses a min \
             under {} sats. The Advisor page has what each fee would bring.",
            sats(FLOOR_SATS)
        ),
        f.place.edit(),
    ));
    steps.push(step(
        Level::Info,
        "Back up both directions' swap records to another disk (from the tacmon checkout):",
        vec![format!("DATA_DIR={} make backup DEST=<another disk>", arg(&f.place.data_dir))],
    ));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::{
        btc_eth::{ledger::Place as TxPlace, model::Role},
        common::chain::{Standing, Status},
    };

    fn place() -> Place {
        Place::new("/var/lib/lp-btc", &["journalctl", "-u", "lp-btc", "-f"].map(String::from), "http://127.0.0.1:8788")
    }
    fn facts(place: &Place) -> Facts<'_> {
        Facts {
            place,
            healthy: Some(true),
            health_error: "",
            terms: None,
            open: 0,
            promised: None,
            notes: None,
            reverse: None,
            reverse_address: "",
            reverse_held: None,
            pool_account: "0xpool",
            gas: None,
            gas_reserve: GAS_RESERVE_ETH,
            min_chunk: MIN_CHUNK_ETH,
            auto_shield: None,
            priced: None,
            on_board: None,
            stuck: &[],
            lost: &[],
            explorer: true,
            hour_milli: None,
            now: 0,
        }
    }
    fn said(steps: &[Step], what: &str) -> Option<Step> {
        steps.iter().find(|s| s.text.contains(what)).cloned()
    }

    #[test]
    fn a_healthy_provider_with_nothing_short_is_told_how_to_change_its_terms_and_to_back_up() {
        let p = place();

        let steps = upkeep(&facts(&p));

        assert_eq!(
            steps.iter().map(|s| (s.level, s.cmd.clone())).collect::<Vec<_>>(),
            [
                (Level::Info, vec!["${EDITOR:-nano} /var/lib/lp-btc/config.json &&".to_string(), "  sudo systemctl restart lp-btc".to_string()]),
                (Level::Info, vec!["DATA_DIR=/var/lib/lp-btc make backup DEST=<another disk>".to_string()]),
            ]
        );
    }

    #[test]
    fn where_it_is_comes_from_the_configuration_and_a_value_a_shell_would_read_into_is_quoted() {
        let odd = Place::new("/srv/my lp's", &["cat".to_string(), "/tmp/log".to_string()], "http://lp.local/");

        let (known, cmd) = (place(), odd.again());

        assert_eq!((known.unit.as_deref(), known.port, odd.unit.as_deref(), odd.port), (Some("lp-btc"), Some(8788), None, None));
        // with no unit named there is nothing to stop or start, and no port is passed that was not given
        assert_eq!(cmd, [r"node fastbridge-lp/setup.mjs --data '/srv/my lp'\''s' --no-start"]);
        assert_eq!(
            (arg("a-b_c.d/e:1"), arg(""), arg("$(x)"), arg("-rf")),
            ("a-b_c.d/e:1".to_string(), "''".to_string(), "'$(x)'".to_string(), "./-rf".to_string())
        );
    }

    #[test]
    fn a_provider_under_its_min_is_sent_eth_unless_swaps_under_way_hold_it() {
        let p = place();
        let terms = Terms { min: 25_000, max: 97_758, available: Some(9_000), ..Default::default() };

        let empty = upkeep(&Facts { terms: Some(&terms), ..facts(&p) });
        let lent = upkeep(&Facts { terms: Some(&terms), open: 1, promised: Some(878_384), ..facts(&p) });

        let step = said(&empty, "lists it as full").expect("a step");
        assert!(step.level == Level::Warn && step.text.contains("9,000 sats now, under its min of 25,000") && step.text.contains("0xpool"), "{}", step.text);
        // setup shields nothing for a provider already over two swaps of its min: no command is offered until ETH is there
        assert!(step.cmd.is_empty());
        let step = said(&lent, "lists it as full").expect("a step");
        assert!(step.level == Level::Info && step.cmd.is_empty() && step.text.contains("1 swap under way holds 0.00878 ETH"), "{}", step.text);
    }

    #[test]
    fn what_stops_it_being_paid_is_an_alert_and_comes_first() {
        let p = place();
        let lost = ["ab".repeat(32)];
        let lost: Vec<&str> = lost.iter().map(String::as_str).collect();

        let steps = upkeep(&Facts { healthy: None, priced: Some(false), lost: &lost, ..facts(&p) });

        let alerts: Vec<&Step> = steps.iter().take_while(|s| s.level == Level::Alert).collect();
        assert_eq!(alerts.len(), 3);
        assert_eq!(alerts[0].cmd, ["systemctl status lp-btc"]);
        assert!(alerts[1].text.contains("abababab…abab") && alerts[1].text.contains("/var/lib/lp-btc/swaps"), "{}", alerts[1].text);
        assert_eq!(alerts[2].cmd, ["node fastbridge-lp/setup.mjs status --data /var/lib/lp-btc --port 8788"]);
    }

    #[test]
    fn each_side_says_what_it_is_short_of_and_a_side_that_is_off_how_to_turn_it_on() {
        let p = place();
        let low = ReverseSide::On { terms: Terms { min: 1_000_000, available: Some(200_000), ..Default::default() }, open: Some(0), max_open: 2 };
        let blind = ReverseSide::On { terms: Terms { min: 1_000_000, available: None, ..Default::default() }, open: None, max_open: 2 };

        let read =
            [&low, &blind, &ReverseSide::Off].map(|side| upkeep(&Facts { reverse: Some(side), reverse_address: "bc1qrev", gas: Some(0.0004), ..facts(&p) }));

        assert!(said(&read[0], "ETH→BTC can take 0.00200 ETH now, under its min of 0.01000").is_some_and(|s| s.text.contains("bc1qrev") && s.cmd.is_empty()));
        assert!(said(&read[1], "cannot read the coins").is_some_and(|s| s.level == Level::Warn));
        assert!(
            said(&read[2], "ETH→BTC is off").is_some_and(|s| s.level == Level::Info && s.cmd.len() == 3 && s.text.contains("its reverse address, bc1qrev."))
        );
        // with no address configured it says where one comes from, and with terms how much Tacit's setup asks for
        let terms = Terms { max: 97_758, ..Default::default() };
        let unset = upkeep(&Facts { reverse: Some(&ReverseSide::Off), terms: Some(&terms), ..facts(&p) });
        assert!(said(&unset, "set btc_eth.reverse_address").is_some_and(|s| s.text.contains("137,758 sats") && !s.text.contains("It holds")));
        let held = |confirmed, unconfirmed| {
            let f = Facts { reverse: Some(&ReverseSide::Off), terms: Some(&terms), reverse_held: Some(Held { confirmed, unconfirmed, coins: 1 }), ..facts(&p) };
            said(&upkeep(&f), "ETH→BTC is off").map(|s| s.text).unwrap_or_default()
        };
        assert!(held(0, 140_000).contains("It holds 0 sats confirmed now, and 140,000 more not yet confirmed: under that."), "{}", held(0, 140_000));
        assert!(held(140_000, 0).contains("It holds 140,000 sats confirmed now: enough, so setup is all that is left."), "{}", held(140_000, 0));
        assert!(said(&read[0], "holds 0.0004 ETH of its own, under the 0.0200").is_some_and(|s| s.text.contains("0xpool")));
    }

    #[test]
    fn a_stuck_transaction_of_its_own_and_an_ad_that_is_not_up_are_each_a_step() {
        let p = place();
        let row = TxRow {
            txid: "b".repeat(64),
            place: TxPlace::Main,
            role: Some(Role::Take),
            swap: Some((format!("aaaa1111{}bbbb", "0".repeat(52)), Default::default())),
            swap_sats: None,
            sats: None,
            standing: Standing { status: Status::Unconfirmed, rate_milli: Some(1_160), at: Some(400), slow: true, stuck: true },
            bumps: 0,
            by_record: false,
        };

        let steps = upkeep(&Facts { stuck: &[row], hour_milli: Some(1_690), now: 1_000, on_board: Some(false), ..facts(&p) });

        let stuck = said(&steps, "unconfirmed").expect("a step");
        assert!(
            stuck
                .text
                .starts_with("The take of swap aaaa1111…bbbb has been unconfirmed for 10m00s, paying 1.16 sat/vB where about 1.69 confirms within the hour"),
            "{}",
            stuck.text
        );
        assert!(said(&steps, "Its ad is not on the board").is_some_and(|s| s.cmd.len() == 1));
    }

    #[test]
    fn notes_that_were_not_counted_and_notes_one_lock_cannot_draw_whole_are_said_with_nothing_to_run() {
        let p = place();
        let notes = Notes { count: 20, total: 9_000_000, usable: 7_000_000, ..Default::default() };

        let (uncounted, split) = (upkeep(&Facts { notes: Some(Err(6_095_616)), ..facts(&p) }), upkeep(&Facts { notes: Some(Ok(&notes)), ..facts(&p) }));

        assert!(said(&uncounted, "no count of its ETH notes while it holds 0.06095 ETH").is_some_and(|s| s.cmd.is_empty()));
        assert!(said(&split, "One swap can draw 0.07000 of the 0.09000 ETH in its 20 notes").is_some_and(|s| s.cmd.is_empty()));
    }

    #[test]
    fn eth_over_the_providers_gas_reserve_waits_for_a_shield_and_eth_under_it_is_gas() {
        let p = place();
        let with = |gas: f64| upkeep(&Facts { gas: Some(gas), ..facts(&p) });

        let (arrived, gas_only, short, unread) = (with(0.069_997), with(0.03), with(0.019_986_738), upkeep(&facts(&p)));

        let step = said(&arrived, "is not lent until it is shielded").expect("a step");
        // 0.049997 over the 0.02 kept: cut to four places, not rounded up to more than is there
        assert!(step.level == Level::Warn && step.text.contains("holds 0.0699 ETH of its own, 0.0499 over the 0.0200"), "{}", step.text);
        assert_eq!(step.cmd, ["FB_LP_KEY=$(cat /var/lib/lp-btc/provider.key)", "  node tools/fastbridge-lp-shield.mjs 0.0499 --eth"]);
        // 0.01 over the reserve is under a chunk: it is gas, and nobody is told to shield it
        assert!(said(&gas_only, "is not lent").is_none() && said(&gas_only, "under the").is_none() && said(&unread, "is not lent").is_none());
        // a hair under the reserve is under it, and is not drawn as the reserve
        assert!(said(&short, "holds 0.0199 ETH of its own, under the 0.0200 it keeps for gas").is_some_and(|s| s.text.contains("0xpool")));
    }

    #[test]
    fn the_providers_own_reserve_and_chunk_decide_and_one_that_shields_by_itself_is_left_to_it() {
        let p = place();
        let own = Facts { gas: Some(0.08), gas_reserve: 0.05, min_chunk: 0.01, ..facts(&p) };

        let (by_hand, by_itself) = (upkeep(&own), upkeep(&Facts { auto_shield: Some(true), ..own }));

        assert!(said(&by_hand, "0.0300 over the 0.0500").is_some_and(|s| s.level == Level::Warn && s.cmd.len() == 2));
        assert!(said(&by_itself, "shields it by itself").is_some_and(|s| s.level == Level::Info && s.cmd.is_empty()));
    }

    #[test]
    fn the_setup_chain_starts_the_provider_whatever_setup_says() {
        assert_eq!(
            place().again(),
            [
                "sudo systemctl stop lp-btc &&",
                "  { node fastbridge-lp/setup.mjs --data /var/lib/lp-btc --port 8788 --no-start;",
                "  sudo systemctl start lp-btc; }"
            ]
        );
    }
}
