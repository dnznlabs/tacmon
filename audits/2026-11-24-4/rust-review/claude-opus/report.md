# Audit leg report — tacmon

## 1. What could not be done

- `cargo deny` is not installed in this harness, and there is no network for the RustSec database. Claim 10 is therefore only partly checked: the lockfile's sources were read (all 321 packages come from crates.io) but advisories and licences were not evaluated.
- No live board, provider or RPC was available. The HTTP findings come from reading the code and dependency sources (`reqwest` 0.13.5, `hyper-util` 0.1.21), not from traffic.
- Everything else ran: `cargo test --locked --offline` (56 passed), `cargo build --release --locked --offline`, `cargo tree --locked --offline`, the release binary with `--demo --snapshot`, and a scratch crate under `.audit-leg/rt/` that renders hostile strings through ratatui 0.30.2. Its build output was deleted afterwards and the source is kept.

## 2. Files read

All read whole.

| File | What was looked at |
|---|---|
| `SECURITY.md` | threat model, claims 1–10, exclusions |
| `Cargo.toml` | `panic = "abort"`, no `overflow-checks` in release (so wrapping is silent), reqwest features (`rustls` pulls aws-lc-rs / aws-lc-sys) |
| `deny.toml` | bans, sources, licence list (`CDLA-Permissive-2.0` for `webpki-root-certs`, which is not in the tree) |
| `src/config.rs` | load path, `deny_unknown_fields` |
| `src/fmt.rs` | `short` slicing (ASCII-gated), `dur`, `pct` |
| `src/main.rs` | `forbid(unsafe_code)`, `--size` parse, snapshot flow |
| `src/monitors/fast_bridge/adapters/http.rs` | body cap, redirect policy, status handling, board join, MAX_ITEMS, RPC hex parse, proxy behaviour |
| `src/monitors/fast_bridge/adapters/local.rs` | `next_line` cap, child spawn/kill/restart, record size/symlink checks, MAX_RECORDS, RECORD_WAIT |
| `src/monitors/fast_bridge/adapters/memory.rs` | demo data, `now - age` |
| `src/monitors/fast_bridge/adapters/mod.rs` | adapter choice, gas only when `eth_rpc` is set |
| `src/monitors/fast_bridge/adapters/wire.rs` | every reader: units/field/small, terms, swap, want, ad, epoch, kept, log_line, detail |
| `src/monitors/fast_bridge/advisor.rs` | blocker, ladder, standing, competitive fee |
| `src/monitors/fast_bridge/config.rs` | defaults, refresh floors |
| `src/monitors/fast_bridge/feed.rs` | tasks, bounded queue, wake, pool-account watch |
| `src/monitors/fast_bridge/model.rs` | saturating arithmetic, `tac`, `effective_bps` |
| `src/monitors/fast_bridge/mod.rs` | demo scrubbing of pool account, page indexing |
| `src/monitors/fast_bridge/ports.rs` | traits, QUEUE |
| `src/monitors/fast_bridge/state.rs` | apply/staleness flags, log fold counter, my_ad/contested, advice inputs |
| `src/monitors/fast_bridge/ui.rs` | every page, header flow, slicing in `activity`, staleness markers, the Suggestions pane |
| `src/monitors/mod.rs` | monitor list |
| `src/shell/mod.rs` | event loop, unbounded Msg channel, key handling, snapshot wait cap |
| `src/shell/monitor.rs` | Wake coalescing |
| `src/shell/splash.rs` | integer and float arithmetic, `&WORD[..typed]` |
| `src/shell/theme.rs` | colour parse fallback |
| `src/shell/widgets.rs` | `window`, the control-character test |

## 3. Claims

| # | Verdict | Finding or reason |
|---|---|---|
| 1 | held | No code path reads, asks for or stores a key. |
| 2 | held | Only GETs to the provider and board. One POST goes to the configured RPC for `eth_getBalance`, which is a read. No file writes. |
| 3 | held | `#![forbid(unsafe_code)]` at `src/main.rs:4`, with no `build.rs`. The C build of `aws-lc-sys` sits in dependencies (F13). |
| 4 | held, with a gap | No redirect is followed (tested), and `--demo` builds no client. The gap is that a proxy from the environment also carries the loopback `lp_url` (F7). |
| 5 | held | `Command::new(prog).args(args)` at `local.rs:554`, with no shell. |
| 6 | held | rustls with rustls-platform-verifier. No `danger_*` call anywhere, and no other TLS crate in the tree. |
| 7 | held | Experiment: C0, C1, ESC, CSI (U+009B), NUL, BEL, CR, LF and TAB never reach the buffer in Paragraph, Block title, Tabs or Table cells. LF in a Table cell truncates the cell instead (F14). |
| 8 | held, with caveats | Every stated bound is in the code. Per-record strings are not cut, so the total is bounded but very large (F8). A TOCTOU lets a FIFO pin a blocking thread (F9). The fold counter wraps (F11). |
| 9 | broken | The Advisor's standing comes from stale or never-read ads with nothing to say so (F2). A swap record that cannot be parsed vanishes while records show green (F4). Figures are cut at the Suggestions pane's edge (F5). Swap totals stay bold while records are red (F3). |
| 10 | could not tell | No cargo-deny and no network. The lockfile's sources are all crates.io. |

