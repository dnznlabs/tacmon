# tacmon security review: one leg (Claude, Opus 5.5)

Date: 2026-11-24. Tree: `main` at a334085 plus the uncommitted working-tree changes present at the start.
All files under `.audit-leg/` were written by this leg: `fake.py` and `bigbody.py` (stand-in HTTP servers
bound to 127.0.0.1), `logs-*.sh` (stand-in `log_cmd`s), `*.toml` (configs pointing at them), `big.out`, `sink-*.log`.

## 1. What could not be done

- **`cargo deny` is not installed** in this environment (`error: no such command: deny`). Advisories, licences and
  bans were not checked. I did check the lockfile's sources by hand (all 321 packages are
  `registry+https://github.com/rust-lang/crates.io-index`) and ran `cargo tree` for TLS crates.
- No network, as the prompt says. Every runtime experiment used servers on 127.0.0.1 that I wrote, and `log_cmd`
  scripts. Nothing reached the real board.
- `make audit-checks` was not run. I ran its parts that need no network: `cargo build --release --locked --offline`,
  `cargo build --locked --offline` (debug), and `cargo test --locked --offline` (18 passed).
- I did not read the provider's or the board's source, so I could not tell whether the board checks who may post an ad
  (this matters for F3), or whether the provider ever writes counterparty text raw to its log (this matters for F9).

## 2. Files read

All files were read whole.

| File | What I looked at |
|---|---|
| `SECURITY.md` | the threat model, the 10 claims, what is not covered |
| `Cargo.toml` | release profile: `panic = "abort"`, no `overflow-checks` (so release arithmetic wraps); reqwest features |
| `deny.toml` | bans (openssl/native-tls), sources, licences; `multiple-versions = "warn"` |
| `src/config.rs` | load path, `deny_unknown_fields`, theme strings |
| `src/fmt.rs` | `short` slicing guard (ASCII check plus length check), `pct`, `dur` |
| `src/main.rs` | `forbid(unsafe_code)`, `--size` parsing, the demo and snapshot paths |
| `src/monitors/fast_bridge/adapters/http.rs` | client builder (timeout, redirects, user agent), status handling, body size, board join, RPC hex parse |
| `src/monitors/fast_bridge/adapters/local.rs` | process spawn (argv, no shell), line reading and its error path, restart loop, swap file reads |
| `src/monitors/fast_bridge/adapters/memory.rs` | the demo data; checked that it opens no connection; `now - age` (cannot underflow against a real clock) |
| `src/monitors/fast_bridge/adapters/mod.rs` | adapter selection; RPC only when `eth_rpc` is set |
| `src/monitors/fast_bridge/adapters/wire.rs` | every lenient reader: `units`, `field`, `as u64` casts, `maxFeeBps`, log-line signals, `ts.get(11..19)`, `detail` |
| `src/monitors/fast_bridge/advisor.rs` | u128 sums, `effective_bps` use, ladder ties |
| `src/monitors/fast_bridge/config.rs` | defaults (`lp_url` plain http, board URL, `log_cmd`), refresh floors |
| `src/monitors/fast_bridge/feed.rs` | task lifetimes, unbounded channel, pool-account watch, failure-to-`None` mapping |
| `src/monitors/fast_bridge/model.rs` | `tac`, `fee_on`/`effective_bps` overflow and the `as u64` truncation, `closed` |
| `src/monitors/fast_bridge/mod.rs` | refresh drain, page indexing |
| `src/monitors/fast_bridge/ports.rs` | trait shapes |
| `src/monitors/fast_bridge/state.rs` | `apply` per event (staleness flags), the log dedupe, `free()` fallback, `my_ad` matching, scroll arithmetic |
| `src/monitors/fast_bridge/ui.rs` | every string from an untrusted source that reaches a Span or Cell, column widths against amounts, slice bounds in `activity` |
| `src/monitors/mod.rs` | monitor list |
| `src/shell/mod.rs` | event loop, input thread, restore on error, snapshot loop, page keys |
| `src/shell/monitor.rs` | Wake (unbounded) |
| `src/shell/splash.rs` | `&WORD[..typed]` bound, u16 arithmetic, float edge cases (no untrusted input) |
| `src/shell/theme.rs` | colour parsing fallback |
| `src/shell/widgets.rs` | `window` bounds, the `light` glyphs |

