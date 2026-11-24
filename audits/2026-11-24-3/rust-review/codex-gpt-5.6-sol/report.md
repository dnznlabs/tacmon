# 1. What could not be done

- Every scoped file was opened and read in full.
- `cargo test --locked --offline` built the test binary and ran 49 tests. Forty-four passed. Five HTTP-adapter tests could not run because this harness rejects `TcpListener::bind("127.0.0.1:0")` with `EPERM`; their failures were harness limitations, not assertion failures.
- `cargo build --locked --offline` succeeded, and the built binary produced offline `--demo` snapshots for the splash and overview.
- `cargo-deny` is not installed (`cargo deny --version` reports “no such command”), so I could not run the advisory and licence checks needed to establish claim 10. `Cargo.lock` has only the crates.io registry source, and the resolved normal dependency tree contains rustls and no OpenSSL/native-tls package, but those checks do not replace an advisory/licence scan.
- I did not run the deliberate infinite-log, million-record, or metadata-race inputs below because their expected effect is resource exhaustion. Those findings are from code-path review.

# 2. Files read

| File | What I looked at |
|---|---|
| `SECURITY.md` | Read the trust boundary, attacker goals, all ten numbered claims, and stated exclusions first. |
| `Cargo.toml` | Checked release panic behavior, direct dependencies, enabled features, and Tokio runtime features. |
| `deny.toml` | Checked advisory, licence, banned TLS-stack, and source policies. |
| `src/config.rs` | Traced config-file reads, defaults, parsing failures, and theme input. |
| `src/fmt.rs` | Checked time fallback, arithmetic, byte slicing, and formatting bounds. |
| `src/main.rs` | Checked argument parsing, config/monitor construction, snapshot dimensions and waits, and the unsafe prohibition. |
| `src/monitors/fast_bridge/adapters/http.rs` | Traced every response from request through status handling, body limit, JSON parse, list limit, redirect policy, timeout, and RPC conversion. |
| `src/monitors/fast_bridge/adapters/local.rs` | Traced command spawning, log framing/restart behavior, channels, directory enumeration, metadata checks, and file reads. |
| `src/monitors/fast_bridge/adapters/memory.rs` | Checked that demo/test ports perform no I/O and reviewed all generated demo values. |
| `src/monitors/fast_bridge/adapters/mod.rs` | Checked concrete adapter selection and which configured destinations/capabilities are instantiated. |
| `src/monitors/fast_bridge/adapters/wire.rs` | Traced malformed, missing, huge, signed, textual, and timestamp values into every model type and displayed log field. |
| `src/monitors/fast_bridge/advisor.rs` | Checked fee, volume, cap, and size calculations plus intermediate collection growth. |
| `src/monitors/fast_bridge/config.rs` | Checked all adapter destinations, command/path settings, refresh floors, advisor inputs, and defaults. |
| `src/monitors/fast_bridge/feed.rs` | Checked task lifetimes, polling cadence, failure conversion, account propagation, and unbounded event/wake flow. |
| `src/monitors/fast_bridge/model.rs` | Checked overflow/underflow handling, amount formatting, phase classification, and model invariants. |
| `src/monitors/fast_bridge/mod.rs` | Checked demo isolation, feed startup, event draining, page indexing, and state/UI handoff. |
| `src/monitors/fast_bridge/ports.rs` | Checked each outside capability, object-safety boundary, and port ownership/lifetimes. |
| `src/monitors/fast_bridge/state.rs` | Traced each event into retained state, stale/failure flags, log bounding/folding, sorting, aggregation, ad identity, and ages. |
| `src/monitors/fast_bridge/ui.rs` | Traced all untrusted strings and numbers to terminal widgets and checked missing/stale status presentation and row allocation. |
| `src/monitors/mod.rs` | Checked monitor enumeration and demo/live construction. |
| `src/shell/mod.rs` | Checked terminal lifecycle, input/tick/wake queues, refresh/drain loops, snapshots, page selection, and drawing indices. |
| `src/shell/monitor.rs` | Checked the monitor boundary and the unbounded wake sender. |
| `src/shell/splash.rs` | Checked all indexing, slicing, arithmetic, character construction, and small-terminal paths. |
| `src/shell/theme.rs` | Checked that malformed trusted colour strings fall back without panic. |
| `src/shell/widgets.rs` | Checked common row/window behavior and the terminal-control-character regression test. |