## 4. Findings

### F1 — medium — The Advisor recommends matching any unauthenticated ad, however fake
`src/monitors/fast_bridge/state.rs:247-257`, `advisor.rs:541-545`, `ui.rs:375-386`

- **Who and what:** anyone can post to the board without permission. Post `{"name":"x","claimPub":"03ff","minAmount":"1","maxAmount":"1","feeBps":0,"flatFee":"0"}`. Leave out `available`, which `state.rs:253` excludes only when it is `Some(0)`.
- **What happens:** `others` contains 0, so the standing is `Dearer { cheapest: 0, matching: None }`. The Advisor prints "The cheapest other charges 0.00%, under every fee compared here." A fake ad at 0.10% instead makes it print "A fee of 0.10% would match it" and star that row.
- **What is not checked:** whether the ad's size range overlaps anything waiting or the provider's own range, whether `available` is known, and whether the ad is plausible at all.
- **Why it matters:** this is attacker goal 1. It steers the operator to cut fees on the say-so of an unsigned post. `SECURITY.md` lists only "an ad posted under your key" as out of scope, so this case is not covered.
- **Confidence:** high. Read only, and the arithmetic was traced through `advise`.

### F2 — medium — Advisor standing drawn from stale or never-read ads with no marker
`src/monitors/fast_bridge/state.rs:247-257`, `ui.rs:360`, `ui.rs:375-376`, triggered through `adapters/http.rs:345`

- **Who and what:** anyone posts more than 5,000 ads (`MAX_ITEMS`). From then on the `/ads` list is refused on every read and `ads_ok` stays false.
- **Board never read:** if this has been true since start, `self.ads` is empty and the Advisor says "No other provider with TAC to lend is on the board to compare with". That is a false statement drawn as fact.
- **Board read before:** the last standing ("That is the cheapest on the board … Keep it.") stays up indefinitely.
- **No marker:** the Advisor table's title checks `board_ok` (wants) and never `ads_ok`. Wants are fetched separately, so they can stay fine while ads fail.
- **Why it matters:** this breaks claim 9.
- **Confidence:** high. Read only.

### F3 — low — Swap reads fail permanently after about 6,700 swaps, and stale totals stay bold
`src/monitors/fast_bridge/adapters/local.rs:603-607`, `ui.rs:96-98`, `state.rs:198-200`

- **The limit:** `seen` counts every directory entry, including the `.bak` and `.tmp` files the comment at `local.rs:609` says sit beside each record. With three files per swap, the 20,000 limit is reached at about 6,667 swaps. After that every pass bails.
- **What it shows:** `swaps_ok` is false forever. `swaps_known()` is still true because the old list is not empty, so the header keeps drawing received, incoming and earned in bold from the last good read. The Overview table and its deadline countdowns run from old records. The only sign is a red "records" light.
- **Who:** ordinary growth gets there, and so does any counterparty opening swaps.
- **Confidence:** high on the counting. The three-files-per-swap figure comes from the code comment.

### F4 — low — An unparsable swap record silently drops an open swap from the totals and the Overview
`src/monitors/fast_bridge/adapters/local.rs:614-617`, `wire.rs:14`, `wire.rs:57-58`

- **What fails:** `units` accepts only a decimal string or a JSON number that fits in `u64`. A float (`2.5e10`), a number above `u64`, a hex string or a broken JSON file all give `None`, and the record is skipped without any signal.
- **What it shows:** `swaps_ok` stays true, so records are green while an open swap with funds at stake and a deadline is missing from "incoming" and from the Overview.
- **Who:** `SECURITY.md` says the records carry counterparty-supplied values, though whether the provider would persist such a value is unknown. The same silent drop applies to records that time out (`RECORD_WAIT`) or go over 1 MiB.
- **Confidence:** medium. Read only.

### F5 — low — The Suggestions pane cuts figures at its right edge
`src/monitors/fast_bridge/ui.rs:426`, a `Paragraph` with no wrap

- **What happens:** at `--size 127x30 --demo` the pane reads "You have 1,84" (really 1,840.00), "over your max of 1,0" (really 1,000.00) and "the next is 1." (really 1.60%).
- **Why it matters:** the header's `flow` exists precisely so that "a figure cut off … reads as a smaller figure" does not happen, but this pane has no such protection.
- **Confidence:** high. Ran it.

### F6 — low — Overlapping boards make the provider's own ad look forged
`src/monitors/fast_bridge/adapters/http.rs:342-347`, `state.rs:221-233`

- **What happens:** lists from several boards are concatenated without removing duplicates. If two configured boards return the same ad, `my_ads()` counts 2 and `my_ad()` counts 1, so the header shows "an ad that is not yours uses your key" in red.
- **While terms are unknown:** `my_ad()` is `None`, so "on board" shows ✕ and the name falls back to "provider".
- **Competitors too:** the duplicate copies also count twice on the Market page.
- **Confidence:** high on the logic. Whether boards mirror each other in practice is unknown.