Out of scope, opened to understand the code: `ratatui-core-0.1.2/src/text/span.rs:314` and `src/buffer/buffer.rs:351`
(how control characters are filtered), and `reqwest-0.13.5/src/redirect.rs` and `src/async_impl/client.rs` (the
redirect policy, `https_only`, system proxy, key log).

## 3. Claims

| # | Verdict | Finding or reason |
|---|---|---|
| 1 | held | No config field, port or code path reads, asks for or stores a key. |
| 2 | broken, in part | tacmon's own code writes no file and sends only GETs and one `eth_getBalance` POST. But it follows redirects to any URL, so an untrusted server picks where those requests go, including any path on the provider's local API (F2). |
| 3 | held | `#![forbid(unsafe_code)]` at `src/main.rs:4`; no `unsafe` in `src/`. Dependencies (aws-lc-sys and others) contain unsafe code and C; the claim covers this crate only. |
| 4 | broken | Redirects are followed to hosts not in the config, POST bodies included (F2). The system proxy variables (`HTTP(S)_PROXY`) are honoured by default (F17). `--demo` held: `create_demo_ports` builds no client. |
| 5 | held | `local.rs:29`: `Command::new(prog).args(args)`, no shell. `prog` is found through `PATH`, which is part of the trusted environment. |
| 6 | held | No `danger_*` call. reqwest 0.13's `rustls` feature uses rustls-platform-verifier with aws-lc-rs. `cargo tree` shows no openssl or native-tls. `SSLKEYLOGFILE` is opt-in and not turned on. Note: a redirect can still downgrade a board request from https to http (F2). That breaks "reads the board over HTTPS", not the certificate checks. |
| 7 | held | ratatui-core drops every grapheme containing a `char::is_control` character (Cc, which includes ESC and the C1 range) in `Span::styled_graphemes` and `Buffer::set_stringn`. Format characters (bidi overrides, zero-width) still pass, which allows visual spoofing but not terminal control (F16). |
| 8 | broken | One invalid UTF-8 byte stops the log feed for good (F1). A response body has no size cap (F5). In a debug build, board input panics the process (F6). |
| 9 | broken | A dead log is drawn as "following" (F1). An HTTP error body or a missing field is drawn as fresh zeros (F7). An alert is overwritten by a later info line (F4). Stale ads carry no marker (F12). A stale log capacity is drawn as current "free" (F9). |
| 10 | could not tell | cargo-deny is not installed. All lockfile sources are crates.io (checked by hand); advisories and licences were not checked. |

## 4. Findings

### F1. A read error on the log ends the feed silently, leaves `log_cmd` running, and the page still says "following". High

`src/monitors/fast_bridge/adapters/local.rs:39`, `:46`

`while let Ok(Some(line)) = lines.next_line().await` treats `Err` the same as end of file. `Lines::next_line` returns
`Err(InvalidData)` on any line that is not valid UTF-8. The loop exits and goes to `child.wait().await`. But the child
(`journalctl -f`) has not exited and never will: it is still following, and no one reads its stdout any more. That task
waits for ever. The "log_cmd ended; starting it again" warning is never sent and the command is never restarted. Every
later log line is lost, including alerts and the `capacity` lines that set "free/promised/held". The Activity title
still says "· following", so a dead feed looks like a quiet provider.

- **Who:** anything that puts one non-UTF-8 byte into the unit's stdout or stderr as journald records it, for example a
  dependency's binary output, a truncated multibyte character, or a counterparty value the provider logs raw.
  `journalctl -o cat` passes the bytes through.
