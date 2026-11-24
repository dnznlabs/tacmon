# tacmon security review, 2026-11-24

## 1. What this is

This is a self-review by tacmon's own developers using language models. It is not an audit by an outside firm. A set
of checks with one right answer ran, and then one adversarial review leg (`claude-opus`) read the scope. **The run had
one leg, so nothing in this document was independently confirmed.**

## 2. Provenance

```
audit date        2026-11-24
code commit       a334085c79bbb1c79f178315e8166cc7aa0db67a  (scope dirty: yes · tree dirty: yes)
toolchain         rustc 1.99.1 (58cf8f9c3 2026-10-13)
scope             SECURITY.md
                  Cargo.toml
                  deny.toml
                  src/config.rs
                  src/fmt.rs
                  src/main.rs
                  src/monitors/fast_bridge/adapters/http.rs
                  src/monitors/fast_bridge/adapters/local.rs
                  src/monitors/fast_bridge/adapters/memory.rs
                  src/monitors/fast_bridge/adapters/mod.rs
                  src/monitors/fast_bridge/adapters/wire.rs
                  src/monitors/fast_bridge/advisor.rs
                  src/monitors/fast_bridge/config.rs
                  src/monitors/fast_bridge/feed.rs
                  src/monitors/fast_bridge/model.rs
                  src/monitors/fast_bridge/mod.rs
                  src/monitors/fast_bridge/ports.rs
                  src/monitors/fast_bridge/state.rs
                  src/monitors/fast_bridge/ui.rs
                  src/monitors/mod.rs
                  src/shell/mod.rs
                  src/shell/monitor.rs
                  src/shell/splash.rs
                  src/shell/theme.rs
                  src/shell/widgets.rs
checks            9 passed · 0 failed
independence      ONE LEG — nothing in this run was independently confirmed
leg 1             claude 2.1.331 (Claude Code) · requested opus → observed claude-opus-5-5 · 477s wall · tokens 2605085 · 25 of 25 scope files named in the report · tree unchanged
```

## 3. Checks

From [`checks/SUMMARY.txt`](checks/SUMMARY.txt).

| Result | Check | Line |
|---|---|---|
| PASS | lockfile | Cargo.lock is committed and matches Cargo.toml |
| PASS | advisories | no RustSec advisory, yanked or unmaintained crate in the tree |
| PASS | licences | every dependency's licence is on deny.toml's list |
| PASS | bans | no banned crate, no wildcard version |
| PASS | sources | every dependency comes from crates.io |
| PASS | unsafe | every crate root carries `#![forbid(unsafe_code)]` |
| PASS | clippy | clippy is clean with warnings denied |
| PASS | tests | the tests pass |
| PASS | scope | every tracked .rs file is in audits/SCOPE or excluded there with a reason |
| REPORT | panic-surface | 83 lint hits where a panic or a silent wrap could start ([`panic-surface.txt`](checks/panic-surface.txt)) |

No check failed, so no check contributes a finding. `panic-surface` is a count, not a pass: 83 places where a panic or
a silent wrap could start. Several of the leg's findings (F6, F11) sit in that class of code.

## 4. Findings

17 findings were read from the one leg (`claude-opus`, F1–F17; F17 bundles six smaller items). With one leg there is
nothing to deduplicate, so each entry is one leg finding, keeping the leg's number for tracing back to its report.
**Found by:** `claude-opus` alone, in every case. **Severity:** the leg's own, adopted as given; with no second leg
there is no disagreement to settle, and this document does not re-grade.

### F1. A log read error ends the feed silently; the page still says "following". High

`src/monitors/fast_bridge/adapters/local.rs:39`, `:46`

`while let Ok(Some(line)) = lines.next_line().await` treats `Err` (any non-UTF-8 line) as end of file. The loop then
waits on a `journalctl -f` child that never exits, so no restart and no "ended" warning. Every later log line, alerts
and `capacity` included, is lost while the Activity title still reads "· following". Reproduced by the leg with
`logs-utf8.sh` / `utf8.toml`. Breaks claims 8 and 9.

