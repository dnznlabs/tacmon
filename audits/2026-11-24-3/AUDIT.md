# Security review 2026-11-24-3: tacmon

## 1. What this is

This is a self-review by tacmon's own developers using language models. It is not an audit by an outside firm.
It combines the project's checks that have one right answer with two adversarial reviews of the files in scope,
run without sight of each other. Where both legs found the same issue it was found twice independently; a
finding only one leg reports has not been confirmed by the other.

## 2. Provenance

```
audit date        2026-11-24-3
code commit       35168f3771acce15e6682fb0a16db3a5ba75f93a  (scope dirty: no · tree dirty: no)
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
leg 1             codex codex-cli 0.172.0 · requested gpt-5.6-sol → observed gpt-5.6-sol · effort high · 330s wall · tokens 92,266 · 25 of 25 scope files named in the report · tree unchanged
leg 2             claude 2.1.331 (Claude Code) · requested opus → observed claude-opus-5-5 · 402s wall · tokens 2887776 · 25 of 25 scope files named in the report · tree unchanged
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
| REPORT | panic-surface | 84 lint hits where a panic or a silent wrap could start ([`panic-surface.txt`](checks/panic-surface.txt)) |

No check failed. `panic-surface` is a count, not a result: 84 hits. Two of them are findings below
(`state.rs:182`, finding 10; `http.rs:20`, finding 13).

## 4. Findings

Read before deduplication: **6 findings from `codex-gpt-5.6-sol`**, **13 from `claude-opus`** (its F13 bundles
five smaller items under one heading). After merging, 17 distinct entries. Severity words are the legs' own;
where they differ the adopted one is stated with its reason. Ordering puts the medium findings first, and within
a severity follows SECURITY.md's attacker goals (false numbers, then taking the monitor down).

Location note: several `claude-opus` line numbers fall past the end of the file they name (`local.rs:325-329`
in a 157-line file; `wire.rs:588`, `596-603`, `617-627` in a 295-line file). The entries below give the location
of the code each one describes, identified by the function or expression the leg quotes.

### 1. A failed `/health` status can be drawn as healthy — `src/monitors/fast_bridge/adapters/http.rs:54-59`, `:75-77` — medium

- **Found by:** `codex-gpt-5.6-sol` alone (its finding 4).
- **What:** `get_or_error_body` rejects only 1xx and 3xx, so a 4xx/5xx body is parsed and `health` returns
  `Ok`. A 500 answering `{"ok":true}` becomes `Some(true)` in state and a green light in the header. The test
  `health_is_read_from_the_body_of_a_failed_status` (`http.rs:177`) confirms a 503 body is read on purpose; the
  leg's point is that `ok: true` taken from a failed status is a failed read shown as a healthy one.
- **Cost:** breaks claim 9 directly (attacker goal 1). Who controls the bytes: whatever answers on the provider port.

### 2. Free TAC that nothing has reported is drawn as 0.00 and drives the advice — `src/monitors/fast_bridge/state.rs:193-198`, `ui.rs` Advisor and Demand pages, `advisor.rs:13` — medium

- **Found by:** `claude-opus` alone (F2).
- **What:** `State::free()` returns `0` when `free_known()` is false. The header checks `free_known` and draws a
  dash; the Advisor page prints "You have 0.00 free", and `State::blocker` passes the same `0` to
  `advisor::blocker`, so every in-range want on the Demand page is marked "not enough free TAC". Happens with no
  attacker: `/info` without `available` and no capacity line yet (fresh start, `log_cmd` failing, no journald access).
- **Cost:** breaks claim 9 and the repo rule that a failed read is `None`, never a made-up zero.

### 3. Swap-record size check races the read, and follows symlinks — `src/monitors/fast_bridge/adapters/local.rs:113-117` — medium

- **Found by:** both legs (`codex-gpt-5.6-sol` finding 3; `claude-opus` F10, and its claim 1 row).
- **Severity:** `codex-gpt-5.6-sol` medium, `claude-opus` low. **Adopted: medium.** `claude-opus` lowered it
  because it "needs write access to the directory", but SECURITY.md lists the swap records as not trusted (the
  provider controls them), so that access is inside the threat model, and the race defeats the 1 MiB bound claim 8
  states outright.
- **What:** `tokio::fs::metadata` (follows links) checks `is_file` and `len <= MAX_RECORD`; `tokio::fs::read` then
  reopens the path and reads it whole with no limit. Swapping in a larger file, a FIFO, or a link to `/dev/zero`
  between the two makes the read unbounded or blocking. Separately, a `.json` symlink to any readable regular file
  ≤ 1 MiB is read in full.
- **Cost:** stalls or exhausts the records feed (claim 8). On the symlink half the legs disagree about claim 1; see § 5.

### 4. A 4 MiB board answer parses to roughly 0.4–0.8 GB — `src/monitors/fast_bridge/adapters/http.rs:37`, `:96-97` — medium

- **Found by:** `claude-opus` alone (F1).
- **What:** the 4 MiB cap is on raw bytes; `serde_json::from_slice` builds a `Value` tree, `MAX_ITEMS` is checked
  only after parsing, and the surviving list is deep-cloned. Measured by the leg: `{"ads":[{"":0},…]}` at 4,194,297
  bytes (599,184 objects) peaked at 792 MB VmHWM. Bulk can sit inside a few items to stay under `MAX_ITEMS`.
  `/ads` and `/wants` are fetched together each poll, per board.
- **Cost:** OOM on a small host, with `panic = "abort"` (claim 8, goal 3). Reachability depends on whether the real
  board stores large ads with extra fields, which the leg could not test.

### 5. Unbounded log and wake queues — `src/monitors/fast_bridge/adapters/local.rs:53`, `feed.rs:47`, `shell/monitor.rs:18` — medium

- **Found by:** `codex-gpt-5.6-sol` alone (finding 1). `claude-opus` noted "unbounded channels" while reading
  `feed.rs` but raised no finding.
- **What:** every nonblank log line becomes a `LogEntry` on an unbounded channel, is relayed on a second
  unbounded channel, and sends a wake on the shell's unbounded channel. `MAX_LINE` bounds one line and `MAX_LOG`
  bounds state, but neither bounds the queues. A producer faster than the drawing path grows memory without limit.
- **Cost:** claim 8, goal 3. From code reading; not run.

### 6. Swap-record count is unbounded — `src/monitors/fast_bridge/adapters/local.rs:104` — medium

- **Found by:** `codex-gpt-5.6-sol` alone (finding 2).
- **What:** every `.json` entry ≤ 1 MiB is read and every parseable swap pushed, with no record-count or total-byte
  limit; the vector is sorted, held in state, and the UI allocates a row per record.
- **Cost:** each refresh costs time and memory proportional to the store (claim 8). From code reading; not run.

### 7. A capacity figure from the log can carry no age, or a wrong one — `src/monitors/fast_bridge/adapters/wire.rs:84` (`epoch`), `state.rs:202`, `:206`, `ui.rs:75`, `:86` — low

- **Found by:** both legs for the future-timestamp case (`codex-gpt-5.6-sol` finding 5; `claude-opus` F6).
  `claude-opus` alone for the rest.
- **Severity:** both low.
- **What:** a `ts` in the future gives `now.saturating_sub(at) == 0`, drawn "0s ago" for an arbitrarily old figure.
  A missing or unparseable `ts` gives `at = None`, drawn as "· log" / "(from the log)" with no age at all, which the
  `--demo` snapshot shows. A `ts` with a non-UTC offset is misread by the size of the offset.
- **Cost:** breaks claim 9's "gives its age".

### 8. Swap-record totals show 0.00 before the records are ever read — `src/monitors/fast_bridge/ui.rs:91-93`, `state.rs:209-218` — low

- **Found by:** `claude-opus` alone (F3).
- **What:** received, incoming and fees earned are drawn as bold `0.00` until a read succeeds; after a later failure
  the last totals stay with no stale mark. Only the separate records light says otherwise.
- **Cost:** claim 9.

### 9. The Market page marks a forged ad under the provider's key as "yours" — `src/monitors/fast_bridge/ui.rs:274` — low

- **Found by:** `claude-opus` alone (F4).
- **What:** state refuses a contested ad as `my_ad` and raises `ad_contested`, but the Market page tests only
  `a.claim_pub == st.id.claim_pub` and draws every match with the "▶" mark, impostor included; with the real ad
  absent the forged row is the only one marked.
- **Cost:** the screen shows "your" ad with terms the provider never set, contradicting the header warning.
  SECURITY.md excludes removing such an ad, not presenting it as the operator's.

### 10. Repeated-line count is an unchecked `u32` increment — `src/monitors/fast_bridge/state.rs:182` — low

- **Found by:** `codex-gpt-5.6-sol` alone (finding 6). Also listed by the `panic-surface` check.
  `claude-opus` stated that "the arithmetic saturates" under claim 8.
- **What:** `last.count += 1` on consecutive identical log messages wraps to zero in release (no `overflow-checks`)
  after 2³² repeats, or panics with overflow checks on.
- **Cost:** understated repeat count; needs billions of lines, so less practical than findings 4–6.

### 11. Unauthenticated demand steers the Advisor; over 5,000 wants blinds the Demand page — `src/monitors/fast_bridge/advisor.rs:41-77`, `adapters/wire.rs:63-69` (`want`), `adapters/http.rs:96` — low

- **Found by:** `claude-opus` alone (F5), which calls it a gap in the claims rather than a code bug.
- **What:** wants carry no identity and are not de-duplicated. Uncapped wants (`max_fee_bps: None`) push the "best"
  fee to the top of the ladder; capped ones push it down; huge ones drive the "N more TAC" lines. Posting more than
  `MAX_ITEMS` wants makes every `/wants` read fail, by design, for as long as the flood lasts.
- **Cost:** SECURITY.md's exclusions name forged ads, not forged demand.

### 12. One board's failure is silent with several boards; mirrored entries count twice — `src/monitors/fast_bridge/adapters/http.rs:94-97` — low

- **Found by:** `claude-opus` alone (F7).
- **What:** a board that errors, answers non-2xx or floods is skipped with `continue`; the joined list is still
  `Ok`, so `board_ok` stays true over a partial market. An entry on two boards is counted twice in demand totals,
  Advisor volumes and `ad_contested`. The default config has one board.
- **Cost:** claim 9 (a failed read not shown as failed) in multi-board configs.

### 13. The HTTP client is built with `expect` — `src/monitors/fast_bridge/adapters/http.rs:20` — low

- **Found by:** `claude-opus` alone (F8). Also listed by the `panic-surface` check.
- **What:** `rustls-platform-verifier` loads system roots eagerly; with none (minimal container,
  `SSL_CERT_FILE=/dev/null`) `build()` errors and `expect("http client")` aborts, even if every URL is plain HTTP.
- **Cost:** a startup panic instead of an error message. The environment is trusted; not reachable from untrusted input. Not run.

### 14. A configured proxy also carries requests to the local provider; claim 4 names too few variables — `src/monitors/fast_bridge/adapters/http.rs:19-21` — low

- **Found by:** `claude-opus` alone (F9).
- **What:** reqwest's env proxy matcher does not exempt loopback, so with `HTTP_PROXY` set and no `NO_PROXY` the
  `/info` and `/health` GETs go through the proxy, whose operator can answer them. The matcher also reads lowercase
  variables and `NO_PROXY`, and is disabled when `REQUEST_METHOD` is set; claim 4 names only the three uppercase ones.
- **Cost:** a claim-wording gap, plus a proxy that can answer for the provider. From dependency source; not run.

### 15. `--wait` with a huge value panics — `src/shell/mod.rs:86` — info

- **Found by:** `claude-opus` alone (F11). Ran: `tacmon --demo --snapshot overview --wait 18446744073709551615`
  aborts with "overflow when adding duration to instant", exit 134. The CLI is trusted. The leg also notes
  `shell/mod.rs:188` (`header_height(..) + 2` in `u16`) as unreachable in practice.

### 16. The TLS path compiles C through `aws-lc-sys` — `Cargo.toml:24` — info

- **Found by:** `claude-opus` alone (F12). reqwest's `rustls` feature pulls in `aws-lc-rs`/`aws-lc-sys`, whose build
  script compiles C. Claim 3 covers only this crate and stays true; the leg notes it says nothing of this native dependency.

### 17. Smaller ways the screen can mislead — info

- **Found by:** `claude-opus` alone (F13), five items:
  - the ETH balance is converted through `f64` (`http.rs:127`), display only;
  - a failed health read is drawn as unknown "○" rather than failed (`state.rs:110`, `widgets.rs:439`); related
    to, but distinct from, finding 1;
  - log lines fold on `msg` and level alone (`state.rs:176-184`), so distinct events show as `×2` with the last detail;
  - a missing or non-integer deadline reads as 0 and draws "—" (`wire.rs:55`, `ui.rs:158`), so the open-swap
    deadline warning never fires;
  - each `log_cmd` restart replays `-n 2000` lines (`local.rs:55-86`).

## 5. Claims

| # | Claim (short) | `codex-gpt-5.6-sol` | `claude-opus` | Stands? |
|---:|---|---|---|---|
| 1 | No private key read, requested or stored | broken: a `.json` symlink to any readable file ≤ 1 MiB is read (finding 3) | held: the symlink is followed, but only the provider can place one | **disagreement**, not settled here |
| 2 | Nothing written; every request a read | held | held | stands |
| 3 | No `unsafe` in this crate | held | held (notes F12, outside the claim) | stands; `unsafe` check PASS |
| 4 | Only configured hosts; proxy per `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`; no redirect followed; `--demo` makes none | held (loopback redirect tests could not run in its harness) | held, with caveats: the proxy variable list is incomplete and loopback goes via the proxy (finding 14) | stands per both legs; wording incomplete per `claude-opus` |
| 5 | Only `log_cmd` is started, without a shell | held | held | stands |
| 6 | rustls with platform verification; no switch-off | held | held (notes F8) | stands |
| 7 | Untrusted text cannot control the terminal | held | held, traced into ratatui-core | stands |
| 8 | No untrusted input can panic, hang or grow memory without bound | broken (findings 3, 5, 6, 10) | "held in letter", while reporting the same race (finding 3) and the parse amplification (finding 4) | **disagreement on the verdict**; both legs report finding 3 against the 1 MiB bound |
| 9 | A failed read shows as failed; stale values give their age | broken (findings 1, 7) | broken (findings 2, 7, 8, 9) | **does not stand**: both legs broke it, with different findings except 7 |
| 10 | Dependencies from crates.io, no known advisory, permissive licences | could not tell: `cargo-deny` not installed | could not tell: `cargo-deny` not installed; hand-checked sources and licences | legs could not tell; the run's checks (`advisories`, `licences`, `sources`) all PASS through `cargo-deny` |

## 6. Agreement and disagreement

**Both legs found:** finding 3 (swap-record check-then-read race) and the future-timestamp half of finding 7.
Both broke claim 9, and both could not run `cargo-deny` for claim 10.

**One leg alone:**

- `codex-gpt-5.6-sol` alone: findings 1, 5, 6, 10.
  - Finding 1 looks like a miss by `claude-opus`, which examined health display nearby (F13) but did not consider a
    failed status carrying `ok: true`.
  - Findings 5 and 6 look like misses: `claude-opus` noted the unbounded channels but did not raise them, and did
    not consider record count. Both are from code reading; neither was run.
  - Finding 10 is real, since the `panic-surface` check lists the line. It contradicts `claude-opus`'s statement that
    the arithmetic saturates, so that is a miss.
- `claude-opus` alone: findings 2, 4, 8, 9, 11–17.
  - Finding 4 was measured, and finding 15 was run, so neither looks like a false positive. Finding 4's reachability
    through the real board is open.
  - Finding 2 contradicts `codex-gpt-5.6-sol`'s claim 9 note that "other source failures were marked unknown/stale
    or cleared conservatively". The mechanism is a direct `free()` → `0`, so this reads as a miss by codex.
  - Findings 8, 9 and 12 are display and state paths that `codex-gpt-5.6-sol` traced without flagging them. They
    look like misses rather than false positives.
  - Findings 11, 13, 14, 16 and 17 are claim-wording gaps, trusted-environment issues, or info items. They are
    outside what `codex-gpt-5.6-sol` reported on, not things it contradicted.

**Disagreements on substance:** claim 1 (whether reading a symlinked file in the provider's directory counts), the
severity of finding 3 (medium adopted, reason given there), and claim 8's verdict.

## 7. Legs

**[`codex-gpt-5.6-sol`](rust-review/codex-gpt-5.6-sol/)**: gpt-5.6-sol through codex-cli 0.172.0 at high effort,
330 s. **6 findings**, all from code reading: findings 1–4 medium, 5–6 low.
It says it read every scoped file in full. `cargo test --locked --offline` ran 49 tests and 44 passed. The other
five, HTTP-adapter tests, failed because its harness rejects `TcpListener::bind("127.0.0.1:0")` with `EPERM`,
which it calls a harness limitation, not an assertion failure. `cargo-deny` was not installed, so it could not
establish claim 10. It deliberately did not run the infinite-log, million-record or metadata-race inputs.

**[`claude-opus`](rust-review/claude-opus/)**: claude-opus-5-5 through Claude Code 2.1.331, 402 s.
**13 findings**: F1–F2 medium, F3–F10 low, F11–F13 info, with F13 bundling five items. `cargo test --locked --offline`
passed 49 tests. It built the release binary and ran it only with `--demo`; live mode was not run because the leg
had no network. `cargo-deny` and `/usr/bin/time` were missing; it measured memory by polling `VmHWM`. Its
measurement crate for F1 is in [`rust-review/claude-opus/amp/`](rust-review/claude-opus/amp/). It also read
dependency sources (ratatui-core, reqwest, hyper-util, rustls-platform-verifier). Several of its line numbers fall
past the end of the file they cite (§ 4, location note).

There is no `audits/KNOWN_ANSWER.md`, so this run was not graded against a known answer.