- **Input:** `printf 'garbage \377 byte\n'` followed by a JSON alert line.
- **Checked by running it:** `.audit-leg/logs-utf8.sh` with `utf8.toml`, then
  `tacmon -c .audit-leg/utf8.toml --snapshot activity --wait 3`. Only the line before the bad byte appears, the alert
  after it never does, there is no "ended" warning, and the title is "Activity · following".
- **Confidence:** high.
- **Related:** `BufReader::lines` has no limit on line length (F14).

### F2. HTTP redirects are followed to any host, POST body included, and https can be downgraded to http. Medium

`src/monitors/fast_bridge/adapters/http.rs:13` (the client builder sets no redirect policy and no `https_only`),
used at `:18` and `:84`

reqwest's default is `Policy::limited(10)`, with `https_only = false`. Whatever answers `lp_url`, any configured board,
and the RPC can each answer with a 3xx to any URL. tacmon then sends its request there with `User-Agent: tacmon`:

- A 302 from the board sends tacmon's GETs to any host, which learns the operator's IP address and polling rhythm
  (threat 4). An https board URL can be redirected to plain http.
- A 307 from the RPC replays the POST, with the pool account in its body, to any URL. That includes any path on
  `http://127.0.0.1:8787`, the provider's own API, so a remote party can make tacmon POST to the provider's endpoints.
  The body is a fixed `eth_getBalance` JSON, so a state change needs an endpoint that accepts it. Claim 2's "every
  request is a read" then rests on the provider, not on tacmon.
- The redirect target's answer is parsed as if it came from the configured source (see F7 for what a `{}` does).

**Checked by running it:** `.audit-leg/fake.py 18722 redirect` as `lp_url`, the board and `eth_rpc`, with a sink on
port 18799 that appears nowhere in `redir.toml`. `sink-18799.log` recorded `GET /elsewhere/{info,health,wants,ads}`
and `POST /stolen-post ... "params":["0xATTACKER…beef","latest"]`.

**Confidence:** high. The fix is `redirect(Policy::none())`, plus `https_only` for the board if that is what the
claim means.

### F3. Anyone can post an ad under the operator's `claimPub` and take the operator's name and "on board" light. Medium

`src/monitors/fast_bridge/state.rs:185` (`my_ad`), `:188` (`name`), and the uses at `ui.rs:59` and `shell/mod.rs:187`

`my_ad` is the first ad whose `claimPub` equals the operator's. The operator's `claimPub` is public: it is in their real
ad. SECURITY.md says ads are "posted without permission", and tacmon checks no signature. An attacker can post an ad
with that key and any name.

- The title bar then shows the attacker's name.
- If the operator's own provider has dropped off the board, the "on board" light stays green on the impersonating ad.
  That is threat 1: the monitor shows a healthy provider when it is not.
- The Market page marks the attacker's row with "▶" as the operator's.
- Ads are sorted cheapest first, so a cheap impersonating ad sorts before the real one, and `find` picks it.

**Checked by running it:** `fake.py 18721 spoof` plus `spoof.toml`. The title read "tacmon fast bridge IMPERSONATOR" and
"● on board" was lit from that ad.

**Confidence:** high that tacmon trusts the field. Not known whether the board refuses a second ad for a key it has
seen (I did not read the board).

### F4. The log dedupe lets a later info line overwrite an alert, which then leaves the warnings pane. Medium

`src/monitors/fast_bridge/state.rs:154-161`

Two consecutive lines are merged when their `msg` is equal. The level and detail are not compared, and
`last.entry = entry` keeps the newer one.

- An `alert` followed by an `info` with the same `msg` becomes a single Info line. The Overview's "Warnings and alerts"
  pane (`ui.rs:161-162`) filters on level, so the alert disappears.
- Lines with the same `msg` and different details (for example two "offer accepted" for different swap ids) collapse
  into one, keeping only the last detail.

**Input:** `{"level":"alert","msg":"tick failed","why":"pool unreachable"}` then `{"level":"info","msg":"tick failed"}`.