# 3. Claims

| # | Result | Finding or reason |
|---:|---|---|
| 1 | broken | Finding 3: `metadata` follows a `.json` symlink and `read` then reads the target bytes. A link to any readable regular file at most 1 MiB, including a private-key file, is a code path that reads it even when parsing later discards it. |
| 2 | held | All in-crate filesystem operations are reads. Provider/board requests are GETs and the sole POST is the read-only `eth_getBalance` RPC. No write endpoint or filesystem write was found. |
| 3 | held | `src/main.rs` has `#![forbid(unsafe_code)]`; no `unsafe` occurrence exists in scoped crate source. The offline build succeeded with the prohibition active. |
| 4 | held | Live HTTP objects are built only from `lp_url`, `boards`, and optional `eth_rpc`; the shared client has an 8-second timeout and `Policy::none()`, and every 3xx is rejected before body parsing. Demo selects only memory ports. Loopback redirect tests could not execute in this harness. |
| 5 | held | The only runtime process construction is `Command::new(prog).args(args)` for configured `log_cmd`, with no shell. |
| 6 | held | The manifest disables reqwest defaults and enables `rustls`; the resolved normal tree contains `hyper-rustls`, `rustls`, and `rustls-platform-verifier`, with no OpenSSL or native-tls package. No verification-disable option was found. |
| 7 | held | All untrusted strings go through ratatui cells. The hostile-control-character regression test passed and asserts that no control character remains in the drawn buffer. |
| 8 | broken | Findings 1–3 provide unbounded queues, unbounded record cardinality, and a race that bypasses the file size/type gate. Finding 6 also gives a debug panic/release wrap after enough repeated log entries. Per-body, per-line, and per-file nominal limits otherwise held in reviewed paths. |
| 9 | broken | Finding 4 lets a failed HTTP status display green health, and finding 5 displays a future-dated stale capacity value as `0s` old. Other source failures were marked unknown/stale or cleared conservatively. |
| 10 | could not tell | All locked package sources are crates.io and the reviewed deny policy is strict, but `cargo-deny` is unavailable, so known advisories and actual resolved licences were not checked in this leg. |

# 4. Findings

## 1. Unbounded log and wake queues let an untrusted log exhaust memory — `src/monitors/fast_bridge/adapters/local.rs:53`, `src/monitors/fast_bridge/feed.rs:47`, `src/shell/monitor.rs:18`

**Severity: medium.** The provider controls the log bytes, including values induced by counterparties. A continuous stream of nonblank lines such as `x\n` is parsed into one `LogEntry` per line and sent through an unbounded channel. The feed forwards each entry through another unbounded channel and emits a separate wake into the shell's unbounded channel. There is no admission limit, coalescing before enqueue, or backpressure. `MAX_LINE` bounds one allocation, and `MAX_LOG` bounds state only after queued entries reach it; neither bounds any of the three backlogs. A producer faster than the relay/drawing path grows memory until the process is killed or the machine becomes unusable, blinding the operator.

Confidence is high from reading the complete producer, relay, and consumer paths. I did not execute an intentional resource-exhaustion run.

## 2. Swap-record cardinality is unbounded — `src/monitors/fast_bridge/adapters/local.rs:104`

**Severity: medium.** The provider controls the swap-record directory. Every `.json` directory entry of at most 1 MiB is visited, and every parseable swap is pushed into `out` with no record-count or aggregate-byte limit. For example, arbitrarily many files containing `{"id":"x","phase":"done","created":"1","deadline":"1","offer":{"btc":{"value":"1"},"evm":{"amount":"1"}}}` all survive parsing. The resulting vector is then sorted and retained by state, and UI rendering allocates a row for every record. A large or long-lived store therefore makes each refresh consume unbounded time and memory despite the per-record cap.

