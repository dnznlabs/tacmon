# Audit 2026-11-24-4

## 1. What this is

This is a self-review by the project's own developers using language models, not an audit by an outside firm. It
reconciles a set of deterministic checks and two adversarial reviews (legs), run in separate harnesses without
sight of each other, over commit `09fb20e`. Findings are carried as the legs reported them; nothing here is fixed.

## 2. Provenance

```
audit date        2026-11-24-4
code commit       09fb20eb663fdc1375b1f9b3d862ededc2bc48c7  (scope dirty: no · tree dirty: no)
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
independence      2 legs, run without sight of each other
leg 1             codex codex-cli 0.172.0 · requested gpt-5.6-sol → observed gpt-5.6-sol · effort high · 260s wall · tokens 91,496 · 25 of 25 scope files named in the report · tree unchanged
leg 2             claude 2.1.331 (Claude Code) · requested opus → observed claude-opus-5-5 · 326s wall · tokens 2245684 · 25 of 25 scope files named in the report · tree unchanged
```

## 3. Checks

From [`checks/SUMMARY.txt`](checks/SUMMARY.txt).

| Result | Check | What it says |
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
| REPORT | panic-surface | 85 lint hits where a panic or a silent wrap could start ([`panic-surface.txt`](checks/panic-surface.txt)). This is a count, not a pass. |

No check failed, so no check contributes a finding.

## 4. Findings

Read before deduplication: **4 findings** from `codex-gpt-5.6-sol` (F-01 to F-04) and **16 findings** from
`claude-opus` (F1 to F16). They reduce to **17 distinct issues**: three were found by both legs.

**A note on line numbers.** Several of `claude-opus`'s citations point past the end of the file at this commit
(`local.rs` is 203 lines, `http.rs` 229, `feed.rs` 157, `advisor.rs` 221), yet name code that does exist. Each
such entry below gives the line where the described code is at `09fb20e`, matched on mechanism, with the leg's
own citation after it. Its citations into `state.rs`, `ui.rs`, `wire.rs` and `memory.rs` match the tree.

### 1. The Advisor compares against stale or never-read ads with no marker — medium

`src/monitors/fast_bridge/ui.rs:360`, `src/monitors/fast_bridge/state.rs:247-257`

- **Found by:** both legs (`codex-gpt-5.6-sol` F-02, `claude-opus` F2).
- **Severity:** medium from both.
- **What:** ads and wants are separate reads with separate flags (`ads_ok`, `board_ok`). `State::advice` uses the
  retained `self.ads` whatever `ads_ok` says, and the Advisor table's title checks only `board_ok`. When `/wants`
  answers and `/ads` fails, the standing ("That is the cheapest on the board … Keep it.", or a matching fee) is
  drawn from the last ads as current. `claude-opus` adds that if `/ads` has never answered, the page states as fact
  that no other provider is on the board, and that a board can force this permanently by holding more than
  `MAX_ITEMS` (5,000) ads, since an over-long list is refused (`http.rs:104`; leg cites `http.rs:345`).
- **Cost:** breaks claim 9, and lands on attacker goal 1: a fee decision from a false market view.

### 2. A partial multi-board answer is reported as a complete one — medium

`src/monitors/fast_bridge/adapters/http.rs:99-107`

- **Found by:** `codex-gpt-5.6-sol` alone (F-01).
- **Severity:** medium.
- **What:** `HttpBoard::list` skips any board that errors, times out, returns bad JSON or more than `MAX_ITEMS`
  items, and returns `Ok` if any one other board answered. There is no partial-read signal, so the source shows as
  answering. With two boards, one answering `{"wants":[]}` and one failing, the result is a successful empty
  market. The doc comment at `http.rs:98` states the skip is intended; the leg's point is that the result
  contradicts claim 9.
- **Cost:** one failing or hostile board removes its share of demand or supply from totals and fee comparisons
  without any failure indication.

### 3. The Advisor matches any unauthenticated ad, however implausible — medium

`src/monitors/fast_bridge/state.rs:247-257`, `src/monitors/fast_bridge/advisor.rs:98-100` (leg cites
`advisor.rs:541-545`), `src/monitors/fast_bridge/ui.rs:375-386`

