# 1. What could not be done

- `cargo test --locked --offline` ran 56 tests, but six HTTP-adapter tests could not bind a loopback socket in this harness (`Operation not permitted`). The other 50 tests passed. I therefore checked the redirect, status, timeout, and body-limit paths in those six tests by reading them, not by observing them run.
- `cargo-deny` is not installed in this harness, and the harness has no network. I could not independently refresh the advisory database or complete the licence/advisory half of claim 10. `cargo build --locked --offline` succeeded, and `cargo tree --locked --offline -e features` succeeded.

# 2. Files read

| File | What I looked at |
|---|---|
| `SECURITY.md` | Read whole first; extracted the trust boundary, attacker goals, exclusions, and all ten numbered claims. |
| `Cargo.toml` | Read whole; checked release panic policy, direct dependencies, default features, TLS selection, and Tokio capabilities. |
| `deny.toml` | Read whole; checked advisory, licence, source, and alternate-TLS policy. |
| `src/config.rs` | Read whole; checked config parsing, defaults, paths, and whether untrusted runtime data enters here. |
| `src/fmt.rs` | Read whole; checked time fallback, arithmetic, string slicing, and formatting bounds. |
| `src/main.rs` | Read whole; checked argument parsing, snapshot dimensions and waits, demo selection, and startup error paths. |
| `src/monitors/fast_bridge/adapters/http.rs` | Read whole; traced every response through status handling, redirects, body/item limits, JSON parsing, board aggregation, RPC parsing, and tests. |
| `src/monitors/fast_bridge/adapters/local.rs` | Read whole; traced directory entries, symlinks, file metadata/read limits, log-line truncation, process spawning/restart, queueing, and tests. |
| `src/monitors/fast_bridge/adapters/memory.rs` | Read whole; checked that demo/test ports perform no I/O and examined all generated values and log signals. |
| `src/monitors/fast_bridge/adapters/mod.rs` | Read whole; checked concrete adapter selection and that demo bypasses live adapters. |
| `src/monitors/fast_bridge/adapters/wire.rs` | Read whole; traced every JSON field, numeric conversion, missing/wrong/large values, timestamps, text limits, log detail expansion, and tests. |
| `src/monitors/fast_bridge/advisor.rs` | Read whole; checked fee/size comparisons, overflow behavior, selection semantics, totals, and tests. |
| `src/monitors/fast_bridge/config.rs` | Read whole; checked endpoints, command/path configuration, refresh lower bounds, and advisor defaults. |
| `src/monitors/fast_bridge/feed.rs` | Read whole; checked task lifetimes, request ordering, failure events, bounded channels, account propagation, sleeps, and tests. |
| `src/monitors/fast_bridge/model.rs` | Read whole; checked all amount arithmetic, narrowing, saturation, formatting, phase classification, and tests. |
| `src/monitors/fast_bridge/mod.rs` | Read whole; checked live/demo composition, event draining, page indexing, state refresh, and tests. |
| `src/monitors/fast_bridge/ports.rs` | Read whole; checked capability boundaries, boxed-future lifetimes, queue bounds, and exposed I/O operations. |
| `src/monitors/fast_bridge/state.rs` | Read whole; traced every event into retained state, freshness flags, expiry, log retention/coalescing, ad identity, advice inputs, arithmetic, and tests. |
| `src/monitors/fast_bridge/ui.rs` | Read whole; traced every untrusted field to terminal widgets and checked stale/missing labels, table slicing, fee advice, deadlines, and tests. |
| `src/monitors/mod.rs` | Read whole; checked monitor enablement and live/demo construction. |
| `src/shell/mod.rs` | Read whole; checked terminal lifecycle, bounded wake behavior, snapshots, input thread, indexing, drawing, and plain-text output. |
| `src/shell/monitor.rs` | Read whole; checked wake coalescing and the monitor interface's indexing and lifecycle assumptions. |
| `src/shell/splash.rs` | Read whole; checked indexing, float/division behavior, character boundaries, dimensions, and animation bounds. |
| `src/shell/theme.rs` | Read whole; checked colour parsing and fallback behavior. |
| `src/shell/widgets.rs` | Read whole; checked status semantics, scrolling bounds, and the control-character terminal test. |

# 3. Claims

| # | Result | Finding or reason |
|---|---|---|
| 1 | broken | F-03: the check-before-open race can make tacmon follow a swapped-in symlink and read an arbitrary regular file of at most 1 MiB, including a private-key file readable by the process. |
| 2 | held | Production paths only read files, stdout, and network answers. Provider/board calls are GETs; the configured RPC uses the read-only `eth_getBalance` method. No production disk write was found. |
| 3 | held | The crate root has `#![forbid(unsafe_code)]`; no crate `unsafe` was found in scope, and the crate built with the prohibition active. |
| 4 | held | All live clients use config-derived bases, reqwest's environment proxy support, an eight-second timeout, and `Policy::none`; non-success answers are rejected except the deliberately parsed health error body, and redirects are rejected before parsing. Demo constructs memory ports only. The loopback tests themselves could not run in this harness. |
| 5 | held | The sole production spawn is `Command::new(prog).args(args)` for `log_cmd`; there is no shell and no other process start in scope. |
| 6 | held | Reqwest disables default features and enables rustls; the resolved feature tree uses rustls plus `rustls-platform-verifier`, and no OpenSSL/native-tls TLS stack was resolved. |
| 7 | held | All hostile text is passed through ratatui cells; the scoped test proving rendered cells contain no control characters passed. Direction-changing and invisible Unicode remain the stated exclusion. |
| 8 | broken | F-03 breaks the explicit promise that a link is never read as a record. F-04 also gives untrusted repeated log input a debug-build panic and a release-build counter wrap. The body, line, record, list, channel, and retained-log bounds otherwise held on review. |
| 9 | broken | F-01 reports a partial multi-board aggregation as a successful complete read. F-02 draws advice derived from stale ads as current whenever wants still answer. |
| 10 | could not tell | The locked offline build and feature tree succeeded, and the manifest/deny policy uses registry dependencies and permissive licences, but `cargo-deny` and a current advisory database were unavailable. |