### F7 — low — An environment proxy also carries the loopback provider URL
`src/monitors/fast_bridge/adapters/http.rs:262`, behaviour in `hyper-util-0.1.21/src/client/proxy/matcher.rs`

- **What happens:** reqwest reads `HTTP_PROXY` and `ALL_PROXY` from the environment, and the matcher does not exempt loopback unless `NO_PROXY` names it. With a proxy set, `GET http://127.0.0.1:8787/info` and `/health` go to the proxy. The proxy then connects to *its own* 127.0.0.1:8787 and answers.
- **Effect:** health and terms come from another machine (or an error) and are drawn as the provider's. The proxy operator also learns the polling.
- **Why it is a finding:** claim 4 allows proxies, but the claims never say a local read can leave the machine.
- **Confidence:** medium. Read the dependency source, did not run with a proxy.

### F8 — low — Per-record strings are not bounded, so the total memory bound is very large
`src/monitors/fast_bridge/adapters/wire.rs:52-59`, `wire.rs:119-124`, `state.rs:56`

- **Swap records:** `id`, `phase` and `lockTxid` are kept whole, up to about 1 MiB each record. Times 20,000 records, that is about 20 GB held in `State.swaps`, with a second copy in flight in the `Event`.
- **Log identity:** the `Started` identity fields (`bitcoinAddress`, `claimPub`, `poolAccount`) are not passed through `kept()`. Each can be up to 64 KiB, and 5,000 such lines stay in the log ring.
- **Why it matters:** claim 8 is literally met, since each piece is bounded, but the product of the bounds is not a useful one.
- **Confidence:** high. Read only.

### F9 — low — A TOCTOU in `record()` lets a FIFO pin a blocking thread, and a symlink pass as a regular file
`src/monitors/fast_bridge/adapters/local.rs:502-507`

- **The race:** `symlink_metadata` is checked first, then the path is `open`ed by name.
- **FIFO:** if a FIFO replaces the file in between, `File::open` blocks inside a tokio blocking-pool thread. The 2-second `timeout` drops the future but not the thread, so one thread can leak per pass, up to the pool's 512, after which every `tokio::fs` call stalls.
- **Symlink:** a symlink swapped in passes the post-open `is_file()` check, so any file this user can read gets parsed as a swap.
- **Fix:** open with `O_NOFOLLOW|O_NONBLOCK`, then check with `fstat`.
- **Who:** needs write access to `swaps/`, which is provider-level.
- **Confidence:** medium. Read only.

### F10 — info — ETH balance in floating point
`src/monitors/fast_bridge/adapters/http.rs:376`, `ports.rs:204`. The `u128` wei value is converted to `f64` and only displayed. Precision loss is far below the 4 places shown.

### F11 — info — The fold counter wraps in release
`src/monitors/fast_bridge/state.rs:182`. `last.count += 1` on a `u32` panics in debug and wraps in release (no `overflow-checks`) after 2³² identical lines in a row. Not practical to reach.

### F12 — info — A future log timestamp makes log capacity look fresh for ever
`src/monitors/fast_bridge/state.rs:203`, `wire.rs:84-94`. `saturating_sub` shows "0s ago" for a `ts` in the future. `epoch` also accepts day 31 for every month. The provider controls `ts`.

### F13 — info — The build includes C code and a build script
The reqwest `rustls` feature enables `__rustls-aws-lc-rs`, which pulls in `aws-lc-sys` (C, cmake or cc build script). Claim 3 covers only this crate.

`deny.toml` also allows `CDLA-Permissive-2.0` for `webpki-root-certs`, which `cargo tree` shows is not in the tree. That is a stale exception.

### F14 — info — A newline in a Table cell hides the rest of the cell
`src/monitors/fast_bridge/ui.rs:290`. `Cell::from(String)` splits on `\n`, and a one-line row shows only the first part. An ad named `"cheap\n(scam)"` shows "cheap". Confirmed with the experiment in `.audit-leg/rt/`. No control character reaches the terminal.

### F15 — info — What leaves the machine is not pinned
- `feed.rs:542-544`: every later "fast bridge LP" line re-sets the pool account sent to the RPC.
- `http.rs:262`: the board sees `User-Agent: tacmon` every 20 s from the operator's address.

### F16 — info — The demo can underflow on a wrong clock
`src/monitors/fast_bridge/adapters/memory.rs:81-82`. `now - age` underflows if the clock reads under 21,000 s (and `fmt::now` returns 0 when the clock is before the epoch). Wraps in release. Trusted input only.

## 5. What I did not look at

- The ratatui and crossterm output path past the buffer, meaning how crossterm writes cell symbols. The experiment covered only the buffer.
- The behaviour of `journalctl` itself, and how `log_cmd`'s process group is handled when it is killed.
- Header and body limits in hyper, and h1 parsing edge cases. Not exercised.
- How rustls-platform-verifier on Linux handles `SSL_CERT_FILE` and `SSL_CERT_DIR` (operator environment, trusted).
