//! The provider's and the board's JSON, read into the model. None of it is a stable interface, so every
//! reader is lenient: a missing field is zero or empty, and a log line that is not recognised is still shown.

pub use crate::monitors::common::wire::{eth_address, health};
use crate::monitors::{
    btc_eth::model::{
        Ad, Books, Buyback, Capacity, Direction, Fee, Identity, Info, Level, LogEntry, Notes, ReverseSide, Role, Said, Signal, Stock, Swap, Target, Terms, eth,
        sats,
    },
    common::wire::{asset, btc_address, field, kept, log_detail, log_head, small, text, txid, units, word32},
};
use serde_json::Value;

/// `/info`, and the same fields in a board ad. `None` when the size range or the fee is not there to read:
/// terms of all zeros would be drawn as real ones.
fn terms(v: &Value) -> Option<Terms> {
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

/// A whole number that may be under zero, as a string or a number.
fn signed(v: &Value) -> Option<i128> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_i64().map(i128::from),
        _ => None,
    }
}

fn part<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|p| p.is_object())
}

/// What the provider's upkeep adds to `/info`. `None` until it has made its first look: there are no books yet,
/// which is not books of zeros.
fn books(v: &Value) -> Option<Books> {
    let s = part(v, "stats")?;
    Some(Books {
        since: small(s, "since"),
        swaps: small(s, "swaps"),
        forward: small(s, "swapsForward"),
        reverse: small(s, "swapsReverse"),
        refunds: small(s, "refunds"),
        fees_sats: field(s, "feesEarnedSats"),
        fees_eth: field(s, "feesEarnedEth"),
        cost_sats: field(s, "networkCostSats"),
        cost_eth: field(s, "networkCostEth"),
        loss_sats: field(s, "lossesSats"),
        loss_eth: field(s, "lossesEth"),
        net_eth: s.get("netEth").and_then(signed),
        reinvested_eth: field(s, "reinvestedEth"),
        owed_eth: field(s, "owedEth"),
        // a look without its ETH or its worth is not a look to draw
        stock: part(s, "capacity").and_then(|c| {
            Some(Stock {
                eth: c.get("eth").and_then(units)?,
                sats: c.get("sats").and_then(units),
                value_eth: c.get("valueEth").and_then(units)?,
                at: small(c, "at"),
                first_value_eth: field(c, "firstValueEth"),
                first_at: small(c, "firstAt"),
            })
        }),
        notes: part(s, "notes").and_then(|n| {
            Some(Notes {
                count: u64::try_from(n.get("notes").and_then(units)?).unwrap_or(u64::MAX),
                total: n.get("total").and_then(units)?,
                usable: n.get("usable").and_then(units)?,
                dust: small(n, "dust"),
                largest: field(n, "largest"),
            })
        }),
        target: part(v, "target").and_then(|t| {
            Some(Target {
                eth: t.get("eth").and_then(units)?,
                btc_as_eth: field(t, "btcAsEth"),
                reached: t.get("reached").and_then(Value::as_bool).unwrap_or(false),
            })
        }),
        lean_bps: v.get("skewBps").and_then(Value::as_i64),
        buyback: part(v, "buyback").map(|b| Buyback {
            on: b.get("on").and_then(Value::as_bool).unwrap_or(false),
            routed_eth: field(b, "routedEth"),
            sends: small(b, "sends"),
        }),
    })
}

/// `/info`. `None` for an answer without terms: it is a failed read.
pub fn info(v: &Value) -> Option<Info> {
    Some(Info { terms: terms(v)?, books: books(v) })
}

/// `/reverse/info`. `None` for an answer without terms: it is a failed read, not a side with nothing to sell.
pub fn reverse(v: &Value) -> Option<ReverseSide> {
    Some(ReverseSide::On { terms: terms(v)?, open: v.get("open").and_then(units).map(|n| u64::try_from(n).unwrap_or(u64::MAX)), max_open: small(v, "maxOpen") })
}

