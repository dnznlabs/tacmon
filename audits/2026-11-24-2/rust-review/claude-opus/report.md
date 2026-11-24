# Audit leg report: tacmon

Harness: Claude Code, model Claude Opus 5.5 (`claude-opus-5-5`). Date 2026-11-24.

## 1. What could not be done

- **`cargo deny` is not installed** in this environment (`error: no such command: deny`). The advisory,
  licence and ban checks behind claim 10 were not run. I checked only that `Cargo.lock` names no git or
  non-crates.io source, by grep.
- Nothing else failed. `cargo build --release --locked --offline` and `cargo test --locked --offline`
  ran: 35 passed, 0 failed. The built binary ran with `--snapshot` against small local Python HTTP
  servers in `.audit-leg/`, bound to 127.0.0.1 only, with a fake `log_cmd` and a fake `data_dir`.
- `--snapshot` has no colour, so I could not see colour (the status lights) in a run. Where a finding
  depends on a light's colour, I worked the colour out from the code.

Files in `.audit-leg/`: `server.py` (redirects, control characters, impostor ad), `flood.py` (a 4 MiB
`/wants`), `log.sh`, `cfg.toml`, `cfg-flood.toml`, `data/swaps/rec1.json`, `measure.py`, `snap-*.txt`
(the captured output).

## 2. Files read

Every file was read whole.

| File | What I looked at |
|---|---|
| `SECURITY.md` | threat model, claims 1–10, what is not covered |
| `Cargo.toml` | `panic = "abort"` in release, reqwest features (`rustls`, `json`, no `system-proxy`), tokio features |
| `deny.toml` | bans, sources, licences, `unmaintained = "all"` |
| `src/config.rs` | TOML loading, `deny_unknown_fields`, default path |
| `src/fmt.rs` | `short` slicing (ASCII guard), `dur`, `pct`, `now` |
| `src/main.rs` | `forbid(unsafe_code)`, argument parsing, `--size` parsing, snapshot flow |
| `src/monitors/fast_bridge/adapters/http.rs` | client builder (timeout, redirect policy), body cap, `error_for_status`, board joining, RPC hex parsing to f64 |
| `src/monitors/fast_bridge/adapters/local.rs` | `next_line` cap, command spawn and restart, swap-file metadata then read, symlinks, FIFOs |
| `src/monitors/fast_bridge/adapters/memory.rs` | that demo ports make no I/O; demo `now - age` arithmetic |
| `src/monitors/fast_bridge/adapters/mod.rs` | which adapters are built; that the RPC is only built when configured |
| `src/monitors/fast_bridge/adapters/wire.rs` | every lenient default, `units`, `terms`, `swap`, `want`, `ad`, `epoch`, `log_line`, `detail` |
| `src/monitors/fast_bridge/advisor.rs` | blocker order, ladder arithmetic, revenue on dust |
| `src/monitors/fast_bridge/config.rs` | defaults, refresh floors |
| `src/monitors/fast_bridge/feed.rs` | task lifetimes, unbounded channels, failure → `None`, pool-account learning |
| `src/monitors/fast_bridge/model.rs` | `tac`, `fee_on`, `total`, `effective_bps` overflow behaviour |
| `src/monitors/fast_bridge/mod.rs` | the `Monitor` implementation, page indexing |
| `src/monitors/fast_bridge/ports.rs` | trait shapes; `GasBalance` returns f64 |
| `src/monitors/fast_bridge/state.rs` | what a `None` does per source, log store bound, dedupe counter, own-ad logic |
| `src/monitors/fast_bridge/ui.rs` | every untrusted string drawn, slicing in `activity`, staleness marks per page |
| `src/monitors/mod.rs` | `create_monitors`, demo switch |
| `src/shell/mod.rs` | event loop, unbounded `Msg` channel, snapshot drain, key handling indexes |
| `src/shell/monitor.rs` | `Wake`, the trait |
| `src/shell/splash.rs` | `&WORD[..typed]` bound, u16 arithmetic, braille `from_u32` |
| `src/shell/theme.rs` | colour parse fallback |
| `src/shell/widgets.rs` | `window` bound, light semantics |

I also opened ratatui-core 0.1.2 (`Span::styled_graphemes`, `Buffer::set_stringn`), reqwest 0.13.5
(`error_for_status`, features) and hyper-util 0.1.21 (env proxy matcher) to check what the in-scope
code relies on.

## 3. Claims

