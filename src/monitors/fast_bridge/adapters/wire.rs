//! The provider's and the board's JSON, read into the model. None of it is a stable interface, so every
//! reader is lenient: a missing field is zero or empty, and a log line that is not recognised is still shown.

pub use crate::monitors::common::wire::{btc_address, health, txid_text, word32};
use crate::monitors::{
    common::wire::{asset, field, kept, log_detail, log_head, small, text, txid, units},
    fast_bridge::model::{Ad, Capacity, Direction, Identity, Level, LogEntry, ReverseSide, ReverseStock, Role, Signal, Swap, Terms, UNIT, Want, tac},
};
use serde_json::Value;

/// The transactions a record names, in the order they happen. One it does not name yet, or names unreadably, is left out.
fn linked(v: &Value, named: &[(Role, Option<&str>, &str)]) -> Vec<(Role, String)> {
    named.iter().filter_map(|(role, inside, key)| Some((*role, txid(inside.map_or(Some(v), |k| v.get(k))?, key)?))).collect()
}

/// `/info`, and the same fields in a board ad. `None` when the size range or the fee is not there to read:
/// terms of all zeros would be drawn as real ones.
pub fn terms(v: &Value) -> Option<Terms> {
    Some(Terms {
        btc_asset: asset(v, "btcAsset"),
        evm_asset: asset(v, "evmAsset"),
        min: v.get("minAmount").and_then(units)?,
        max: v.get("maxAmount").and_then(units)?,
        fee_bps: u64::try_from(v.get("feeBps").and_then(units)?).unwrap_or(u64::MAX),
        flat: field(v, "flatFee"),
        available: v.get("available").and_then(units),
    })
}

/// A record in the provider's `swaps/`. `None` for a file that is not a swap (no offer in it).
pub fn swap(v: &Value) -> Option<Swap> {
    let offer = v.get("offer")?;
    Some(Swap {
        direction: Direction::Forward,
        id: text(v, "id"),
        phase: text(v, "phase"),
        created: small(v, "created"),
        deadline: small(v, "deadline"),
        // an offer without both amounts is not a swap to count: zeros would understate what is at stake
        pays: offer.get("btc")?.get("value").and_then(units)?,
        gets: offer.get("evm")?.get("amount").and_then(units)?,
        lock_txid: text(v, "lockTxid"),
        take_txid: v.get("take").and_then(|t| txid(t, "txid")).unwrap_or_default(),
        // the lock is named twice, in the offer and once the provider has seen it: the same transaction
        linked: txid(v, "lockTxid")
            .or_else(|| txid(offer.get("btc")?, "txid"))
            .map(|id| (Role::BridgerLock, id))
            .into_iter()
            .chain(linked(v, &[(Role::TakeCommit, Some("take"), "commitTxid"), (Role::Take, Some("take"), "txid")]))
            .collect(),
        pool_leaf: v.get("lock").and_then(|l| word32(l.get("lockLeaf")?.as_str()?)).unwrap_or_default(),
    })
}

/// A record in the provider's `reverse/swaps/`. The provider locks TAC on Bitcoin (`amount`) for the bridger's pool
/// TAC (`evmAmount`). `None` without both amounts, as forward.
pub fn reverse_swap(v: &Value) -> Option<Swap> {
    Some(Swap {
        direction: Direction::Reverse,
        id: text(v, "id"),
        phase: text(v, "phase"),
        created: small(v, "created"),
        // known once the bridger has offered its pool lock
        deadline: v.get("evmOffer").map_or(0, |o| small(o, "deadline")),
        pays: v.get("evmAmount").and_then(units)?,
        gets: v.get("amount").and_then(units)?,
        lock_txid: v.get("lock").map(|l| text(l, "txid")).unwrap_or_default(),
        take_txid: String::new(),
        linked: linked(
            v,
            &[
                (Role::LockCommit, Some("lock"), "commitTxid"),
                (Role::Lock, Some("lock"), "txid"),
                (Role::Refund, Some("refund"), "txid"),
                (Role::TakeRefund, Some("takeRefund"), "txid"),
            ],
        ),
        // in reverse the pool lock is the bridger's, and the record does not name its leaf
        pool_leaf: String::new(),
    })
}