**Checked by running it:** `logs-merge.sh` with `merge.toml`, snapshot of the overview. The "Warnings and alerts" pane
was empty.

**Confidence:** high.

### F5. Response bodies have no size cap. 20 MB of JSON costs about 470 MB of memory. Medium

`src/monitors/fast_bridge/adapters/http.rs:18`, `:84`, `:55` (`items.iter().cloned()` holds a second copy)

`.json()` buffers the whole body, then builds a `serde_json::Value`, and the board path clones every item. The only
bound is the 8 s total timeout, so the limit is bandwidth × 8 s.

- On localhost (`lp_url`, which SECURITY.md lists as "whatever answers on that port") that is several GB, enough to be
  killed for running out of memory (threat 3).
- The board and the RPC can do the same at internet speed.

**Checked by running it:** `bigbody.py` serving `{"wants":[0,0,…]}` with 10M elements (20 MB). tacmon's peak RSS was
478,508 KiB, about 24 times the body.

**Confidence:** high.

### F6. u128 sums and fee arithmetic on board data wrap in release and panic in debug. `as u64` casts truncate. Medium

- Sums: `ui.rs:216` (Demand title total), `advisor.rs:55`, `:56`, `:66`, `:68` (volumes and revenue), and
  `state.rs:175`, `:179`, `:182`.
- Products: `model.rs:23` (`amount * bps + flat`), `model.rs:31` (`* 10_000`, then `as u64`).
- Casts: `wire.rs:32` (`feeBps as u64`), `wire.rs:48`, `:49`, `:57`, `:86`, `:87`.

The release profile does not turn on `overflow-checks`, so arithmetic wraps silently there.

- **Who:** anyone who can post a want or an ad.
- **What a want can do:** two wants of `2^127` base units wrap the Demand title's total. Three wants (two of `2^127`
  and one of 5,000 TAC) are shown as "3 · 5,000.00 TAC". A poster can choose amounts that make the total read as any
  figure.
- **What an ad can do:** `feeBps: "18446744073709551616"` is read as 0. A `flatFee` of about 1.8e25 makes
  `effective_bps` truncate to 1. The ad then lists first as "cheapest" at "@100 TAC 0.01%" while its real fee is
  astronomical.
- **In a debug build**, the same Demand input panics the process: `attempt to add with overflow` at
  `iter/traits/accum.rs`. This reaches threat 3 for anyone who runs `cargo run` without `--release`.

**Checked by running it:** `fake.py 18711 overflow` plus `overflow.toml`, in both the release and the debug binary.

**Confidence:** high. `make run` uses `--release`, so the panic needs a debug build; the wrong numbers do not.

### F7. An HTTP error or a missing field is drawn as fresh zero terms. Medium

`src/monitors/fast_bridge/adapters/http.rs:17-19` (the status is ignored on purpose, for every endpoint),
`wire.rs:11-21` (`field` returns 0 for a missing value, a string, a float, a negative number, or a number above
`u64::MAX`), `wire.rs:28-35`

A `404 {}` from `/info`, a proxy's error JSON, or a provider build that renamed a field becomes
`Terms { min: 0, max: 0, fee_bps: 0, flat: 0 }`. The header then shows "fee 0.00% + 0.00 · takes 0.00 to 0.00 TAC" as
real terms, and every want is "over your max". The Advisor gives advice on those zeros.

- This contradicts claim 9 and AGENTS.md ("A failed read is `None`, never a made-up zero").
- Ads and wants with missing fields show as "TAKES 0.00 – 0.00" and "EXPIRES IN 0s".
- A `capacity` log line with string values that do not parse gives held 0 and promised 0, which drives `free`.
- In `wire.rs:116`, the keys `free`, `held` and `promised` show "0.00" when their value does not parse.

**Checked by running it:** the redirect run (F2) sent `/info` to a sink that answers `404 {}`. The header showed
"fee 0.00% + 0.00 · takes 0.00 to 0.00 TAC". The overflow run showed "TAKES 0.00 – 0.00" for an ad that had no
min or max.