| # | Verdict | Finding or reason |
|---|---|---|
| 1 | held | No code path reads, requests or stores a key. The only identity data is public (claim key, pool account, BTC address) from the log. |
| 2 | held | Only `read_dir`, `metadata`, `read`, `read_to_string` and GET/POST reads. The single POST is `eth_getBalance`. Nothing is written but the terminal and stdout. |
| 3 | held | `#![forbid(unsafe_code)]` at the crate root (`src/main.rs:4`); no `unsafe` in the crate. Dependencies do contain unsafe code and a C build (`aws-lc-sys`): see F-12. |
| 4 | **broken** | F-1: a 3xx is not an error. It is not followed, but its body is read as a good answer. F-10: the "system proxy" wording does not match the build. |
| 5 | held | `tokio::process::Command::new(prog).args(args)`, no shell (`local.rs:58`). `prog` is looked up on `PATH`, which is trusted. |
| 6 | held | reqwest `rustls` uses `rustls-platform-verifier` and aws-lc-rs; no `danger_*` call; `deny.toml` bans openssl/native-tls. Roots can still be replaced through `SSL_CERT_FILE`/`SSL_CERT_DIR` (trusted environment; info). |
| 7 | held | ratatui-core drops every grapheme that contains a `char::is_control` (`span.rs:314`, `buffer.rs:351`), which covers C0, DEL and C1 (U+009B). Confirmed by a run: ESC, BEL and U+009B from `/health`, a board ad name and a log line did not reach the output. Nothing in this crate tests or pins this (F-11). |
| 8 | held, with caveats | No panic found on any untrusted path. The caps hold. The bounded amounts are large, and two queues can grow while the terminal is blocked: F-7, F-8, F-9. |
| 9 | **broken** | F-2 (Market and Advisor draw stale board data as current), F-3 (log-sourced held/promised/open/holds carry no "from the log" mark or age, and show a made-up 0.00 before any line), F-5 (made-up zeros from missing fields), F-6 (a partial board failure is invisible). |
| 10 | could not tell | `cargo deny` was not available. `Cargo.lock` names crates.io only. |

## 4. Findings

### F-1 — A redirect is read as a successful answer, not an error (claim 4 broken)
- **Location:** `src/monitors/fast_bridge/adapters/http.rs:38-40` (`get`), `:43-45` (`get_any_status`), `:109` (RPC); client at `:16-17`.
- **Severity:** low.
- **Who, what input, what happens:** whoever answers `lp_url`, a board or the RPC. With
  `Policy::none()`, reqwest hands the 3xx response back to the caller. `error_for_status()` only rejects
  4xx and 5xx (reqwest `async_impl/response.rs:381`), so a 3xx with a JSON body is parsed as a real
  answer. The existing test passes only because its 307 has an empty body, which fails JSON parsing.
  - Ran: `/info` answering `307 Location: http://203.0.113.9/info` with a terms body. The header
    showed `fee 1.50% + 0.10 · takes 10.00 to 1,000.00 TAC`.
  - Ran: `/ads` answering `302` with a JSON body. The ad was listed on Market.
  - The Location target is not contacted, so nothing leaks. But "a redirect is an error" is false. A
    provider or board mid-migration that redirects with a cached body is shown as healthy and current.
- **Confidence:** high. Ran it (`.audit-leg/server.py`, `snap-overview.txt`, `snap-market.txt`).

### F-2 — Market and Advisor draw stale board data as current; expired wants are kept (claim 9 broken)
- **Location:** `src/monitors/fast_bridge/state.rs:104-120`; `src/monitors/fast_bridge/ui.rs:234-280` (`market`, never reads `ads_ok`); `ui.rs:282-343` (`advisor`, never reads `board_ok`).
- **Severity:** medium. Threat 1: false numbers the operator acts on.
- **Who, what input, what happens:** the board stops answering (down, blocked, 5xx, or over 4 MiB). The
  last ads and wants stay in the state, which is intended. But:
  - The Market page title is the same as when it is fresh.
  - The Advisor's "would earn the most", "a max of X would put all of them in range" and "N more TAC
    would cover all of it" are still computed from the old wants, with nothing on the page saying so.
  - `until` is only checked when a fresh list arrives (`state.rs:109`). While the board is down, wants
    past their time stay counted, and Demand shows them as `0s` to expire.
  - Only Demand's title and the header light say the board is not answering.
- **Confidence:** high, from reading. Not run, because a snapshot draws once and the failure needs a
  success followed by a failure.