- **Found by:** `claude-opus` alone (F1).
- **Severity:** medium.
- **What:** `others` excludes only the provider's own ad and ads with `available == Some(0)`. An ad posted with
  `feeBps: 0`, `flatFee: "0"`, a 1-unit size range and no `available` makes the cheapest other fee 0, and the
  Advisor says the provider is dearer than it; a fake ad at 0.10% makes it name and star that fee. Size overlap,
  a known `available` and plausibility are not checked.
- **Cost:** attacker goal 1. `SECURITY.md` excludes only an ad posted under the operator's own key, so this case is
  not covered by an exclusion. The leg does not map it to a numbered claim.

### 4. Check-then-open race in swap record reading — low

`src/monitors/fast_bridge/adapters/local.rs:32-35` (`codex-gpt-5.6-sol` cites `:31`; `claude-opus` cites
`:502-507`)

- **Found by:** both legs (`codex-gpt-5.6-sol` F-03, `claude-opus` F9).
- **Severity:** low from both.
- **What:** `symlink_metadata(p)` rejects a link, then `File::open(p)` opens the path by name and follows whatever
  is there. A symlink swapped in between passes the post-open file-type check, so any regular file up to 1 MiB
  readable by tacmon is read and parsed as a swap. `claude-opus` adds a second effect: a FIFO swapped in makes
  `File::open` block a tokio blocking-pool thread; the 2 s `RECORD_WAIT` timeout drops the future but not the
  thread, so threads can leak per pass up to the pool's limit, after which `tokio::fs` stalls.
- **Cost:** needs write access to `data_dir/swaps`. The legs disagree on what it breaks (see § 5): `codex-gpt-5.6-sol`
  says claims 1 and 8, `claude-opus` says neither outright.

### 5. The repeated-log-line counter overflows — low

`src/monitors/fast_bridge/state.rs:182`

- **Found by:** both legs (`codex-gpt-5.6-sol` F-04, `claude-opus` F11).
- **Severity:** `codex-gpt-5.6-sol` low, `claude-opus` info. **Adopted: low.** Both agree it needs 2³² identical
  consecutive lines and is not practical to reach. Low is adopted because the reason `codex-gpt-5.6-sol` gives is
  a stated property and not likelihood: in a debug build it is a panic from untrusted input, which claim 8 says
  cannot happen, and in release (no `overflow-checks`) it wraps silently, which the project's conventions forbid.
- **What:** `last.count += 1` on a `u32` when consecutive entries coalesce.
- **Cost:** a debug-build panic, or in release a wrapped count that understates the repetition.

### 6. Swap reads fail permanently past about 6,700 swaps, with stale totals still bold — low

`src/monitors/fast_bridge/adapters/local.rs:132-139` (leg cites `:603-607`), `src/monitors/fast_bridge/ui.rs:96-98`,
`src/monitors/fast_bridge/state.rs:198-200`

- **Found by:** `claude-opus` alone (F3).
- **Severity:** low.
- **What:** `seen` counts every directory entry, including the `.bak` and `.tmp` files the comment at `local.rs:139`
  says sit beside each record, against `MAX_RECORDS` (20,000). At three files a swap, every pass bails from about
  6,667 swaps. `swaps_ok` stays false, but `swaps_known()` stays true on the old list, so the header keeps drawing
  received, incoming and earned in bold from the last good read; the only sign is the red "records" light.
- **Cost:** reached by ordinary growth. The three-files figure comes from the code comment, not from a provider.

### 7. An unparsable swap record vanishes silently — low

`src/monitors/fast_bridge/adapters/local.rs:144` (leg cites `:614-617`), `src/monitors/fast_bridge/adapters/wire.rs:14`,
`wire.rs:57-58`

- **Found by:** `claude-opus` alone (F4).
- **Severity:** low, confidence medium.
- **What:** `units` accepts only a decimal string or a JSON number fitting `u64`. A float, an over-`u64` number, a
  hex string or broken JSON yields `None` and the record is skipped, while `swaps_ok` stays true. The same silent
  drop applies to records that time out or exceed 1 MiB.
- **Cost:** an open swap with funds and a deadline is missing from "incoming" and the Overview while records show
  green. Whether the provider would ever persist such a value is unknown.

### 8. The Suggestions pane cuts figures at its right edge — low