**Confidence:** high. The fix: check the status for `/info`, and read required fields as `Option`.

### F8. A want whose `maxFeeBps` is not a u64 is read as having no cap, and can be shown as "you can take it". Low

`src/monitors/fast_bridge/adapters/wire.rs:57`

`v.get("maxFeeBps").and_then(Value::as_u64)` turns `"10"`, `10.0` or `-1` into `None`. The rest of the code reads
`None` as "any fee". So a bridger's real cap is shown as "any", `blocker` skips the fee check, and the Advisor counts
the bridge at every fee. The failure falls on the permissive side. Every other amount on the board is a string, so
this format is plausible.

**Checked by running it:** `fake.py 18723 capstr` plus `cap.toml`. The Demand page showed "MAX FEE any" for
`"maxFeeBps":"10"`.

**Confidence:** high.

### F9. Any JSON log line with the right `msg` replaces identity and capacity, and `free` falls back to the log with no age. Medium

`src/monitors/fast_bridge/adapters/wire.rs:76-90`, `state.rs:144-151`, `state.rs:170-172`, `feed.rs:57-58`

Any line whose `msg` is `"fast bridge LP"` replaces the pool account, the claim key, the bitcoin address and
`dry_run`. The new pool account is then sent to the RPC, and the ETH shown is that account's. Any `"capacity"` line
replaces held and promised.

journald splits on newlines. If the provider ever writes counterparty text containing `\n{...}` to stdout or stderr,
that text becomes a line of its own. SECURITY.md says the log carries counterparty values.

`free()` uses the last capacity line whenever `/info` gives no `available`, or does not answer at all. That value can
be hours old, or from before a restart. It is drawn green as "free", with no age, and the Demand verdict "✔ you can take
it" uses it.

**Checked by running it:** `logs-merge.sh` injected a start line and a capacity line. The header showed
"pool 0xATTACK…00beef" and "free 9,999,999.00" while the status line said "the provider does not answer /info".

**Confidence:** high that tacmon trusts these lines. Whether a counterparty can inject one depends on the provider.

### F10. The health light comes wholly from whatever owns the `lp_url` port. Medium

`src/monitors/fast_bridge/adapters/http.rs:30-37`, `config.rs:46` (plain `http://127.0.0.1:8787`), `ui.rs:56`

When the provider has crashed or not started, any local user can bind 127.0.0.1:8787 and answer `{"ok":true}` with
generous terms. tacmon shows a green health light and those terms. Nothing cross-checks them: not the log's start
line, not the `claimPub`, not a pid. That is exactly threat 1, from a source SECURITY.md lists as untrusted.

**Checked by running it:** the `fake.py` stand-ins on unprivileged ports were shown as healthy.

**Confidence:** high. Severity depends on whether the machine has other local users.

### F11. Right-aligned number columns drop leading digits, so large amounts read as small ones. Low

`src/monitors/fast_bridge/ui.rs:32-34` (`right`), and the widths at `ui.rs:136-145`, `:223`, `:254-262`, `:300`

A right-aligned `Line` wider than its column is cut on the left. An amount of `2^127` base units is shown as
"873,037,158.84" in the 14-wide Demand column. An ad's flat fee of about 1.8e17 TAC is shown as "5,516.16" in the
8-wide FLAT column.

- On the Demand page the verdict ("over your max") is still right.
- On Market and Overview (PAYS and GETS are 12 wide, so from 100,000,000.00 up) nothing says the figure was cut.

**Checked by running it:** the overflow run.

**Confidence:** high.

### F12. Stale or partial board data is drawn as fresh. Low

- `src/monitors/fast_bridge/state.rs:107-112`: `Event::Ads(None)` keeps the last ads with no marker. `board_ok` is set
  only from `/wants`, so if `/ads` fails and `/wants` works, the Market page and the "on board" light run on old data
  as if it were fresh.
- `http.rs:50-58`: with more than one board, a board that fails is skipped silently. Its ads vanish, which can turn
  "on board" red although the board you are on is merely down. Ads and wants listed on several boards are counted
  once per board, which inflates demand and the Advisor's revenue.