### F-3 — Figures from the log carry no source or age, and a missing capacity is drawn as 0.00 (claim 9 broken)
- **Location:** `src/monitors/fast_bridge/ui.rs:39-53`; `state.rs:43,76,183-189`.
- **Severity:** medium.
- **Who, what input, what happens:** the claim says "a figure taken from the log says so and gives its
  age". Only `free` does.
  - `promised`, `held`, `open` and `holds` come from the last `capacity` line and are drawn bold, with
    no mark and no age, however old that line is.
  - Before any capacity line (or when the line is older than journalctl's `-n 2000`, or `log_cmd`
    fails), `Capacity::default()` is drawn as `free 0.00 (from the log)   promised 0.00   held 0.00
    (0 open, 0 holds)`. That is a made-up zero said to come from a log that said nothing.
  - Ran: with no capacity line in the log, the header read exactly that (`snap-overview.txt`).
- **Confidence:** high. Ran it.

### F-4 — One forged ad under the provider's key lights "on board" and names the provider; with the identity unknown, the light is red, not unknown
- **Location:** `src/monitors/fast_bridge/state.rs:202-219`; `ui.rs:65-67`, `ui.rs:239`.
- **Severity:** medium. Threat 1. The board checks no signature (SECURITY.md).
- **Who, what input, what happens:** two cases.
  - **Forged ad, real one missing.** Anyone posts one ad with `claimPub` equal to the provider's while
    the provider's own ad is absent (not posted, expired, or dropped by the board). `ad_contested()` is
    false, so `my_ad()` returns the impostor without comparing its terms with `/info`.
    - The "on board" light goes green.
    - Market marks the row `▶` as yours, with the impostor's terms.
    - The impostor's `name` becomes the window title.
    - SECURITY.md's mitigation ("says when more than one ad uses the provider's key") does not fire,
      because only one does.
    - Ran: an ad with other terms (fee 0.01%, takes 0.00–0.00) against `/info` at 1.50%. Output: title
      `tacmon fast bridge imp[5mostor`, Market `▶ imp[5mostor 0.01%`. No contested warning.
  - **Identity unknown.** When `claim_pub` is not known (the start line is older than `-n 2000`, the
    log cannot be read, or the provider restarted long ago), `my_ads()` is empty. The light is then
    `Some(false)`, red ("not on board"), when it should be unknown.
- **Confidence:** high for the first case (ran it; the light colour is from the code). High for the
  second, from reading.

### F-5 — Missing or unreadable fields become zeros that are drawn as real values
- **Location:** `src/monitors/fast_bridge/adapters/wire.rs:19-21` (`field`), `:49-60` (`swap`), `:142` (`detail`), `:68` (`want.until`).
- **Severity:** low.
- **Who, what input, what happens:**
  - A swap record whose `offer.btc` or `offer.evm` lacks `value`/`amount` shows `PAYS 0.00 GETS 0.00
    FEE 0.00`. It adds 0 to "incoming" and "received", so the header money is understated with no mark.
    A missing `created` shows an age of `20781d…`. Ran: `data/swaps/rec1.json` gave
    `rec1 ●●○○○○ locking 0.00 0.00 0.00 20781d0 —`.
  - A log line with `"free":"lots"` (or a negative number, or a float) is shown as `free 0.00`.
    Ran: `snap-activity.txt`.
  - A want without `until` shows `EXPIRES IN 0s`, when it means it has no expiry.
  - This is contrary to the repo's rule that a failed read is `None`, never a made-up zero.
- **Confidence:** high. Ran the first two.

### F-6 — With several boards, one failing is invisible, and the same entry on two boards is counted twice
- **Location:** `src/monitors/fast_bridge/adapters/http.rs:76-84`.
- **Severity:** low. Only with more than one board configured; the default is one.
- **Who, what input, what happens:**
  - A board that errors is skipped with `continue`. The joined list from the others is reported as a
    full success (`board_ok`/`ads_ok` true).
  - Lists are not de-duplicated. A provider's own ad mirrored on two boards makes `ad_contested()` true
    ("another ad uses your key", in red) and drops the name. A want mirrored on two boards doubles
    volume and revenue on Demand and Advisor.
- **Confidence:** high, from reading.

### F-7 — Board answers inflate about 50x in memory; anyone can make the Advisor count fees on dust
- **Location:** `http.rs:79-81` (whole `Value` tree plus `cloned()` copy); `advisor.rs:41-71`; `model.rs:23-25`.
- **Severity:** low.
- **Who, what input, what happens:** anyone who can post wants. Ran: a 4 MiB `/wants` of 123,360 wants
  of 1 base unit each.
  - Peak RSS was 197 MiB, against 11 MiB at baseline. That is per board per refresh, so several boards
    multiply it.
  - Each want is kept, and every frame builds a table row for each one.
  - With a small `min` and no cap, the Advisor showed `0.10% … 123360 bridges, volume 0.00, YOU EARN
    12,336.00 ★ best`. The flat fee is counted as revenue on wants smaller than the fee itself: a bridge
    that would receive less than nothing. Revenue should be capped at the amount, or such wants excluded.
  - More generally, every Advisor suggestion is driven by unauthenticated wants. SECURITY.md lists the
    unsigned *ad* but not the unsigned *want*.