`src/monitors/fast_bridge/ui.rs:426`

- **Found by:** `claude-opus` alone (F5), by running `--size 127x30 --demo`.
- **Severity:** low.
- **What:** a `Paragraph` with no wrap. Observed: "You have 1,84" for 1,840.00, "over your max of 1,0" for 1,000.00,
  "the next is 1." for 1.60%.
- **Cost:** a cut figure reads as a smaller one, which the header's `flow` exists to prevent.

### 9. Overlapping boards make the provider's own ad look forged — low

`src/monitors/fast_bridge/adapters/http.rs:99-107` (leg cites `:342-347`), `src/monitors/fast_bridge/state.rs:221-233`

- **Found by:** `claude-opus` alone (F6).
- **Severity:** low.
- **What:** board lists are concatenated without deduplication. If two boards return the same ad, `my_ads()` counts
  2 against `my_ad()`'s 1 and the header shows "an ad that is not yours uses your key" in red; competitors are
  counted twice on the Market page.
- **Cost:** a false alarm. Whether boards mirror each other in practice is unknown.

### 10. An environment proxy also carries the loopback provider URL — low

`src/monitors/fast_bridge/adapters/http.rs:21` (leg cites `:262`)

- **Found by:** `claude-opus` alone (F7), from reading `hyper-util` 0.1.21's proxy matcher; not run.
- **Severity:** low, confidence medium.
- **What:** with `HTTP_PROXY` or `ALL_PROXY` set and `NO_PROXY` not naming loopback, `GET http://127.0.0.1:8787/info`
  and `/health` go to the proxy, which connects to its own 127.0.0.1:8787.
- **Cost:** health and terms from another machine drawn as the provider's, and the polling disclosed to the proxy.
  Claim 4 permits proxies but does not say a local read can leave the machine.

### 11. Per-record strings are not bounded — low

`src/monitors/fast_bridge/adapters/wire.rs:52-59`, `wire.rs:119-124`, `src/monitors/fast_bridge/state.rs:56`

- **Found by:** `claude-opus` alone (F8).
- **Severity:** low.
- **What:** a swap's `id`, `phase` and `lockTxid` are kept whole (up to about 1 MiB a record, times 20,000 records,
  plus an in-flight copy in the event). The `Started` identity fields are not passed through `kept()`, so each can
  be 64 KiB, across 5,000 retained log lines.
- **Cost:** claim 8 is met piecewise, but the product of its bounds is around 20 GB.

### 12. ETH balance in floating point — info

`src/monitors/fast_bridge/adapters/http.rs:135` (leg cites `:376`), `ports.rs:204`. `claude-opus` alone (F10). Wei
converted to `f64` for display; the loss is below the four places shown.

### 13. A future log timestamp keeps capacity looking fresh — info

`src/monitors/fast_bridge/state.rs:203`, `wire.rs:84-94`. `claude-opus` alone (F12). `saturating_sub` shows "0s ago"
for a future `ts`; `epoch` accepts day 31 in every month. The provider controls `ts`.

### 14. The build includes C code and a build script; a stale licence exception — info

`Cargo.toml`, `deny.toml`. `claude-opus` alone (F13). The reqwest `rustls` feature pulls in `aws-lc-sys`, a C build;
claim 3 covers only this crate. `deny.toml` allows `CDLA-Permissive-2.0` for `webpki-root-certs`, which is not in the
tree.

### 15. A newline in a table cell hides the rest of it — info

`src/monitors/fast_bridge/ui.rs:290`. `claude-opus` alone (F14), confirmed with the scratch crate in
[`rust-review/claude-opus/rt/`](rust-review/claude-opus/rt/). An ad named `"cheap\n(scam)"` shows "cheap". No control
character reaches the terminal.

### 16. What leaves the machine is not pinned — info

`src/monitors/fast_bridge/adapters/wire.rs:119` (leg cites `feed.rs:542-544`), `http.rs:21` (leg cites `:262`).
`claude-opus` alone (F15). Every later "fast bridge LP" log line resets the pool account sent to the RPC; the board
sees `User-Agent: tacmon` every 20 s.

### 17. The demo can underflow on a wrong clock — info

`src/monitors/fast_bridge/adapters/memory.rs:81-82`. `claude-opus` alone (F16). `now - age` underflows when the
clock reads under 21,000 s, and wraps in release. Trusted input only.