**Confidence:** high (read only).

### F13. Swap records are skipped silently, and a hung read keeps "records" green for ever. Low

`src/monitors/fast_bridge/adapters/local.rs:66-82`

- A file that cannot be read or parsed, or that has no `offer` or `offer.btc`, is dropped while `swaps_ok` stays
  true. A permanently corrupt record for an open swap disappears from the Overview and from "incoming", with no sign.
- `tokio::fs::read` has no size cap and no timeout, and it follows symlinks. If `foo.json` is a FIFO or a link to
  `/dev/zero`, the swaps task hangs or grows memory without bound. Because no new event is ever sent, `swaps_ok` keeps
  its last value, `true`.

**Who:** whoever can write in the provider's `dataDir`. That is semi-trusted, hence Low.

**Confidence:** high (read only).

### F14. Log lines have no length limit. Low

`local.rs:38` (`BufReader::lines`), `state.rs:12`

`MAX_LOG = 5000` bounds the number of lines, not their size. One line with no newline grows a `String` until the
process runs out of memory. 5000 lines of 1 MB each hold 5 GB.

**Confidence:** high (read only).

### F15. Expired wants still count as current demand. Info

`state.rs:98-105`, `ui.rs:211`

A want whose `until` has passed is shown with "0s" and counted in the totals and the Advisor. The board may filter
these out itself; I could not check.

**Confidence:** medium.

### F16. Unicode format characters pass the terminal filter. Info

ratatui-core removes Cc characters only. Bidi overrides (U+202E), zero-width characters and U+2028 in ad names, the
health error or log text reach the terminal. They allow visual spoofing, such as a name that renders reversed or a
miscounted width, but not cursor or colour control. Claim 7 holds as written.

**Confidence:** high (read only).

### F17. Smaller items. Info

- **System proxy:** reqwest's `auto_sys_proxy` is on by default (`client.rs:310`), so `HTTP(S)_PROXY` and `ALL_PROXY`
  send every request to a host that is not in the config. The environment is trusted, but claim 4 does not say this.
- **Fingerprint:** `User-Agent: tacmon` (`http.rs:13`) tells the board and the RPC that a tacmon operator is polling.
- **Lights:** the status lights use the same glyph, ●, for ok and failed (`widgets.rs:14-15`). With no colour (a
  monochrome terminal, `--snapshot`, colour-blind users) a failed read looks the same as a good one.
- **Build:** aws-lc-sys, pulled in by reqwest's rustls default provider, runs a C build script.
- **Startup panic:** `expect("http client")` (`http.rs:13`) panics if the platform verifier cannot initialise. That
  happens at start-up, before the terminal is taken over.
- **Terminal restore:** `panic = "abort"` in release. `ratatui::init`'s panic hook still restores the terminal before
  the abort.

### Checked and found sound

These are where I looked for bugs and found none:

- `fmt::short` slicing.
- `wire.rs:97` `t.get(11..19)` (`get`, so no panic).
- The slice bounds in `ui.rs:164-166`.
- `widgets::window`.
- `market`'s `claim_pub.get(2..8)`.
- The `effective_bps` zero guard.
- Splash indexing.
- `shell::on_key` page arithmetic.
- `log_cmd` children are killed when tacmon exits (`kill_on_drop`; checked by running it: no orphan).
- `--demo` builds no client.
- Config parsing is `deny_unknown_fields`, and the config is trusted.

## 5. What I did not look at

- The source of the provider and of the board: whether ads are authenticated (F3), what the provider puts in its log
  (F9), whether the board drops expired wants (F15).
- cargo-deny output (claim 10).
- Behaviour on non-Linux platforms (other system proxy and certificate stores).
- Slow-drip bodies under the 8 s timeout. These are bounded by the timeout, so I did not measure them.
- A fuzz run of `wire::log_line` and `wire::swap`. I read them and found no panic path.