- **Confidence:** high. Ran it (`flood.py`, `measure.py`).

### F-8 — The log store can hold hundreds of MiB
- **Location:** `src/monitors/fast_bridge/state.rs:12,176-179`; `local.rs:15`; `wire.rs:98,124`.
- **Severity:** low.
- **Who, what input, what happens:** the provider's log, carrying counterparty text. Each entry keeps
  `msg` (up to 64 KiB) and `detail` (up to about 64 KiB) for up to 5,000 entries: about 640 MiB worst
  case. It is bounded, but large enough to take a small VPS down, which is threat 3.
- **Confidence:** medium. Read only. The size comes from the caps, not from a run.

### F-9 — Unbounded channels grow while a draw is blocked; a swap file can hang or outgrow its cap
- **Location:** `feed.rs:47`, `local.rs:53`, `shell/mod.rs:49`; `local.rs:113-117`.
- **Severity:** low / info.
- **Who, what input, what happens:**
  - **Channels.** Events and `Msg::Wake` go through unbounded channels that are drained only between
    draws. If the terminal write blocks (XOFF, a stalled SSH session), the log reader keeps queueing
    lines of up to 64 KiB, one `Wake` per event, with no bound.
  - **Swap files.** `metadata` and then `read` is a TOCTOU. A file that grows after the check is read
    whole, and a `.json` swapped for a FIFO after the check blocks the swaps task for ever. Symlinks are
    followed, so any file of 1 MiB or less is opened. This needs write access to `dataDir`.
  - **Directory size.** The number of records read per pass is unbounded.
- **Confidence:** medium. Read only.

### F-10 — The "system proxy" wording does not match the build; a proxy can answer for the local provider
- **Location:** `Cargo.toml:24` (no `system-proxy` feature); `http.rs:16-17`.
- **Severity:** info.
- **Who, what input, what happens:**
  - With `default-features = false`, reqwest's `system-proxy` is off. macOS and Windows system proxy
    settings are ignored; only `HTTP(S)_PROXY`/`ALL_PROXY` are used.
  - The hyper-util env matcher does not exempt loopback. If `HTTP_PROXY` is set and `NO_PROXY` does not
    name 127.0.0.1, the request for `http://127.0.0.1:8787/health` goes to the proxy, and the proxy
    answers for the provider.
- **Confidence:** medium. Read the dependency source; not run.

### F-11 — Claim 7 rests on ratatui behaviour that nothing in the crate pins
- **Location:** every untrusted `Span`/`Cell` in `ui.rs` (for example `:92`, `:185`, `:191`, `:247`) and `shell/mod.rs:187`.
- **Severity:** info.
- **Who, what input, what happens:** no stripping happens in tacmon. If ratatui changes its grapheme
  filter, or a future widget writes through a path that does not filter, claim 7 breaks silently. A test
  that draws a control-laden string and checks the buffer would pin it.
- **Confidence:** high. Read the source, and ran it (held today).

### F-12 — The build compiles C crypto from a build script
- **Location:** `Cargo.toml:24` (reqwest `rustls` → aws-lc-rs → `aws-lc-sys`).
- **Severity:** info.
- **What it means:** the "no unsafe" claim covers this crate only. The TLS stack runs a cmake/cc build
  and is mostly C. `ring` would be the smaller option if that matters.
- **Confidence:** high (`cargo tree`).

### F-13 — Minor
- **Info, read only.**
  - `state.rs:172`: `last.count += 1` on `u32` panics in debug and wraps in release after 2³² identical
    lines. It is not reachable in practice.
  - `wire.rs:78`: `reverse` is true for any non-null, non-`false` value, including `"false"` and `0`.
  - `feed.rs:57-58`: every later `fast bridge LP` line in the log replaces the identity and the learned
    pool account. A line forged into the provider's stdout or stderr (if it ever prints counterparty
    text verbatim on its own line) would re-point the ETH balance and the claim key.
  - `http.rs:111`: the ETH balance goes through `f64`. That is a display only, but it is money in
    floating point.

## 5. What I did not look at

- I did not run the advisory and licence checks (no `cargo-deny`).
- I did not audit the dependencies' own code beyond the functions named above.
- I did not test the interactive loop (`run`) in a real terminal. Blocking and back-pressure (F-9) are
  from reading.
- I did not observe the colours of the status lights; they were worked out from the code.