/// `/reverse/info`. `None` for an answer without terms: it is a failed read, not a side with nothing to sell.
pub fn reverse(v: &Value) -> Option<ReverseSide> {
    Some(ReverseSide::On { terms: terms(v)?, open: v.get("open").and_then(units).map(|n| u64::try_from(n).unwrap_or(u64::MAX)), max_open: small(v, "maxOpen") })
}

pub fn want(v: &Value) -> Want {
    // a cap that is there but cannot be read is the strictest cap, not "any fee"
    let max_fee_bps = match v.get("maxFeeBps") {
        None | Some(Value::Null) => None,
        Some(cap) => Some(units(cap).map_or(0, |c| u64::try_from(c).unwrap_or(u64::MAX))),
    };
    Want { btc_asset: asset(v, "btcAsset"), amount: field(v, "amount"), max_fee_bps, until: small(v, "until") }
}

/// `None` for an ad without readable terms: it is left out of the market, not listed at a fee of zero.
pub fn ad(v: &Value) -> Option<Ad> {
    Some(Ad {
        name: text(v, "name"),
        claim_pub: text(v, "claimPub"),
        terms: terms(v)?,
        any_tac: v.get("anyTac").and_then(Value::as_bool).unwrap_or(false),
        reverse: v.get("reverse").is_some_and(|r| !r.is_null() && r != &Value::Bool(false)),
        reverse_terms: v.get("reverse").and_then(terms),
    })
}

/// "20", "2.5", "0.00000001" in TAC as base units. More than 8 places, or anything else, is `None`.
fn decimal_tac(s: &str) -> Option<u128> {
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if whole.is_empty() || frac.len() > 8 || !whole.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    whole.parse::<u128>().ok()?.checked_mul(UNIT)?.checked_add(format!("{frac:0<8}").parse::<u128>().ok()?)
}

/// What tacit's `fastbridge-lp-reverse-status.mjs` prints, which is not a stable interface: the three lines it is
/// read from are found by their labels. `None` without the address or the TAC line: a run that stopped part way.
pub fn reverse_stock(out: &str) -> Option<ReverseStock> {
    let after = |label: &str| out.lines().find_map(|l| l.strip_prefix(label)).map(str::trim);
    let address = btc_address(after("reverse address")?.split_whitespace().next()?)?;
    // "1 confirmed note, 20 TAC in all  (+…)"
    let mut tac_line = after("TAC at it")?.split_whitespace();
    let notes = tac_line.next()?.parse().ok()?;
    let tac = tac_line.skip_while(|w| !w.ends_with(',')).nth(1).and_then(decimal_tac)?;
    // "1 confirmed coin, 30000 sats in all", or "could not be read from any explorer"
    let sats = after("plain sats").and_then(|l| l.split_whitespace().skip_while(|w| !w.ends_with(',')).nth(1)?.parse().ok());
    Some(ReverseStock { address, notes, tac, sats })
}

/// What tacit's `fastbridge-lp-reverse-normalize.mjs` prints for one transaction when it sends nothing. `Some(Some)`:
/// that much TAC from it waits to be moved. `Some(None)`: none of it is this key's to move. `None`: it did not say
/// (no key, an explorer that failed, not confirmed yet), which is no verdict.
pub fn silent_verdict(out: &str) -> Option<Option<u128>> {
    if let Some(amount) = out.lines().find_map(|l| l.trim().strip_prefix("would move ")).and_then(|l| decimal_tac(l.split_whitespace().next()?)) {
        return Some(Some(amount));
    }
    ["no TAC from that transaction was found for this key", "already a plain note at the reverse address"].iter().any(|said| out.contains(said)).then_some(None)
}