/// What a record says of the transaction it keeps under `inside`: its id, and whatever of its fee, size and time
/// is there to read. A figure that is not a whole number is left out, not read as zero.
fn said(v: &Value, inside: &str) -> Option<(String, Said)> {
    let t = v.get(inside)?;
    let n = |key: &str| t.get(key).and_then(Value::as_u64);
    Some((txid(t, "txid")?, Said { fee: n("fee"), vsize: n("vsize"), at: n("at"), bumps: n("bumps").unwrap_or(0) }))
}

/// A record in the provider's `swaps/`: the user's sats for the provider's pool ETH. `None` for a file that is not
/// a swap (no offer in it), or an offer without both amounts: zeros would understate what is at stake.
pub fn swap(v: &Value) -> Option<Swap> {
    let offer = v.get("offer")?;
    let lock = txid(v, "lockTxid").or_else(|| txid(offer.get("btc")?, "txid"));
    let take = v.get("take").and_then(|t| txid(t, "txid")).or_else(|| txid(v, "takenBy"));
    Some(Swap {
        direction: Direction::Forward,
        id: text(v, "id"),
        phase: text(v, "phase"),
        created: small(v, "created"),
        deadline: small(v, "deadline"),
        sats: offer.get("btc")?.get("value").and_then(units)?,
        eth: offer.get("evm")?.get("amount").and_then(units)?,
        // the lock is named twice, in the offer and once the provider has seen it: the same transaction
        lock_txid: lock.clone().unwrap_or_default(),
        // a take the record does not name itself is known by what spent the lock
        linked: lock.map(|id| (Role::UserLock, id)).into_iter().chain(take.map(|id| (Role::Take, id))).collect(),
        said: said(v, "take").into_iter().collect(),
        lock_height: v.get("lockHeight").and_then(Value::as_u64),
        pool_leaf: v.get("lock").and_then(|l| word32(l.get("lockLeaf")?.as_str()?)).unwrap_or_default(),
    })
}

/// A record in the provider's `reverse/swaps/`: the provider locks sats (`amount`) for the user's pool ETH
/// (`evmAmount`). `None` without both amounts, as forward.
pub fn reverse_swap(v: &Value) -> Option<Swap> {
    Some(Swap {
        direction: Direction::Reverse,
        id: text(v, "id"),
        phase: text(v, "phase"),
        created: small(v, "created"),
        // known once the user has offered its pool lock
        deadline: v.get("evmOffer").map_or(0, |o| small(o, "deadline")),
        sats: v.get("amount").and_then(units)?,
        eth: v.get("evmAmount").and_then(units)?,
        lock_txid: v.get("lock").and_then(|l| txid(l, "txid")).unwrap_or_default(),
        linked: [
            (Role::LockCommit, "lock", "commitTxid"),
            (Role::Lock, "lock", "txid"),
            (Role::Refund, "refund", "txid"),
            (Role::TakeRefund, "takeRefund", "txid"),
        ]
        .into_iter()
        .filter_map(|(role, inside, key)| Some((role, txid(v.get(inside)?, key)?)))
        .collect(),
        said: ["lock", "refund", "takeRefund"].into_iter().filter_map(|inside| said(v, inside)).collect(),
        lock_height: None,
        // in reverse the pool lock is the user's, and the record does not name its leaf
        pool_leaf: String::new(),
    })
}

/// `None` for an ad without readable terms: it is left out of the market, not listed at a fee of zero.
pub fn ad(v: &Value) -> Option<Ad> {
    Some(Ad {
        name: text(v, "name"),
        claim_pub: text(v, "claimPub"),
        terms: terms(v)?,
        reverse: v.get("reverse").is_some_and(|r| !r.is_null() && r != &Value::Bool(false)),
        reverse_terms: v.get("reverse").and_then(terms),
    })
}

