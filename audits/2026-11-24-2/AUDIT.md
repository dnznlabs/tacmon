# Security review 2026-11-24-2: tacmon

## 1. What this is

This is a self-review by tacmon's own developers using language models. It is not an audit by an outside firm.
It combines the project's checks that have one right answer with one adversarial review of the files in
scope. The run had **one leg**, so nothing in it was independently confirmed.

## 2. Provenance

```
audit date        2026-11-24-2
code commit       85014a22409a7c67896edd3a72a5199f7fbb2fce  (scope dirty: no · tree dirty: yes)
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
leg 1             claude 2.1.331 (Claude Code) · requested opus → observed claude-opus-5-5 · 366s wall · tokens 1959029 · 25 of 25 scope files named in the report · tree unchanged
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
| PASS | tests | the tests pass (35 passed, 0 failed) |
| PASS | scope | every tracked .rs file is in audits/SCOPE or excluded there with a reason |
| REPORT | panic-surface | 82 lint hits where a panic or a silent wrap could start ([`panic-surface.txt`](checks/panic-surface.txt)). This is a count, not a pass. |

No check failed, so § Findings has no entry from the checks.

## 4. Findings

I read **13 findings** from the one leg (F-1 to F-13, with F-13 grouping four minor items). There was nothing
to deduplicate. With one leg there is no second severity to set against the first, so every severity below
is the leg's own, adopted as given. None of it is independently confirmed. The F-numbers are the leg's.

The leg ran its reproductions in `.audit-leg/`. Its scripts and captured output were kept in the leg
directory, which is where the paths below point.

### F-2: Market and Advisor draw stale board data as current; expired wants are kept
- **Where:** `src/monitors/fast_bridge/state.rs:104-120`; `src/monitors/fast_bridge/ui.rs:234-280` (`market`, never reads `ads_ok`); `ui.rs:282-343` (`advisor`, never reads `board_ok`).
- **Severity:** medium. **Found by:** claude-opus alone (the only leg).
- **What it is:** once the board stops answering, the last ads and wants are kept, as intended. But the
  Market title looks the same as when the data is fresh. The Advisor's suggestions are still computed from
  the old wants and nothing says so. `until` is checked only when a fresh list arrives (`state.rs:109`), so
  expired wants are still counted and Demand shows them as `0s` to expire. Only the Demand title and the
  header light show the failure.
- **What it costs:** threat 1. The operator acts on stale numbers that look current. Breaks claim 9.
- **Evidence:** from reading only. A snapshot draws once, so it cannot show a success followed by a failure.

### F-3: Figures from the log carry no source or age, and a missing capacity is drawn as 0.00
- **Where:** `src/monitors/fast_bridge/ui.rs:39-53`; `state.rs:43,76,183-189`.
- **Severity:** medium. **Found by:** claude-opus alone.
- **What it is:** only `free` is marked "from the log". `promised`, `held`, `open` and `holds` come from the
  last `capacity` line and are drawn bold, with no mark and no age. If there is no capacity line (none yet,
  older than `-n 2000`, or `log_cmd` failed), `Capacity::default()` is drawn as
  `free 0.00 (from the log)   promised 0.00   held 0.00 (0 open, 0 holds)`.
- **What it costs:** made-up zeros attributed to a log that said nothing. Breaks claim 9 and the repo rule
  that a failed read is `None`.
- **Evidence:** ran it ([`snap-overview.txt`](rust-review/claude-opus/snap-overview.txt)).

### F-4: A forged ad under the provider's key lights "on board"; an unknown identity shows red, not unknown
- **Where:** `src/monitors/fast_bridge/state.rs:202-219`; `ui.rs:65-67`, `ui.rs:239`.
- **Severity:** medium. **Found by:** claude-opus alone.
- **What it is:** the leg describes two cases.
  - **A forged ad, with the real one missing.** If exactly one ad carries the provider's `claimPub`,
    `ad_contested()` is false and `my_ad()` returns it without checking its terms against `/info`. The
    "on board" light goes green. Market marks the impostor's row as yours. The impostor's `name` becomes the
    window title. The mitigation in SECURITY.md does not fire, because it only triggers when more than one
    ad uses the key. The leg posted an ad at fee 0.01% against `/info` at 1.50%: the title and Market showed
    the impostor, and nothing warned that the key was contested.
  - **An unknown identity.** When `claim_pub` is not known, `my_ads()` is empty and the light is
    `Some(false)` (red, "not on board") instead of unknown.
- **What it costs:** threat 1. Anyone can make the provider look listed, with false terms.
- **Evidence:** the first case was run. The light colour and the second case come from reading.

### F-1: A redirect is read as a successful answer, not an error
- **Where:** `src/monitors/fast_bridge/adapters/http.rs:38-40` (`get`), `:43-45` (`get_any_status`), `:109` (RPC); client at `:16-17`.
- **Severity:** low. **Found by:** claude-opus alone.
- **What it is:** with `Policy::none()`, reqwest returns the 3xx to the caller, and `error_for_status()` only
  rejects 4xx and 5xx. A 3xx with a JSON body is parsed as a good answer. The redirect is never followed, so
  nothing leaks. The existing test `a_redirect_is_an_error_and_is_not_followed` passes only because its 307
  has an empty body.
- **What it costs:** a provider or board that redirects with a body is shown as healthy and current. Breaks
  claim 4's "a redirect is an error".
- **Evidence:** ran it. `/info` answering 307 with a terms body was drawn in the header, and `/ads`
  answering 302 was listed on Market ([`server.py`](rust-review/claude-opus/server.py),
  [`snap-overview.txt`](rust-review/claude-opus/snap-overview.txt), [`snap-market.txt`](rust-review/claude-opus/snap-market.txt)).

### F-5: Missing or unreadable fields become zeros that are drawn as real values
- **Where:** `src/monitors/fast_bridge/adapters/wire.rs:19-21` (`field`), `:49-60` (`swap`), `:142` (`detail`), `:68` (`want.until`).
- **Severity:** low. **Found by:** claude-opus alone.
- **What it is:**
  - A swap record whose offer has no `value`/`amount` shows `0.00` paid, got and fee, and adds 0 to incoming
    and received with no mark. A missing `created` shows an age of `20781d`.
  - `"free":"lots"`, a negative or a float shows as `free 0.00`.
  - A want without `until` shows `EXPIRES IN 0s`.
- **What it costs:** understated money with no mark. Against claim 9 and the `None` rule.
- **Evidence:** the first two were run ([`data/swaps/rec1.json`](rust-review/claude-opus/data/swaps/rec1.json),
  [`snap-activity.txt`](rust-review/claude-opus/snap-activity.txt)).

### F-6: With several boards, one failing is invisible and duplicates are counted twice
- **Where:** `src/monitors/fast_bridge/adapters/http.rs:76-84`.
- **Severity:** low, and only with more than one board configured. **Found by:** claude-opus alone.
- **What it is:** a board that errors is skipped with `continue`, and the joined list is reported as a full
  success. Lists are not deduplicated. The provider's own ad mirrored on two boards trips "another ad uses
  your key". A want mirrored on two boards doubles volume and revenue.
- **Evidence:** from reading.

### F-7: Board answers inflate about 50x in memory, and the Advisor counts fees on dust
- **Where:** `http.rs:79-81`; `advisor.rs:41-71`; `model.rs:23-25`.
- **Severity:** low. **Found by:** claude-opus alone.
- **What it is:** a 4 MiB `/wants` of 123,360 one-base-unit wants raised peak RSS from 11 MiB to 197 MiB.
  That is per board per refresh, and every frame builds a table row for every want. The Advisor counted the
  flat fee as revenue on wants smaller than the fee (`YOU EARN 12,336.00 ★ best`). The leg also notes that
  SECURITY.md lists the unsigned ad as a threat but not the unsigned want.
- **What it costs:** threat 1 (advice driven by anyone who can post wants), and memory pressure toward threat 3.
- **Evidence:** ran it ([`flood.py`](rust-review/claude-opus/flood.py), [`measure.py`](rust-review/claude-opus/measure.py)).

### F-8: The log store can hold hundreds of MiB
- **Where:** `src/monitors/fast_bridge/state.rs:12,176-179`; `local.rs:15`; `wire.rs:98,124`.
- **Severity:** low. **Found by:** claude-opus alone.
- **What it is:** up to 5,000 entries, each holding a `msg` and a `detail` of up to about 64 KiB. That is
  roughly 640 MiB in the worst case. It is bounded, but enough to take down a small host (threat 3).
- **Evidence:** worked out from the caps. Not run. Medium confidence.

### F-9: Unbounded channels grow while a draw is blocked; a swap file can hang the task or outgrow its cap
- **Where:** `feed.rs:47`, `local.rs:53`, `shell/mod.rs:49`; `local.rs:113-117`.
- **Severity:** low / info. **Found by:** claude-opus alone.
- **What it is:**
  - Events and `Msg::Wake` use unbounded channels that are drained only between draws. A blocked terminal
    write lets log lines of up to 64 KiB pile up without limit.
  - The swap reader checks `metadata` and then calls `read`, a TOCTOU. A file that grows after the check is
    read whole, and a FIFO swapped in after the check blocks the swaps task for ever. Symlinks are followed.
    All of this needs write access to `dataDir`.
  - The number of records read per pass is unbounded.
- **Evidence:** from reading. Medium confidence.

### F-10: The "system proxy" wording does not match the build; a proxy can answer for the local provider
- **Where:** `Cargo.toml:24`; `http.rs:16-17`.
- **Severity:** info. **Found by:** claude-opus alone.
- **What it is:** reqwest's `system-proxy` feature is off, so macOS and Windows system proxy settings are
  ignored and only `HTTP(S)_PROXY`/`ALL_PROXY` apply. That contradicts claim 4's wording. The env matcher
  does not exempt loopback: with `HTTP_PROXY` set and no `NO_PROXY` for 127.0.0.1, the proxy answers for
  the provider.
- **Evidence:** read in the dependency source. Not run.

### F-11: Claim 7 rests on ratatui behaviour that nothing in the crate pins
- **Where:** the untrusted `Span`/`Cell` sites in `ui.rs` (for example `:92`, `:185`, `:191`, `:247`) and `shell/mod.rs:187`.
- **Severity:** info. **Found by:** claude-opus alone.
- **What it is:** tacmon strips nothing itself. ratatui-core drops graphemes that contain control characters,
  and the leg confirmed in a run that ESC, BEL and U+009B did not reach the output. No test holds that in
  place, so a change in ratatui would break claim 7 without anything failing.

### F-12: The build compiles C crypto from a build script
- **Where:** `Cargo.toml:24` (reqwest `rustls` → aws-lc-rs → `aws-lc-sys`).
- **Severity:** info. **Found by:** claude-opus alone.
- **What it is:** claim 3 covers this crate only. The TLS stack is mostly C, built with cmake/cc.

### F-13: Minor items
- **Severity:** info, from reading. **Found by:** claude-opus alone.
  - `state.rs:172`: `last.count += 1` on a `u32` would panic in debug and wrap in release after 2³²
    identical lines. Not reachable in practice.
  - `wire.rs:78`: `reverse` is true for any value that is neither null nor `false`, including `"false"` and `0`.
  - `feed.rs:57-58`: each later `fast bridge LP` line replaces the identity and the pool account. A forged
    line in the provider's output would re-point the ETH balance and the claim key.
  - `http.rs:111`: the ETH balance passes through `f64`. It is display only.

## 5. Claims

| # | Claim (short) | claude-opus | Stands? |
|---|---|---|---|
| 1 | No private key is read, requested or stored | held | stands (one leg) |
| 2 | Nothing written, every request is a read | held | stands (one leg) |
| 3 | No `unsafe` in this crate | held (dependencies do contain unsafe code and C: F-12) | stands, also by the `unsafe` check |
| 4 | Only configured hosts; system proxy; a redirect is an error | **broken** (F-1, F-10) | **does not stand** |
| 5 | Only `log_cmd` is started, never through a shell | held | stands (one leg) |
| 6 | rustls with the platform's checks, no way to switch verification off | held (roots can be replaced through `SSL_CERT_FILE`/`SSL_CERT_DIR`, a trusted environment) | stands (one leg) |
| 7 | Untrusted text cannot control the terminal | held (by ratatui, not by tacmon: F-11) | stands (one leg) |
| 8 | No panic, hang or unbounded memory from untrusted input | held, with caveats (F-7, F-8, F-9) | stands, with the caveats open |
| 9 | A failed read shows as failed; stale is never drawn as fresh | **broken** (F-2, F-3, F-5, F-6) | **does not stand** |
| 10 | crates.io only, no advisory, permissive licences | could not tell (no `cargo-deny` in the leg's environment) | stands on the `advisories`, `licences`, `bans` and `sources` checks |

No claim has a disagreement to report, because only one leg ran. On claim 10 the leg and the checks do not
conflict: the leg could not run the tool, and the runner did.

## 6. Agreement and disagreement

This run had one leg, so this section has nothing to measure. Every finding above was found by one model and
by nothing else.

## 7. Grading against the known answer

Left out. The run had no `## Known answer`.

## 8. Legs

**claude-opus** ([`rust-review/claude-opus/`](rust-review/claude-opus/), [`report.md`](rust-review/claude-opus/report.md)).
This was Claude Code 2.1.331 running `claude-opus-5-5`. The leg read all 25 scope files whole and built and
tested the crate (35 passed). It ran the release binary with `--snapshot` against local Python servers on
127.0.0.1, using a fake `log_cmd` and `data_dir`. It reported 13 findings: 3 medium, 6 low (one of them
low/info), and 4 info. In its own words:

- "**`cargo deny` is not installed** in this environment … The advisory, licence and ban checks behind
  claim 10 were not run."
- "`--snapshot` has no colour, so I could not see colour (the status lights) in a run."
- "I did not audit the dependencies' own code beyond the functions named above."
- "I did not test the interactive loop (`run`) in a real terminal. Blocking and back-pressure (F-9) are from reading."

The run was not graded, because there was no known answer.