### F2. HTTP redirects are followed to any host, POST body included; https can drop to http. Medium

`src/monitors/fast_bridge/adapters/http.rs:13` (no redirect policy, no `https_only`), used at `:18`, `:84`

reqwest's default `Policy::limited(10)` lets `lp_url`, a board or the RPC send tacmon anywhere with a 3xx. A 307 from
the RPC replays the `eth_getBalance` POST (with the pool account) to any URL, including paths on the provider's own
local API. The redirected answer is parsed as if from the configured source. Reproduced with `fake.py 18722 redirect`
and `redir.toml`; the unconfigured sink on port 18799 logged the GETs and the POST (`sink-18799.log`). Breaks claim 4,
and claim 2 in part.

### F3. Anyone can post an ad under the operator's `claimPub` and take the name and "on board" light. Medium

`src/monitors/fast_bridge/state.rs:185` (`my_ad`), `:188` (`name`); uses at `ui.rs:59`, `shell/mod.rs:187`

`my_ad` is the first ad whose public `claimPub` matches; no signature is checked. An impersonating ad sets the title
name, keeps "on board" green when the real provider is gone (threat 1), and, sorting cheapest first, is the one `find`
picks. Reproduced with `fake.py 18721 spoof` / `spoof.toml`. The leg did not read the board, so whether the board
refuses a duplicate key is not known.

### F4. The log dedupe lets a later info line overwrite an alert. Medium

`src/monitors/fast_bridge/state.rs:154-161`

Consecutive lines are merged on equal `msg` alone, keeping the newer entry. An `alert` followed by an `info` with the
same `msg` becomes one Info line and leaves the Overview's warnings pane (`ui.rs:161-162`); different details under one
`msg` collapse to the last. Reproduced with `logs-merge.sh` / `merge.toml`. Breaks claim 9.

### F5. Response bodies have no size cap. Medium

`src/monitors/fast_bridge/adapters/http.rs:18`, `:84`, `:55`

`.json()` buffers the whole body into a `serde_json::Value`, and the board path clones every item; the only bound is
the 8 s timeout. A 20 MB body drove peak RSS to 478,508 KiB, about 24× the body (`bigbody.py`, `big.toml`, `big.out`).
Breaks claim 8 (threat 3).

### F6. u128 sums and fee arithmetic on board data wrap in release and panic in debug; `as u64` truncates. Medium

Sums: `ui.rs:216`, `advisor.rs:55`, `:56`, `:66`, `:68`, `state.rs:175`, `:179`, `:182`. Products: `model.rs:23`,
`model.rs:31`. Casts: `wire.rs:32`, `:48`, `:49`, `:57`, `:86`, `:87`.

No `overflow-checks` in the release profile. Chosen wants make the Demand total read as any figure; an ad's oversized
`feeBps` reads as 0 and a huge `flatFee` truncates `effective_bps` to 1, listing it as cheapest. A debug build panics
(`attempt to add with overflow`) on the same input. Reproduced with `fake.py 18711 overflow` / `overflow.toml`, release
and debug. Breaks claims 8 (debug only) and 9.

### F7. An HTTP error or a missing field is drawn as fresh zero terms. Medium

`src/monitors/fast_bridge/adapters/http.rs:17-19` (status ignored), `wire.rs:11-21` (`field` returns 0), `wire.rs:28-35`,
`wire.rs:116`