/// How an amount under a log line's key is written: pool ETH and sats where the provider logs them under these names.
fn amount(key: &str, units: u128) -> Option<String> {
    match key {
        "free" | "held" | "promised" | "feeEth" | "costEth" => Some(eth(units)),
        "feeSats" | "costSats" => Some(sats(units)),
        _ => None,
    }
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
        "pair provider: Bitcoin for ETH" => {
            Some(Signal::StartPrice(v.get("price").and_then(|p| p.get("ethPerBtc")).and_then(Value::as_f64).filter(|p| p.is_finite() && *p > 0.0)))
        }
        "the provider has its price again" => Some(Signal::Priced(true)),
        // only a finished swap's line with a fee that reads is a fee: one that does not is no figure, not zero
        "stats: a swap is counted" => match text(&v, "dir").as_str() {
            "fwd" => v.get("feeSats").and_then(units).map(Fee::Sats),
            "rev" => v.get("feeEth").and_then(units).map(Fee::Eth),
            _ => None,
        }
        .filter(|_| !text(&v, "id").is_empty())
        .map(|fee| Signal::Counted { id: text(&v, "id"), fee }),
        m if m.starts_with("the reverse side is on") => btc_address(&text(&v, "reverseAddress")).map(Signal::ReverseOn),
        m if m.starts_with("the provider has no price") => Some(Signal::Priced(false)),
        _ => None,
    };
    let (time, at, level) = log_head(&v);
    Some(LogEntry { time, at, level, detail: kept(log_detail(&msg, &v, amount)), msg: kept(msg), signal })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_forward_record_reads_the_users_sats_and_the_providers_eth_and_one_without_both_is_not_a_swap() {
        let id = "ab".repeat(32);
        let record = json!({ "id": "s1", "phase": "taking", "created": 100, "deadline": 900, "lockTxid": id,
            "offer": { "btc": { "value": "100000", "txid": id }, "evm": { "amount": "3100000" } } });
        let half = json!({ "id": "s2", "phase": "accepted", "offer": { "btc": { "value": "100000" }, "evm": {} } });

        let (read, not_one, no_offer) = (swap(&record), swap(&half), swap(&json!({ "id": "s3" })));

        let s = read.expect("a swap");
        assert_eq!((s.direction, s.sats, s.eth, s.deadline, s.lock_txid), (Direction::Forward, 100_000, 3_100_000, 900, id));
        assert!(not_one.is_none() && no_offer.is_none());
    }

    #[test]
    fn a_reverse_record_reads_what_the_provider_locks_and_has_no_deadline_until_the_user_offers() {
        let built = json!({ "id": "r1", "phase": "built", "amount": "200000", "evmAmount": "6250000", "lock": { "txid": "not an id" } });
        let offered = json!({ "id": "r1", "phase": "offered", "amount": "200000", "evmAmount": "6250000", "evmOffer": { "deadline": 777 } });

        let (built, offered, half) = (reverse_swap(&built), reverse_swap(&offered), reverse_swap(&json!({ "id": "r2", "amount": "1" })));

        let b = built.expect("a swap");
        assert_eq!((b.direction, b.sats, b.eth, b.deadline, b.lock_txid.as_str()), (Direction::Reverse, 200_000, 6_250_000, 0, ""));
        assert_eq!(offered.map(|s| s.deadline), Some(777));
        assert!(half.is_none());
    }

    #[test]
    fn a_record_names_its_transactions_in_the_order_they_happen_and_one_that_is_not_an_id_is_left_out() {
        let id = |c: &str| c.repeat(64);
        let offer = json!({ "btc": { "value": "100000", "txid": id("a") }, "evm": { "amount": "3100000" } });
        let taking = json!({ "id": "s1", "phase": "taking", "offer": offer, "lockHeight": 977_636, "lock": { "lockLeaf": format!("0x{}", id("C")) },
            "take": { "txid": id("b"), "commitTxid": null, "at": 1_000, "feeRate": 3, "fee": 500, "vsize": 99, "bumps": 1, "tip": 977_636 } });
        // a lock someone else spent is named by what spent it, and a take that is not an id is not a transaction
        let taken = json!({ "id": "s2", "phase": "done", "offer": offer, "take": { "txid": "$(reboot)" }, "takenBy": id("c") });
        let back = json!({ "id": "r1", "phase": "refunded", "amount": "200000", "evmAmount": "6250000",
            "lock": { "commitTxid": id("d"), "txid": id("e") }, "refund": { "txid": id("f") }, "takeRefund": { "txid": "none yet" } });

        // the take as the provider wrote it: 500 sats over 99 virtual bytes, sent at 1,000 and bumped once
        let told = swap(&taking).map(|s| (s.said, s.lock_height));
        assert_eq!(swap(&taking).map(|s| s.pool_leaf), Some(id("c")));
        let read = [swap(&taking), swap(&taken), reverse_swap(&back)].map(|s| s.expect("a swap").linked);

        let take = Said { fee: Some(500), vsize: Some(99), at: Some(1_000), bumps: 1 };
        assert_eq!((told, take.rate_milli()), (Some((vec![(id("b"), take)], Some(977_636))), Some(5_050)));
        assert_eq!(read[0], [(Role::UserLock, id("a")), (Role::Take, id("b"))]);
        assert_eq!(read[1], [(Role::UserLock, id("a")), (Role::Take, id("c"))]);
        assert_eq!(read[2], [(Role::LockCommit, id("d")), (Role::Lock, id("e")), (Role::Refund, id("f"))]);
    }

    #[test]
    fn info_without_terms_is_a_failed_read_and_without_stats_has_no_books() {
        let plain = json!({ "minAmount": "50000", "maxAmount": "100000", "feeBps": 30, "flatFee": "1000", "available": null });

        let (no_terms, no_books) = (info(&json!({ "stats": { "swaps": 3 } })), info(&plain));

        assert!(no_terms.is_none());
        assert_eq!(no_books.map(|i| (i.terms.fee_bps, i.terms.available, i.books)), Some((30, None, None)));
    }

    #[test]
    fn the_books_are_read_as_the_upkeep_writes_them() {
        let answer = json!({ "minAmount": "50000", "maxAmount": "100000", "feeBps": 30, "skewBps": -6,
            "stats": { "since": 1000, "swaps": 5, "swapsForward": 3, "swapsReverse": 2, "refunds": 1, "feesEarnedSats": "900", "feesEarnedEth": "28000",
                "networkCostSats": "400", "networkCostEth": "0", "lossesSats": "0", "lossesEth": "0", "netEth": "-1200", "reinvestedEth": "0", "owedEth": "0",
                "capacity": { "eth": "50000000", "sats": "1000000", "valueEth": "81000000", "at": 2000, "firstValueEth": "80000000", "firstAt": 1000 },
                "notes": { "notes": 3, "total": "50000000", "usable": "50000000", "dust": 1, "largest": "30000000" } },
            "target": { "eth": "1000000000", "btcAsEth": "1000000000", "sats": null, "reached": false },
            "buyback": { "on": false, "routedEth": "0", "sends": 0 } });

        let b = info(&answer).and_then(|i| i.books).expect("books");

        assert_eq!(
            (b.swaps, b.forward, b.reverse, b.refunds, b.fees_sats, b.fees_eth, b.net_eth, b.lean_bps),
            (5, 3, 2, 1, 900, 28_000, Some(-1200), Some(-6))
        );
        assert_eq!(b.stock.as_ref().and_then(Stock::price), Some(31.0));
        assert_eq!(b.notes.map(|n| (n.count, n.usable, n.dust)), Some((3, 50_000_000, 1)));
        assert_eq!(b.target.map(|t| (t.eth, t.reached)), Some((1_000_000_000, false)));
        assert_eq!(b.buyback.map(|x| x.on), Some(false));
    }

    #[test]
    fn a_look_that_lacks_its_figures_is_left_out_of_the_books_and_not_read_as_zero() {
        let answer =
            json!({ "minAmount": "1", "maxAmount": "2", "feeBps": 1, "stats": { "capacity": { "sats": "5" }, "notes": { "notes": 2 }, "netEth": "lots" } });

        let b = info(&answer).and_then(|i| i.books).expect("books");

        assert_eq!((b.stock, b.notes, b.net_eth, b.target, b.lean_bps), (None, None, None, None, None));
    }

    #[test]
    fn the_log_says_who_the_provider_is_what_it_holds_and_whether_it_has_a_price() {
        let line = |v: Value| log_line(&v.to_string()).and_then(|e| e.signal);
        let address = "bc1qexampleaddress000000000000000000000000";

        let read = [
            line(json!({ "msg": "pair provider: Bitcoin for ETH", "price": { "ethPerBtc": 31.02, "at": 5, "n": 3 } })),
            line(json!({ "msg": "pair provider: Bitcoin for ETH", "price": null })),
            line(
                json!({ "level": "alert", "msg": "the provider has no price for Bitcoin against ETH: its sources are down or disagree, so it quotes nothing" }),
            ),
            line(json!({ "msg": "the provider has its price again" })),
            line(json!({ "msg": "capacity", "held": "7000000", "promised": "3100000", "open": 1, "holds": 0 })),
            line(json!({ "msg": "the reverse side is on: the TAC and the sats at this Bitcoin address are what it sells", "reverseAddress": address })),
            line(json!({ "msg": "the reverse side is on", "reverseAddress": "bc1q; reboot" })),
        ];

        assert_eq!(
            read,
            [
                Some(Signal::StartPrice(Some(31.02))),
                Some(Signal::StartPrice(None)),
                Some(Signal::Priced(false)),
                Some(Signal::Priced(true)),
                Some(Signal::Capacity(Capacity { held: 7_000_000, promised: 3_100_000, open: 1, holds: 0 })),
                Some(Signal::ReverseOn(address.into())),
                None,
            ]
        );
    }

    #[test]
    fn a_counted_swap_carries_its_fee_in_its_directions_unit_and_one_without_a_fee_carries_none() {
        let line = |v: Value| log_line(&v.to_string()).expect("a line");

        let (fwd, rev, bare) = (
            line(json!({ "msg": "stats: a swap is counted", "id": "s1", "dir": "fwd", "feeSats": "1300", "costSats": "250" })),
            line(json!({ "msg": "stats: a swap is counted", "id": "r1", "dir": "rev", "feeEth": "9400" })),
            line(json!({ "msg": "stats: a swap is counted", "id": "s2", "dir": "fwd" })),
        );

        assert_eq!(fwd.signal, Some(Signal::Counted { id: "s1".into(), fee: Fee::Sats(1300) }));
        assert_eq!(fwd.detail, "costSats 250  dir fwd  feeSats 1,300  id s1");
        assert_eq!(rev.signal, Some(Signal::Counted { id: "r1".into(), fee: Fee::Eth(9400) }));
        assert_eq!(bare.signal, None);
    }

    #[test]
    fn a_line_that_is_not_json_is_shown_as_it_is_and_a_blank_one_is_not_a_line() {
        let (text, blank) = (log_line("Error: something broke"), log_line("   "));

        assert_eq!(text.map(|e| (e.msg, e.level)), Some(("Error: something broke".to_string(), Level::Info)));
        assert!(blank.is_none());
    }

    #[test]
    fn an_ad_reads_both_members_terms_and_one_without_terms_is_left_out() {
        let listed = json!({ "name": "north", "claimPub": "02aa", "minAmount": "50000", "maxAmount": "500000", "feeBps": 30, "flatFee": "1000", "available": "400000",
            "reverse": { "minAmount": "1000000", "maxAmount": "9000000", "feeBps": 40, "flatFee": "20000" } });

        let (read, bare) = (ad(&listed), ad(&json!({ "name": "nobody", "claimPub": "02bb" })));

        let a = read.expect("an ad");
        assert_eq!((a.terms.fee_bps, a.reverse, a.reverse_terms.map(|t| (t.fee_bps, t.max))), (30, true, Some((40, 9_000_000))));
        assert!(bare.is_none());
    }
}
