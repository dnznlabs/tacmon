//! The provider's and the board's JSON, read into the model. None of it is a stable interface, so every
//! reader is lenient: a missing field is zero or empty, and a log line that is not recognised is still shown.

use crate::{
    fmt::short,
    monitors::fast_bridge::model::{Ad, Capacity, Health, Identity, Level, LogEntry, Signal, Swap, Terms, Want, tac},
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

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// `/info`, and the same fields in a board ad.
pub fn terms(v: &Value) -> Terms {
    Terms {
        min: field(v, "minAmount"),
        max: field(v, "maxAmount"),
        fee_bps: field(v, "feeBps") as u64,
        flat: field(v, "flatFee"),
        available: v.get("available").and_then(units),
    }
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
        created: field(v, "created") as u64,
        deadline: field(v, "deadline") as u64,
        pays: field(offer.get("btc")?, "value"),
        gets: field(offer.get("evm")?, "amount"),
        lock_txid: text(v, "lockTxid"),
    })
}

pub fn want(v: &Value) -> Want {
    Want { amount: field(v, "amount"), max_fee_bps: v.get("maxFeeBps").and_then(Value::as_u64), until: field(v, "until") as u64 }
}

pub fn ad(v: &Value) -> Ad {
    Ad {
        name: text(v, "name"),
        claim_pub: text(v, "claimPub"),
        terms: terms(v),
        any_tac: v.get("anyTac").and_then(Value::as_bool).unwrap_or(false),
        reverse: v.get("reverse").is_some_and(|r| !r.is_null() && r != &Value::Bool(false)),
    }
}

/// One line of the provider's log. `None` only for a blank line: anything that is not its JSON is shown as it is.
pub fn log_line(raw: &str) -> Option<LogEntry> {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return (!raw.trim().is_empty()).then(|| LogEntry::plain(Level::Info, raw.trim()));
    };
    let msg = text(&v, "msg");
    let signal = match msg.as_str() {
        "fast bridge LP" => Some(Signal::Started(Identity {
            bitcoin_address: text(&v, "bitcoinAddress"),
            claim_pub: text(&v, "claimPub"),
            pool_account: text(&v, "poolAccount"),
            dry_run: v.get("dryRun").and_then(Value::as_bool).unwrap_or(false),
        })),
        "capacity" => Some(Signal::Capacity(Capacity {
            held: field(&v, "held"),
            promised: field(&v, "promised"),
            open: field(&v, "open") as u64,
            holds: field(&v, "holds") as u64,
        })),
        _ => None,
    };
    let level = match text(&v, "level").as_str() {
        "warn" => Level::Warn,
        "alert" | "error" => Level::Alert,
        _ => Level::Info,
    };
    // "2026-11-24T05:51:50.890Z" -> "05:51:50"
    let time = v.get("ts").and_then(Value::as_str).and_then(|t| t.get(11..19)).unwrap_or("--:--:--").to_string();
    Some(LogEntry { time, level, detail: detail(&msg, &v), msg, signal })
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
            _ if matches!(k.as_str(), "free" | "held" | "promised") => tac(units(val).unwrap_or(0)),
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

        assert_eq!(e.time, "05:51:50");
        assert!(matches!(e.signal, Some(Signal::Capacity(Capacity { held: 24_900_000_000, promised: 24_900_000_000, open: 1, holds: 1 }))));
        assert!(e.detail.contains("held 249.00"));
    }

    #[test]
    fn line_that_is_not_json_is_shown_as_it_is() {
        let raw = "  Error: connect ECONNREFUSED  ";

        let e = log_line(raw).expect("an entry");

        assert_eq!((e.msg.as_str(), e.level), ("Error: connect ECONNREFUSED", Level::Info));
    }
}