A `404 {}` from `/info`, or a renamed field, becomes `Terms` of all zeros, drawn as real terms ("fee 0.00% + 0.00 ·
takes 0.00 to 0.00 TAC") and fed to the Advisor. Missing ad and want fields show as 0; unparsed `capacity` values give
0. Observed in the F2 and F6 runs. Contradicts claim 9 and AGENTS.md's "a failed read is `None`".

### F9. Any JSON log line with the right `msg` replaces identity and capacity; `free` falls back to the log with no age. Medium

`src/monitors/fast_bridge/adapters/wire.rs:76-90`, `state.rs:144-151`, `state.rs:170-172`, `feed.rs:57-58`

A `"fast bridge LP"` line replaces pool account (then sent to the RPC), claim key, bitcoin address and `dry_run`; a
`"capacity"` line replaces held and promised. `free()` uses the last capacity line, of any age, when `/info` gives no
`available` or does not answer, and draws it green. Reproduced with `logs-merge.sh`: "pool 0xATTACK…00beef" and "free
9,999,999.00" while `/info` was not answering. Whether a counterparty can inject such a line depends on the provider,
which the leg did not read. Breaks claim 9.

### F10. The health light comes wholly from whatever owns the `lp_url` port. Medium

`src/monitors/fast_bridge/adapters/http.rs:30-37`, `config.rs:46`, `ui.rs:56`

With the provider down, any local user can bind 127.0.0.1:8787 and answer `{"ok":true}` with chosen terms; nothing
cross-checks them. Threat 1. The leg's stand-in servers were shown as healthy. The leg notes severity depends on other
local users existing.

### F8. A want whose `maxFeeBps` is not a u64 is read as having no fee cap. Low

`src/monitors/fast_bridge/adapters/wire.rs:57`

`as_u64` turns `"10"`, `10.0` or `-1` into `None`, read everywhere as "any fee", so `blocker` skips the fee check and
the Advisor counts the bridge at any fee. Fails permissive. Reproduced with `fake.py 18723 capstr` / `cap.toml` ("MAX
FEE any").

### F11. Right-aligned number columns drop leading digits. Low

`src/monitors/fast_bridge/ui.rs:32-34` (`right`); widths at `ui.rs:136-145`, `:223`, `:254-262`, `:300`

An over-wide right-aligned `Line` is cut on the left: `2^127` base units shows as "873,037,158.84", a flat fee of about
1.8e17 TAC as "5,516.16". Nothing marks the cut on Market and Overview. Observed in the overflow run.

### F12. Stale or partial board data is drawn as fresh. Low

`src/monitors/fast_bridge/state.rs:107-112`; `adapters/http.rs:50-58`

`Event::Ads(None)` keeps old ads with no marker, and `board_ok` comes only from `/wants`. With several boards, a failing
one is skipped silently (its ads vanish, which can turn "on board" red), and entries listed on several boards are
counted once per board. Read only. Breaks claim 9.

### F13. Swap records are skipped silently, and a hung read keeps "records" green. Low

`src/monitors/fast_bridge/adapters/local.rs:66-82`

Unreadable or malformed records are dropped with `swaps_ok` still true. `tokio::fs::read` has no size cap or timeout
and follows symlinks, so a FIFO or a link to `/dev/zero` hangs or grows memory, and `swaps_ok` stays `true`. Needs write
access to `dataDir` (semi-trusted), hence Low. Read only.

### F14. Log lines have no length limit. Low

`src/monitors/fast_bridge/adapters/local.rs:38`, `state.rs:12`

`MAX_LOG = 5000` bounds count, not size; one line without a newline grows until out of memory. Read only. Breaks claim
8.

### F15. Expired wants still count as current demand. Info

`src/monitors/fast_bridge/state.rs:98-105`, `ui.rs:211`

Wants past `until` show "0s" and are counted in totals and the Advisor. The board may filter them; not checked. The
leg's confidence: medium.

### F16. Unicode format characters pass the terminal filter. Info

ratatui-core drops Cc characters only. Bidi overrides, zero-width characters and U+2028 in board or log text reach the
terminal: visual spoofing, not terminal control. Claim 7 holds as written. Read only.

### F17. Smaller items. Info

- System proxy variables are honoured by default (reqwest `auto_sys_proxy`), sending requests to a host not in the
  config; claim 4 does not mention it.
- `User-Agent: tacmon` (`http.rs:13`) identifies the client to the board and the RPC.
- Status lights use the same glyph for ok and failed (`widgets.rs:14-15`); without colour they look the same.
- aws-lc-sys runs a C build script.
- `expect("http client")` (`http.rs:13`) panics at start-up if the platform verifier cannot initialise.
- With `panic = "abort"`, `ratatui::init`'s hook still restores the terminal (noted, not a defect).

The leg also lists places it looked and found sound: `fmt::short`, `wire.rs:97`, `ui.rs:164-166`, `widgets::window`,
`claim_pub.get(2..8)`, the `effective_bps` zero guard, splash indexing, `shell::on_key`, `log_cmd` killed on exit (run),
`--demo` building no client, and `deny_unknown_fields` config parsing.

## 5. Claims

| # | Claim (short) | `claude-opus` | Stands? |
|---|---|---|---|
| 1 | No private key read, requested or stored | held | stands |
| 2 | Nothing written; every request is a read | broken, in part (F2: redirects let a remote party aim tacmon's POST at the provider's local API) | broken in part, on one leg |
| 3 | No `unsafe` in this crate | held (`src/main.rs:4`); `unsafe` check PASS | stands |
| 4 | Outbound only to configured hosts; `--demo` none | broken (F2; F17 system proxy). `--demo` part held | broken, on one leg |
| 5 | Only `log_cmd`, argv list, no shell | held (`local.rs:29`) | stands |
| 6 | rustls, platform verification, no way off | held; notes F2 can downgrade a board request to http, which is not a certificate check | stands |
| 7 | Untrusted text cannot control the terminal | held; F16 visual spoofing only | stands |
| 8 | No untrusted input panics, hangs or grows memory unbounded | broken (F1, F5, F6 in debug; also F13, F14) | broken, on one leg |
| 9 | Failed reads show as failed; nothing stale drawn as fresh | broken (F1, F4, F7, F9, F12) | broken, on one leg |
| 10 | Dependencies from crates.io, no advisory, permissive licences | could not tell: cargo-deny was not installed in the leg's environment; sources checked by hand, all crates.io | the `sources`, `advisories` and `licences` checks all PASS; the leg did not contradict them |

"On one leg" means no second leg confirmed or disputed the verdict.

## 6. Agreement and disagreement

The run had one leg, so this section has nothing to measure. Every finding above is single-leg and unconfirmed, and
there is no way from this run to tell a miss from a false positive. The leg reproduced most findings at runtime against
local stand-in servers (F1–F11); F12–F16 are from reading the code only.

One point of contact between the checks and the leg: the leg could not run cargo-deny, while the runner's checks ran it
and passed `advisories`, `licences`, `bans` and `sources`. These do not conflict.

## 8. Legs

**[`claude-opus`](rust-review/claude-opus/)** ([`report.md`](rust-review/claude-opus/report.md)). Claude Code 2.1.331,
observed model `claude-opus-5-5`, 477 s wall; the provenance block records it named all 25 scope files and left the
tree unchanged. It read every scope file whole and ran experiments against stand-in HTTP servers and `log_cmd` scripts
on 127.0.0.1, whose scripts, configs and output are kept in the leg directory. 17 findings (1 High, 8 Medium, 5 Low,
3 Info). It said it could not: run `cargo deny` ("error: no such command: deny"), so advisories, licences and bans were
unchecked by it; run `make audit-checks` (it ran the offline builds and `cargo test`, 18 passed, instead); reach the
network; or read the provider's and board's source, which leaves F3, F9 and F15 conditional. It also did not test
non-Linux platforms, slow-drip bodies, or fuzz `wire::log_line` and `wire::swap`.

The run was not graded: there is no `audits/KNOWN_ANSWER.md`.