## 5. Claims

| # | Claim (short) | `codex-gpt-5.6-sol` | `claude-opus` | Stands? |
|---|---|---|---|---|
| 1 | No private key read | broken (finding 4: a raced symlink can read a key file) | held | **Disagreement.** Same mechanism, read differently: one leg takes "cannot read a key" literally, the other found no path that *asks for* one. |
| 2 | No writes, reads only | held | held | Stands |
| 3 | No `unsafe` | held | held (notes the C in `aws-lc-sys`, finding 14) | Stands; also the `unsafe` check passed |
| 4 | Outbound only to configured hosts; no redirects; demo makes none | held (its redirect tests could not run) | held, with a gap (finding 10) | Stands, with finding 10 open against its wording |
| 5 | Only `log_cmd` started, no shell | held | held | Stands |
| 6 | rustls, verification cannot be turned off | held | held | Stands |
| 7 | Untrusted text cannot control the terminal | held | held (experiment; finding 15 is truncation, not control) | Stands |
| 8 | No panic, hang or unbounded memory from untrusted input | broken (findings 4, 5) | held, with caveats (findings 4, 5, 11) | **Disagreement.** Both legs found the same two issues; they differ on whether they break the claim. |
| 9 | A failed read shows as failed; stale never drawn as fresh | broken (findings 1, 2) | broken (findings 1, 6, 7, 8) | **Does not stand** |
| 10 | crates.io only, no advisory, permissive licences | could not tell | could not tell | Neither leg could run `cargo-deny`. The `advisories`, `licences` and `sources` checks passed in this run. |

## 6. Agreement and disagreement

**Every leg found:** the stale-ads Advisor (finding 1), the check-then-open race (finding 4) and the counter
overflow (finding 5). They agree on the mechanism of all three, and on severity for two. Both broke claim 9.

**Only `codex-gpt-5.6-sol` found:** the partial multi-board answer (finding 2). `claude-opus` read the board join
and reported a different defect in it (finding 9) but not this one. The code matches the description and its own
doc comment confirms the behaviour, so this looks like a miss by `claude-opus`, not a false positive; whether the
intended behaviour is acceptable under claim 9 is a judgement for triage.

**Only `claude-opus` found:** findings 3 and 6 to 17, of which six are info. The single-leg low and medium
findings name specific lines and mechanisms, and finding 8 was observed in a run. None looks like a false positive
on its face; findings 7, 9 and 10 carry the leg's own stated uncertainty about whether the trigger occurs in
practice. The large difference in count follows the difference in effort: `claude-opus` used roughly 24 times
the tokens and also built and ran code.

**Disagreement on claims:** 1 and 8, both over issues the two legs found alike (§ 5).

## 7. Legs

**[`codex-gpt-5.6-sol`](rust-review/codex-gpt-5.6-sol/)** read all 25 scope files whole and reported 4 findings
(two medium, two low). It says six HTTP-adapter tests could not bind a loopback socket in its harness
(`Operation not permitted`) and the other 50 of 56 passed, so the redirect, status, timeout and body-limit paths
were checked by reading those tests, not by running them. It says `cargo-deny` was not installed and there was no
network, so the advisory and licence half of claim 10 was not checked.

**[`claude-opus`](rust-review/claude-opus/)** read all 25 scope files whole and reported 16 findings (two medium,
seven low, seven info). It ran `cargo test` (56 passed), a release build, `cargo tree`, the release binary with
`--demo --snapshot`, and a scratch crate rendering hostile strings through ratatui 0.30.2, kept in
[`rt/`](rust-review/claude-opus/rt/). It says `cargo-deny` and the network were unavailable, so claim 10 was only
partly checked, and that no live board, provider or RPC was available. It lists as not examined: crossterm's output
path past the buffer, `journalctl` and `log_cmd` process-group handling, hyper's header and body limits, and
rustls-platform-verifier's handling of `SSL_CERT_FILE` and `SSL_CERT_DIR`. Many of its line numbers in `local.rs`,
`http.rs`, `feed.rs` and `advisor.rs` do not match this commit (§ 4).

There is no `audits/KNOWN_ANSWER.md`, so this run was not graded.
