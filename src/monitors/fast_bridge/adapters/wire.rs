//! The provider's and the board's JSON, read into the model. None of it is a stable interface, so every
//! reader is lenient: a missing field is zero or empty, and a log line that is not recognised is still shown.

use crate::{
    fmt::short,
    monitors::fast_bridge::model::{Ad, Asset, Capacity, Health, Identity, Level, LogEntry, Signal, Swap, Terms, Want, tac},
};
use serde_json::Value;

/// A whole number the provider writes as a string ("24900000000") or a number.
fn units(v: &Value) -> Option<u128> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64().map(u128::from),
        _ => None,
    }
}

fn field(v: &Value, key: &str) -> u128 {
    v.get(key).and_then(units).unwrap_or(0)
}

/// A count or a time: one too large to hold is the largest there is, never its low bits.
fn small(v: &Value, key: &str) -> u64 {
    u64::try_from(field(v, key)).unwrap_or(u64::MAX)
}

/// An asset id: 64 hex digits, with or without "0x". The provider writes it both ways.
fn asset(v: &Value, key: &str) -> Option<Asset> {
    let hex = v.get(key)?.as_str()?;
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut id = [0u8; 32];
    for (i, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(Asset(id))
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
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

pub fn health(v: &Value) -> Health {
    Health { ok: v.get("ok").and_then(Value::as_bool).unwrap_or(false), error: text(v, "error") }
}

/// A record in the provider's `swaps/`. `None` for a file that is not a swap (no offer in it).
pub fn swap(v: &Value) -> Option<Swap> {
    let offer = v.get("offer")?;
    Some(Swap {
        id: text(v, "id"),
        phase: text(v, "phase"),
        created: small(v, "created"),
        deadline: small(v, "deadline"),
        // an offer without both amounts is not a swap to count: zeros would understate what is at stake
        pays: offer.get("btc")?.get("value").and_then(units)?,
        gets: offer.get("evm")?.get("amount").and_then(units)?,
        lock_txid: text(v, "lockTxid"),
    })
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
    })
}

/// "2026-11-24T05:51:50.890Z" as seconds since 1970. UTC only, which is what the provider writes.
fn epoch(ts: &str) -> Option<u64> {
    let n = |r: std::ops::Range<usize>| ts.get(r)?.parse::<u64>().ok();
    let (y, m, d, h, mi, s) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    if y < 1970 || !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    // days from the civil date, counting years from March so the leap day falls last
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let days = y * 365 + y / 4 - y / 100 + y / 400 + (m * 153 + 2) / 5 + d - 1;
    Some((days.checked_sub(719_468)?) * 86_400 + h * 3600 + mi * 60 + s)
}

/// The most of one line that is kept for the screen. Thousands of lines are held, so this is far under what is read.
const MAX_KEPT: usize = 2048;

/// Cut at `MAX_KEPT` bytes, on a character boundary, with a mark that it was cut.
fn kept(mut s: String) -> String {
    if s.len() > MAX_KEPT {
        let mut end = MAX_KEPT;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push('…');
    }
    s
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
        _ => None,
    };
    let level = match text(&v, "level").as_str() {
        "warn" => Level::Warn,
        "alert" | "error" => Level::Alert,
        _ => Level::Info,
    };
    // "2026-11-24T05:51:50.890Z" -> "05:51:50"
    let time = v.get("ts").and_then(Value::as_str).and_then(|t| t.get(11..19)).unwrap_or("--:--:--").to_string();
    let at = v.get("ts").and_then(Value::as_str).and_then(epoch);
    Some(LogEntry { time, at, level, detail: kept(detail(&msg, &v)), msg: kept(msg), signal })
}

/// The rest of a log line as "key value" pairs, with the amounts and the long hex made readable.
fn detail(msg: &str, v: &Value) -> String {
    if msg == "swap" {
        return format!("{}  {} → {}", short(&text(v, "id"), 8, 4), text(v, "from"), text(v, "to"));
    }
    let Some(obj) = v.as_object() else { return String::new() };
    let mut parts = Vec::new();
    for (k, val) in obj {
        if matches!(k.as_str(), "ts" | "level" | "msg") {
            continue;
        }
        let s = match val {
            Value::Null | Value::Object(_) => continue,
            Value::Array(a) => format!("[{}]", a.len()),
            // TAC base units wherever the provider logs them under these names
            _ if matches!(k.as_str(), "free" | "held" | "promised") && units(val).is_some() => tac(units(val).unwrap_or(0)),
            Value::String(s) if s.len() > 24 && s.chars().all(|c| c.is_ascii_hexdigit() || c == 'x') => short(s, 8, 6),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        parts.push(format!("{k} {s}"));
    }
    parts.join("  ")
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