/// One line of the provider's log. `None` only for a blank line: anything that is not its JSON is shown as it is.
pub fn log_line(raw: &str) -> Option<LogEntry> {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return (!raw.trim().is_empty()).then(|| LogEntry::plain(Level::Info, kept(raw.trim().to_string())));
    };
    let msg = text(&v, "msg");
    let signal = match msg.as_str() {
        "fast bridge LP" => Some(Signal::Started(Identity {
            bitcoin_address: text(&v, "bitcoinAddress"),
            claim_pub: text(&v, "claimPub"),
            pool_account: text(&v, "poolAccount"),
            dry_run: v.get("dryRun").and_then(Value::as_bool).unwrap_or(false),
        })),
        // without both amounts it is a line to show, not capacity to believe
        "capacity" => v
            .get("held")
            .and_then(units)
            .zip(v.get("promised").and_then(units))
            .map(|(held, promised)| Signal::Capacity(Capacity { held, promised, open: small(&v, "open"), holds: small(&v, "holds") })),
        m if m.starts_with("the reverse side is on") => btc_address(&text(&v, "reverseAddress")).map(Signal::ReverseOn),
        _ => None,
    };
    let (time, at, level) = log_head(&v);
    // TAC base units wherever the provider logs them under these names
    let detail = log_detail(&msg, &v, |key, units| matches!(key, "free" | "held" | "promised").then(|| tac(units)));
    Some(LogEntry { time, at, level, detail: kept(detail), msg: kept(msg), signal })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::common::wire::{MAX_KEPT, eth_address};
    use serde_json::json;

    #[test]
    fn swap_record_reads_both_sides_of_the_offer() {
        let record = json!({
            "id": "aa11", "phase": "locking", "created": 1795500342u64, "deadline": 1795515000u64, "lockTxid": "bb22",
            "offer": { "btc": { "value": "25289340101" }, "evm": { "amount": "24900000000" } }
        });

        let s = swap(&record).expect("a swap");

        assert_eq!((s.pays, s.gets, s.fee(), s.closed()), (25_289_340_101, 24_900_000_000, 389_340_101, false));
    }

    #[test]
    fn a_take_is_read_only_when_its_id_is_a_transaction_id() {
        let offer = json!({ "btc": { "value": "2" }, "evm": { "amount": "1" } });
        let id = "ABCD1234ABCD1234ABCD1234ABCD1234ABCD1234ABCD1234ABCD1234ABCD1234";
        let records =
            [json!({ "offer": offer, "take": { "txid": id } }), json!({ "offer": offer, "take": { "txid": "$(rm -rf ~)" } }), json!({ "offer": offer })];

        let takes = records.each_ref().map(|r| swap(r).expect("a swap").take_txid);

        assert_eq!(takes, [id.to_ascii_lowercase(), String::new(), String::new()]);
    }

    #[test]
    fn only_a_plain_bech32_address_is_taken_from_the_log() {
        let line =
            |a: &str| format!(r#"{{"msg":"the reverse side is on: the TAC and the sats at this Bitcoin address are what it sells","reverseAddress":"{a}"}}"#);

        let read = ["bc1qtestreverse00000000000000000000000000", "bc1q`reboot`", "1BoatSLRHtKNngkdXEeobR76b53LETtpyT"]
            .map(|a| log_line(&line(a)).and_then(|e| e.signal));

        assert!(matches!(&read[0], Some(Signal::ReverseOn(a)) if a == "bc1qtestreverse00000000000000000000000000"));
        assert!(read[1].is_none() && read[2].is_none());
    }

    #[test]
    fn reverse_info_without_terms_is_a_failed_read() {
        let answers = [json!({ "minAmount": "1", "maxAmount": "9", "feeBps": 30, "available": "5", "open": 1, "maxOpen": 2 }), json!({ "available": "5" })];

        let read = answers.each_ref().map(reverse);

        assert!(matches!(&read[0], Some(ReverseSide::On { terms, open: Some(1), max_open: 2 }) if terms.available == Some(5)));
        assert_eq!(read[1], None);
    }

    #[test]
    fn a_reverse_record_reads_what_the_bridger_pays_in_the_pool_and_gets_on_bitcoin() {
        let record = json!({
            "role": "bl", "id": "cc33", "phase": "claiming", "created": 1795500342u64, "amount": "2000000000", "evmAmount": "2016000000",
            "lock": { "txid": "dd44" }, "evmOffer": { "deadline": 1795515000u64 }
        });

        let s = reverse_swap(&record).expect("a swap");

        assert_eq!(
            (s.direction, s.pays, s.gets, s.fee(), s.deadline, s.lock_txid.as_str()),
            (Direction::Reverse, 2_016_000_000, 2_000_000_000, 16_000_000, 1_795_515_000, "dd44")
        );
        assert!(!s.paid() && !s.closed());
    }

    #[test]
    fn a_reverse_record_without_both_amounts_is_not_a_swap_and_one_not_yet_offered_has_no_deadline() {
        let records = [json!({ "phase": "built", "amount": "5" }), json!({ "phase": "built", "amount": "5", "evmAmount": "6" })];

        let read = records.each_ref().map(reverse_swap);

        assert_eq!(read[0], None);
        assert_eq!(read[1].as_ref().map(|s| s.deadline), Some(0));
    }

    #[test]
    fn an_ads_reverse_terms_are_read_where_they_can_be() {
        let ads = [
            json!({ "minAmount": "1", "maxAmount": "9", "feeBps": 30, "reverse": { "minAmount": "2", "maxAmount": "8", "feeBps": 20, "available": "7" } }),
            json!({ "minAmount": "1", "maxAmount": "9", "feeBps": 30, "reverse": { "note": "soon" } }),
            json!({ "minAmount": "1", "maxAmount": "9", "feeBps": 30 }),
        ];

        let read = ads.each_ref().map(|a| ad(a).map(|a| (a.reverse, a.reverse_terms.map(|t| (t.fee_bps, t.available)))));

        assert_eq!(read, [Some((true, Some((20, Some(7))))), Some((true, None)), Some((false, None))]);
    }

    const STATUS: &str = "main address      bc1qtestmain000000000000000000000000000000   (the provider's earnings)
reverse address   bc1qtestreverse00000000000000000000000000   <- send plain sats here (an ordinary Bitcoin send)
reverse tacit1    tacit1qtest000000000000000000000000000000000000000000000000000000

TAC at it         2 confirmed notes, 20.12345678 TAC in all  (+1 not confirmed yet, or received privately and not moved yet: not counted)
              20 TAC   9a9a9a9a9a9a…:0   546 sats

plain sats        1 confirmed coin, 30000 sats in all
";

    #[test]
    fn the_status_tools_output_is_read_by_its_labels() {
        let out = STATUS;

        let stock = reverse_stock(out).expect("a stock");

        assert_eq!(stock, ReverseStock { address: "bc1qtestreverse00000000000000000000000000".into(), notes: 2, tac: 2_012_345_678, sats: Some(30_000) });
    }

    #[test]
    fn a_status_run_that_stopped_part_way_or_lost_its_explorer_is_read_for_what_it_says() {
        let cut = STATUS.split("TAC at it").next().expect("the head");
        let no_sats = STATUS.replace("1 confirmed coin, 30000 sats in all", "could not be read from any explorer");

        let (cut, no_sats) = (reverse_stock(cut), reverse_stock(&no_sats));

        assert_eq!(cut, None);
        assert_eq!(no_sats.map(|s| (s.tac, s.sats)), Some((2_012_345_678, None)));
    }

    #[test]
    fn a_tac_figure_with_more_places_than_tac_has_is_not_rounded_into_one() {
        let amounts = ["20", "0.5", "1.000000001", "1,000", "-1", ""];

        let read = amounts.map(decimal_tac);

        assert_eq!(read, [Some(20 * UNIT), Some(UNIT / 2), None, None, None, None]);
    }

    #[test]
    fn record_whose_offer_lacks_an_amount_is_not_counted_as_a_swap_of_zero() {
        let record = json!({ "id": "aa11", "phase": "locking", "offer": { "btc": {}, "evm": { "amount": "24900000000" } } });

        let s = swap(&record);

        assert_eq!(s, None);
    }

    #[test]
    fn a_long_line_is_kept_short_and_cut_on_a_character() {
        let raw = "é".repeat(MAX_KEPT);

        let e = log_line(&raw).expect("an entry");

        assert_eq!((e.msg.len(), e.msg.chars().last()), (MAX_KEPT + '…'.len_utf8(), Some('…')));
    }

    #[test]
    fn an_amount_that_cannot_be_read_is_shown_as_written_not_as_zero() {
        let raw = r#"{"msg":"low","free":"lots"}"#;

        let e = log_line(raw).expect("an entry");

        assert_eq!(e.detail, "free lots");
    }

    #[test]
    fn record_without_an_offer_is_not_a_swap() {
        let record = json!({ "id": "aa11", "phase": "locking" });

        let s = swap(&record);

        assert_eq!(s, None);
    }

    #[test]
    fn want_without_a_cap_takes_any_fee() {
        let posted = json!({ "amount": "200000000000", "until": 1795580009u64 });

        let w = want(&posted);

        assert_eq!((w.amount, w.max_fee_bps), (200_000_000_000, None));
    }

    #[test]
    fn capacity_line_carries_its_numbers_as_a_signal() {
        let raw = r#"{"ts":"2026-11-24T05:51:50.890Z","level":"info","msg":"capacity","held":"24900000000","promised":"24900000000","open":1,"holds":1}"#;

        let e = log_line(raw).expect("an entry");

        assert_eq!((e.time.as_str(), e.at), ("05:51:50", Some(1_795_499_510)));
        assert!(matches!(e.signal, Some(Signal::Capacity(Capacity { held: 24_900_000_000, promised: 24_900_000_000, open: 1, holds: 1 }))));
        assert!(e.detail.contains("held 249.00"));
    }

    #[test]
    fn answer_without_terms_is_not_read_as_zero_terms() {
        let answers = [json!({}), json!({ "error": "not found" }), json!({ "minAmount": "1", "maxAmount": "2" })];

        let read = answers.each_ref().map(terms);

        assert_eq!(read, [None, None, None]);
    }

    #[test]
    fn an_asset_reads_the_same_with_or_without_its_prefix() {
        let id = "f0bbe868af10c6c67652a99709bf32048d1aa7194efe3e9a1ef1bde43f94762b";
        let v = json!({ "bare": id, "prefixed": format!("0x{id}"), "upper": id.to_uppercase(), "short": "f0bb", "number": 7 });

        let read = ["bare", "prefixed", "upper", "short", "number", "missing"].map(|k| asset(&v, k));

        assert!(read[0].is_some_and(|a| a.0[0] == 0xf0 && a.0[31] == 0x2b));
        assert_eq!((read[1], read[2]), (read[0], read[0]));
        assert_eq!(read[3..], [None, None, None]);
    }

    #[test]
    fn ad_without_terms_is_left_out() {
        let posted = json!({ "name": "cheap", "claimPub": "02aa" });

        let a = ad(&posted);

        assert_eq!(a, None);
    }

    #[test]
    fn fee_cap_that_cannot_be_read_is_the_strictest_cap() {
        let posted = [json!({ "amount": "5", "maxFeeBps": "10" }), json!({ "amount": "5", "maxFeeBps": -1 }), json!({ "amount": "5", "maxFeeBps": 10.5 })];

        let caps = posted.each_ref().map(|p| want(p).max_fee_bps);

        assert_eq!(caps, [Some(10), Some(0), Some(0)]);
    }

    #[test]
    fn numbers_too_large_to_hold_are_not_cut_to_their_low_bits() {
        let posted = json!({ "minAmount": "1", "maxAmount": "2", "feeBps": "18446744073709551617" });

        let t = terms(&posted).expect("terms");

        assert_eq!(t.fee_bps, u64::MAX);
    }

    #[test]
    fn capacity_line_without_its_amounts_carries_no_signal() {
        let raw = r#"{"ts":"2026-11-24T05:51:50.890Z","level":"info","msg":"capacity","held":"lots"}"#;

        let e = log_line(raw).expect("an entry");

        assert!(e.signal.is_none());
    }

    #[test]
    fn line_that_is_not_json_is_shown_as_it_is() {
        let raw = "  Error: connect ECONNREFUSED  ";

        let e = log_line(raw).expect("an entry");

        assert_eq!((e.msg.as_str(), e.level), ("Error: connect ECONNREFUSED", Level::Info));
    }

    #[test]
    fn a_record_names_its_transactions_and_one_that_is_not_an_id_is_left_out() {
        let id = |c: &str| c.repeat(64);
        let forward = json!({"id": "f", "take": {"commitTxid": "$(reboot)", "txid": id("C")},
            "offer": {"btc": {"value": "2", "txid": id("a")}, "evm": {"amount": "1"}}});
        let back = json!({"id": "r", "amount": "5", "evmAmount": "6", "lock": {"commitTxid": id("d"), "txid": id("e")}, "refund": {"txid": id("f")}});

        let (f, r) = (swap(&forward).expect("a swap"), reverse_swap(&back).expect("a swap"));

        assert_eq!(f.linked, [(Role::BridgerLock, id("a")), (Role::Take, id("c"))]);
        assert_eq!(r.linked, [(Role::LockCommit, id("d")), (Role::Lock, id("e")), (Role::Refund, id("f"))]);
    }

    #[test]
    fn a_forward_record_names_its_pool_leaf_bare_and_an_address_is_an_address_or_nothing() {
        let leaf = "AB".repeat(32);
        let rec = json!({"id": "f", "lock": {"lockLeaf": format!("0x{leaf}")}, "offer": {"btc": {"value": "2"}, "evm": {"amount": "1"}}});
        let none = json!({"id": "f", "lock": {"lockLeaf": "0x12"}, "offer": {"btc": {"value": "2"}, "evm": {"amount": "1"}}});

        let (named, unnamed) = (swap(&rec).expect("a swap").pool_leaf, swap(&none).expect("a swap").pool_leaf);
        let addresses = [eth_address(&format!("0x{}", "Ab".repeat(20))), eth_address(&"ab".repeat(20)), eth_address("0x\"}],\"x\":[{\"")];

        assert_eq!((named, unnamed.as_str()), (leaf.to_lowercase(), ""));
        assert_eq!(addresses, [Some(format!("0x{}", "ab".repeat(20))), None, None]);
    }

    #[test]
    fn a_dry_run_says_how_much_waits_or_that_none_is_this_keys_and_anything_else_is_no_verdict() {
        let outs = [
            "found 1 private note from 5f0d…, confirmed: 200.5 TAC (5f0d…:0)\nwould move 200.5 TAC in one transfer to bc1q… (the reverse address, as a plain note)\n",
            "no TAC from that transaction was found for this key: is it the transaction that paid the reverse tacit1 address, and has it confirmed?",
            "the TAC from that transaction is already a plain note at the reverse address: there is nothing to move",
            "the TAC from that transaction has not confirmed yet: run this again once it has",
            "would move lots TAC",
        ];

        let verdicts = outs.map(silent_verdict);

        assert_eq!(verdicts, [Some(Some(200 * UNIT + UNIT / 2)), Some(None), Some(None), None, None]);
    }
}