Confidence is high from reading the full scan, state, and UI paths. I did not create a resource-exhausting directory.

## 3. Swap metadata checks follow symlinks and race the unbounded read — `src/monitors/fast_bridge/adapters/local.rs:113`

**Severity: medium.** Anyone controlling the provider's swap directory can make `record.json` a symlink. `tokio::fs::metadata` follows that link, contrary to the adjacent “a link ... never stops” comment, and `tokio::fs::read` reads the target bytes. Thus a link to any readable regular file no larger than 1 MiB causes tacmon to read that file, breaking claim 1 even if JSON parsing later discards it.

There is also a time-of-check/time-of-use gap: the path is checked at line 113 and reopened at line 117. Replacing a checked small regular file with a FIFO or a symlink to an endless source such as `/dev/zero` between those operations makes the subsequent whole-file read block or grow without the 1 MiB bound. Replacing it with a larger regular file bypasses the aggregate read limit. This breaks claim 8 and can stall the records feed or exhaust the process.

Confidence is high from the documented follow-symlink semantics of `metadata` and the two separate path-based operations. I did not race the filesystem in this leg.

## 4. A failed `/health` status can be displayed as healthy — `src/monitors/fast_bridge/adapters/http.rs:54`, `src/monitors/fast_bridge/adapters/http.rs:76`

**Severity: medium.** Whatever answers on the configured provider port can return, for example, HTTP 500 with `{"ok":true}`. `get_or_error_body` rejects only informational and redirect statuses, so it parses every 4xx/5xx body. `health` then returns `Ok(Health { ok: true, ... })`; state records `Some(true)` and the header draws a green health light. This is not merely retaining a labelled stale value: an objectively failed HTTP request is presented as a successful healthy reading, directly contradicting claim 9 and potentially prompting action on a failed provider.

Confidence is high from read-only tracing. The source test intentionally confirms that a 503 body is accepted, although this harness could not run that loopback test. Accepting a non-2xx body may be required to show the provider's error detail, but accepting `ok: true` from it is the unsafe case.

## 5. Future log timestamps make stale capacity look freshly read — `src/monitors/fast_bridge/adapters/wire.rs:83`, `src/monitors/fast_bridge/state.rs:201`

**Severity: low.** The untrusted provider log can attach a far-future timestamp to a capacity signal, for example `{"ts":"9999-12-31T23:59:59Z","msg":"capacity","held":"500","promised":"200","open":1,"holds":0}`. `epoch` accepts it. Both capacity-age paths compute `now.saturating_sub(at)`, producing zero until wall time reaches the supplied timestamp. The header consequently describes an arbitrarily old retained capacity figure as `log, 0s ago`, and `free` can use the same figure while labelling it as from the log. That breaks the claim that a stale figure gives its age.

Confidence is high from tracing the parser, state, and exact UI branches. I did not add a source test.

## 6. Repeated-line count overflows on attacker-controlled log volume — `src/monitors/fast_bridge/state.rs:182`

**Severity: low.** Identical consecutive non-`swap`, non-`capacity` log messages increment a `u32` with `last.count += 1`. After 4,294,967,295 repeats, the next line panics in an overflow-checking build or silently wraps to zero in the normal release profile. The release UI then hides or severely understates the repeated warning count. This requires extreme sustained volume, and findings 1 and 2 are more practical availability limits, but it is still an untrusted integer operation with neither saturation nor a checked stop.

Confidence is high from read-only inspection; I did not send billions of lines.

# 5. What you did not look at

Nothing in scope was omitted or only partially read. I did not inspect past audit reports. Outside scope, I used only `Cargo.lock` source entries and Cargo's resolved dependency tree to check the manifest/deny claims; I did not audit dependency implementations.