# 4. Findings

## F-01 — A partial multi-board answer is marked complete (`src/monitors/fast_bridge/adapters/http.rs:99`)

**Severity: medium**

`HttpBoard::list` silently continues past every board that times out, returns an error status, returns malformed JSON, omits the requested list, or sends more than `MAX_ITEMS`. If any other configured board supplies a valid list, `out` is `Some`, the aggregate returns `Ok`, and state marks the source as answering. There is no partial-read bit.

For example, with two configured boards, board A can answer `200 {"wants":[]}` while board B answers 500 (or a `wants` array with 5,001 entries). The returned value is a successful empty market. The same applies to ads. A failing or hostile board can therefore remove its entire portion of demand or supply without any failure indication, and the operator can act on the resulting false totals and fee comparisons.

Who can trigger it: the operator of any one configured board, or a network/service failure affecting one board, when at least one other board answers. Confidence is high. I established it by reading lines 99–107 and tracing the resulting `Some(Vec<_>)` through `feed.rs`, `state.rs`, and the UI. The socket-requiring adapter tests could not run in this harness, and there is no test for partial multi-board failure.

## F-02 — Advisor presents stale ads as current (`src/monitors/fast_bridge/ui.rs:360`)

**Severity: medium**

Ads and wants have separate reads and separate freshness flags. On a failed ads read, `State::apply` sets `ads_ok = false` but deliberately keeps the last ads. `State::advice` then uses those retained ads unconditionally at `state.rs:247-256`. On the Advisor page, however, the freshness label at line 360 checks only `board_ok`, which is the wants flag. The prose at lines 375–385 can consequently say that the provider is cheapest on the board, or name a matching fee, from stale ads with no stale warning on that page.

An exact triggering sequence is: first return a cheap competing ad successfully; on a later refresh let `/wants` succeed but make `/ads` fail. The page labels the fee table as applying to what is waiting now and continues to compare against the old competitor. This targets the threat model's highest-risk outcome: an operator changing a fee based on a false current market view.

Who can trigger it: a board that selectively fails or delays `/ads`, or an ordinary asymmetric endpoint failure. Confidence is high. I checked it by tracing `Event::Wants(Some(_))` and `Event::Ads(None)` through `state.rs:113-129`, `state.rs:247-256`, and the complete Advisor rendering. I did not modify source to add a reproducer.

## F-03 — Symlink rejection has a check-before-open race (`src/monitors/fast_bridge/adapters/local.rs:31`)

**Severity: low**

`record` first calls `symlink_metadata(p)` and rejects a link, then separately calls `File::open(p)`. A process able to mutate the swap directory can replace the checked ordinary file with a symlink between those operations. `File::open` follows that link; the subsequent `file.metadata()` describes the target and accepts it when it is an ordinary file no larger than 1 MiB. Tacmon then reads the target into memory and attempts to parse it as a swap.

This breaks the absolute claims that tacmon cannot read a private key and that a link is never read as a record. The practical impact is limited: exploitation needs a local/provider process that can race entries in the configured data directory, the target must be readable by tacmon and at most 1 MiB, and only fields matching the swap schema reach the UI. Still, the data source is explicitly untrusted and the file-type check does not enforce what its comment promises.

Who can trigger it: a compromised or malicious provider process, or another process with mutation rights in `data_dir/swaps`. Confidence is high in the race from code inspection; I did not run a timing-dependent exploit.

## F-04 — Repeated log-line count overflows (`src/monitors/fast_bridge/state.rs:182`)

**Severity: low**

Consecutive entries with the same message and level are coalesced with `last.count += 1`, where `count` is `u32`. A provider log that emits 4,294,967,296 consecutive matching non-`swap`, non-`capacity` messages reaches the overflow. A debug build panics in the main refresh/draw path; the configured release build silently wraps the count to zero, temporarily hiding the repetition count and then showing a false small count.

The queue and stored log remain bounded, so the input volume is very large and this is not a quick memory attack. It nevertheless contradicts the unconditional no-untrusted-input panic claim in debug builds and the review prompt's requirement that integer overflow not silently wrap in release.

Who can trigger it: the provider or anything able to control the configured log command's output. Confidence is high from integer semantics and the direct increment. I did not emit billions of lines to run it.

# 5. What you did not look at

Nothing in scope was skipped or read only in part. I did not inspect prior audit reports, and I did not review out-of-scope implementation files beyond dependency information emitted by the permitted Cargo commands.
