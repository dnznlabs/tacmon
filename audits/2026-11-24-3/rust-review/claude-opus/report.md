# Leg report

## 1. What could not be done

- `cargo deny` is not installed in this environment, so claim 10 was not checked by its tool. I checked the lockfile's sources and the licence strings by hand instead (see claim 10).
- `/usr/bin/time` is missing. I measured peak memory by polling `VmHWM` in `/proc/<pid>/status`.
- I did not run the binary in live mode. Live mode contacts the configured board, and this leg has no network. Every run of the binary used `--demo`.
- Everything else in the prompt ran: `cargo test --locked --offline` (49 passed), `cargo build --release --locked --offline`, `cargo tree`, `cargo metadata`, and the binary with `--demo --snapshot`.
- Scratch work is in `.audit-leg/amp/`: a small crate that measures how much memory a 4 MiB JSON body takes once parsed.

## 2. Files read

| File | What I looked at |
|---|---|
| `SECURITY.md` | read whole: threat model, the 10 claims, what is not covered |
| `Cargo.toml` | read whole: `panic = "abort"`, no `overflow-checks` set (so arithmetic wraps in release), reqwest features (`rustls`, `json`; no gzip, no http2) |
| `deny.toml` | read whole: licence allow-list, TLS bans, source rules, `all-features` |
| `src/config.rs` | read whole: loading, `deny_unknown_fields`, default path |
| `src/fmt.rs` | read whole: `short` slicing on byte offsets (guarded by `is_ascii`), `dur`, `pct` |
| `src/main.rs` | read whole: `forbid(unsafe_code)`, CLI parsing, `--size` and `--wait` |
| `src/monitors/fast_bridge/adapters/http.rs` | read whole: client builder, redirect policy, body cap, board join, RPC parsing |
| `src/monitors/fast_bridge/adapters/local.rs` | read whole: `next_line` bound, child lifecycle and restart, the swap-record filter and read |
| `src/monitors/fast_bridge/adapters/memory.rs` | read whole: demo data; it sends and reads nothing |
| `src/monitors/fast_bridge/adapters/mod.rs` | read whole: which adapters are built, when an RPC is built |
| `src/monitors/fast_bridge/adapters/wire.rs` | read whole: every reader, `epoch`, `kept`, `detail` |
| `src/monitors/fast_bridge/advisor.rs` | read whole: `blocker`, `advise`, saturating arithmetic |
| `src/monitors/fast_bridge/config.rs` | read whole: defaults (`journalctl … -f -n 2000`, one board) |
| `src/monitors/fast_bridge/feed.rs` | read whole: one task per port, how the pool account is learned, unbounded channels |
| `src/monitors/fast_bridge/model.rs` | read whole: `fee_on`, `total`, `effective_bps`, `tac` |
| `src/monitors/fast_bridge/mod.rs` | read whole: demo vs live ports, the event drain in `refresh` |
| `src/monitors/fast_bridge/ports.rs` | read whole |
| `src/monitors/fast_bridge/state.rs` | read whole: what each event does, `free`/`free_known`, `my_ad`, log folding |
| `src/monitors/fast_bridge/ui.rs` | read whole: every page, the header, which values are drawn when unknown |
| `src/monitors/mod.rs` | read whole |
| `src/shell/mod.rs` | read whole: event loop, snapshot, key handling, layout arithmetic |
| `src/shell/monitor.rs` | read whole |
| `src/shell/splash.rs` | read whole: slicing `WORD`, the braille bit arithmetic |
| `src/shell/theme.rs` | read whole |
| `src/shell/widgets.rs` | read whole: the control-character test, `window` |

I also opened these dependency sources to understand listed files: ratatui-core `buffer.rs` and `span.rs` (control-character filtering), reqwest `async_impl/client.rs` (`build()`), hyper-util `proxy/matcher.rs`, and rustls-platform-verifier `verification/others.rs`.

## 3. Claims

| # | Verdict | Finding or reason |
|---|---|---|
| 1 | held | No code path asks for, reads or stores a key. The only file reads are the config and `swaps/*.json`. A symlink in `swaps/` would be followed (`local.rs:325`), but only the provider can put one there. |
| 2 | held | Nothing opens a file for writing. HTTP is `GET` except the RPC `POST eth_getBalance`, which is a read. The provider only ever receives GETs. |
| 3 | held | `#![forbid(unsafe_code)]` is at `main.rs:4`, and grep finds no other `unsafe`. Note: the dependency `aws-lc-sys` (pulled in by reqwest's `rustls` feature) runs a build script that compiles C. The claim does not cover that (F12). |
| 4 | held, with caveats | Redirects: `Policy::none()` plus the 2xx check. The tests pass, and the redirect target is not contacted. `--demo` never builds the HTTP client (`fast_bridge/mod.rs:36`). Caveats (F9): proxy selection also reads lowercase `http_proxy`/`https_proxy`/`all_proxy` and `NO_PROXY`, and it is turned off when `REQUEST_METHOD` is set. The local `lp_url` (127.0.0.1) also goes through the proxy unless `NO_PROXY` covers it. |
| 5 | held | `tokio::process::Command::new(prog).args(args)` at `local.rs:57`, with no shell. `prog` is found through `PATH`, which is the operator's (trusted) environment. |
| 6 | held | reqwest's `rustls` feature uses `rustls-platform-verifier`. Nothing calls `danger_*`. `SSL_CERT_FILE`/`SSL_CERT_DIR` can change the root set, but that is the trusted environment. The roots are loaded eagerly, so a machine with none panics at startup (F8). |
| 7 | held | ratatui-core drops any grapheme that contains a `char::is_control` character, in both `Buffer::set_stringn` (`buffer.rs:351`) and `Span::styled_graphemes` (`span.rs:314`). Every untrusted string reaches the terminal through `Paragraph`, `Table`, `Block` titles or `Tabs`, all of which go through those two paths. No untrusted text goes to stderr or an error message. The test covers only `Paragraph`, but the other widgets share the same path. |
| 8 | held in letter; F1 shows the bound is much larger than the claim implies | The 4 MiB cap, the 64 KiB line cut and the 1 MiB record skip all hold. No panic is reachable from an untrusted input: every slice is checked, the arithmetic saturates, and the `expect`s are on values the code itself built. But a 4 MiB answer parses into about 400 MB, and about 800 MB once cloned (F1). The swap read has a check-then-read race (F10). The only panic I triggered comes from the trusted CLI (`--wait`, F11). |
| 9 | broken | Before anything has reported free TAC, it is drawn as `0.00` on the Advisor page and drives "not enough free TAC" on the Demand page (F2). Before the swap records are first read, received, incoming and fees earned show `0.00` (F3). The Market page marks a forged ad under the provider's key as "yours" (F4). A capacity figure from a log line without a parseable `ts` is given with no age (F6). |
| 10 | could not tell | `cargo-deny` is not installed. Every one of the 321 lockfile packages comes from crates.io. Some lockfile packages carry licences outside the allow-list (`terminfo` WTFPL; `wezterm-bidi` and `finl_unicode` Unicode-DFS-2016). They come from ratatui's optional termwiz backend, and `cargo tree --all-features -i terminfo` finds them nowhere in this crate's graph, so cargo-deny probably passes. Advisories were not checked. |

## 4. Findings

### F1. A 4 MiB board answer becomes about 0.4–0.8 GB in memory: `http.rs:37`, `http.rs:96-97` (medium)

- **Who and how:** anyone who can post to the board, because they control the bytes inside `/ads` and `/wants`. `json_body` caps the raw body at 4 MiB, then `serde_json::from_slice` builds a `Value` tree. `MAX_ITEMS` is only checked after parsing, and lists that pass it are deep-cloned (`items.iter().cloned()`).
  - Small objects cost about 650 bytes each once parsed (each has its own map allocation).
  - Exact input I measured: `{"ads":[{"":0},{"":0},…]}` at 4,194,297 bytes, which is 599,184 objects. Parsing plus cloning peaked at **792 MB** (VmHWM). Numbers peaked at 136 MB and empty arrays at 90 MB.
- **What happens:** an attacker stays under `MAX_ITEMS` and puts the bulk inside items, for example one ad carrying `"x":[{},{},…]`. That ad is parsed and then cloned. `wants` and `ads` are fetched together (`feed.rs:84`), so both peaks land in the same poll, every 20 s, once per board.
  - On a small VPS that is an OOM kill. With `panic = "abort"` and no cleanup, the operator is left blind.
- **Whether it is reachable:** that depends on whether the public board stores arbitrary extra fields and large ads. I could not test the board.
- **Certainty:** the amplification was measured (`.audit-leg/amp/`). Reachability through the real board was not tested.

### F2. Free TAC that nothing has reported is shown as 0.00 and used to judge every waiting bridge: `ui.rs:358`, `ui.rs:378-383`, `state.rs:243-245`, `advisor.rs:13` (medium)

- **Who and how:** no attacker is needed. It happens whenever `/info` answers without `available` and no capacity line has been read yet. That covers a fresh start before the log reaches a capacity line, `log_cmd` failing to start, and journald permissions missing (a gotcha the docs themselves list).
  - `State::free()` returns `0` whenever `free_known()` is false.
  - The header handles this correctly and draws a dash (`ui.rs:73`).
  - The Advisor page prints "You have 0.00 free" and "N more TAC would cover all of it at once."
  - `State::blocker` calls `advisor::blocker(t, self.free(), w)`, so the Demand page marks every in-range want "✕ not enough free TAC".
  - Neither page says the figure is unknown or that it came from the log.
- **Effect:** this is threat 1. The operator is told to fund the pool, and told they can take nothing, on a figure nobody has given. It breaks claim 9 and the repo rule "a failed read is None, never a made-up zero".
- **Certainty:** high, from reading the code. Following the path is direct: `free_known` is never consulted outside the header.

### F3. Money totals from swap records show 0.00 before the records have ever been read: `ui.rs:91-93`, `state.rs:209-218` (low)

- **Who and how:** any time `swaps/` cannot be read. Examples: missing read access to `dataDir`, a wrong `data_dir`, or the first seconds after start.
  - `received`, `incoming` and `fees earned` are drawn as `0.00` in bold.
  - Only the separate "records" light (✕) says otherwise.
  - After a read has succeeded and later fails, the last totals stay up with nothing beside them to say they are stale. The light is the only cue.
- **Effect:** a figure nobody has given is drawn as zero.
- **Certainty:** high, from reading the code.

### F4. The Market page marks every ad under the provider's key as "yours", including a forged one: `ui.rs:274`, `ui.rs:284`, `ui.rs:292` (low)

- **Who and how:** anyone. The board checks no signature and `claimPub` is public, so an attacker posts an ad under the provider's key with other terms, for example a low fee.
  - `state.rs` correctly refuses to take that ad as the provider's own (`my_ad`) and raises `ad_contested`.
  - The Market page instead tests `a.claim_pub == st.id.claim_pub`. It draws the impostor with the "▶ " mark in bold accent, exactly like the real ad.
  - When the real ad is missing, the forged row is the only one marked as the operator's.
- **Effect:** the screen shows "your" ad with terms the provider never set. The header's warning says otherwise, so the two parts of the screen contradict each other.
- **Certainty:** high, from reading the code.

### F5. Waiting demand is unauthenticated and drives the Advisor's fee and size advice; more than 5,000 wants blinds the Demand page: `advisor.rs:41-77`, `wire.rs:596-603`, `http.rs:96` (low; a gap in the claims rather than a code bug)

- **Steering the advice:** wants are posted without permission and carry no identity, and there is no de-duplication or per-poster limit. Some of the advice this drives:
  - An attacker posts uncapped wants. These "take any fee" (`max_fee_bps: None`), so the "best" fee becomes the top of the ladder. The demo already shows "5.00% would earn the most".
  - An attacker posts capped wants to push the advice toward a low fee.
  - Huge wants produce "A max of X would put all of them in range" and "N more TAC would cover".
- **Blinding the page:** posting more than `MAX_ITEMS` (5,000) wants makes every `/wants` read fail by design. The page then shows "the board does not answer" for as long as the flood stays up.
- **What SECURITY.md says:** its non-coverage list names forged ads but not forged demand.
- **Certainty:** high on the behaviour, from reading the code. How much effect it has on operators is a judgement.

### F6. A capacity figure from the log can be drawn with no age: `ui.rs:75`, `ui.rs:86`, `wire.rs:617-627` (low)

- **Who and how:** the provider's log format. `at` is `None` when `ts` is missing or not exactly `YYYY-MM-DDTHH:MM:SS…`.
  - In that case the header shows `(N open, M holds · log)` and `free … (from the log)` with no age.
  - When `log_cmd` has died, these figures can be arbitrarily old and still look current.
- **Other cases:**
  - A `ts` with a non-UTC offset is misread by the size of the offset.
  - A `ts` in the future saturates to "0s ago".
- **Effect:** the claim promises that a log figure "gives its age", and here it does not. The demo shows the no-age form.
- **Certainty:** high (read the code, and saw it in the `--demo` snapshot).

### F7. A single board's failure is silent when several boards are configured, and the same entry on two boards is counted twice: `http.rs:94-97` (low)

- **Who and how:**
  - **Failure:** with more than one board configured, a board that fails, answers non-2xx or floods is skipped with `continue`. The joined list is still `Ok`, so `board_ok` stays true and the page shows part of the market as all of it. The comment at `http.rs:95` gives exactly this as the reason for refusing a flooded list.
  - **Duplicates:** an entry mirrored on two boards is counted twice in demand totals, the Advisor's volumes and `ad_contested`.
- **Scope:** the default config has one board, which limits the exposure.
- **Certainty:** high, from reading the code.

### F8. The client is built with `expect`, so the process aborts on a machine with no CA roots or a builder error: `http.rs:20` (low)

- **Who and how:** the environment, which is trusted but can be misconfigured.
  - `rustls-platform-verifier` loads the system roots eagerly in `Verifier::new` and errors when there are none. Examples: a minimal container, or `SSL_CERT_FILE=/dev/null` with no default bundle. reqwest's `build()` passes that error on and `expect("http client")` aborts.
  - This happens even when every configured URL is plain HTTP.
- **Effect:** no panic is reachable from untrusted input. Startup fails with a panic instead of an error message.
- **Certainty:** read the dependency sources. I did not run it, because live mode needs the network.

### F9. With a proxy set, requests to the local provider API also go through the proxy: `http.rs:19-21` (low)

- **Who and how:** reqwest uses the system proxy matcher by default (hyper-util `Matcher::from_env`). It does not exempt loopback addresses.
  - With `HTTP_PROXY` set and no `NO_PROXY`, the `GET`s to `http://127.0.0.1:8787/info` and `/health` go to the proxy. Its operator then sees the polling and can answer them, including a healthy `/health`.
- **What the claim leaves out:** claim 4 names only the uppercase variables. The lowercase ones count too, `NO_PROXY` is honoured, and proxies are ignored when `REQUEST_METHOD` is set (CGI).
- **Certainty:** read the dependency source. Not run.

### F10. A swap record can grow between the size check and the read: `local.rs:325-329` (low)

- **Who and how:** whoever can write the provider's `dataDir`, in practice the provider or its host.
  - `metadata()` (which follows symlinks) is checked first, then `tokio::fs::read` reads the whole path with no limit.
  - If the file is swapped for a large one, or a symlink is retargeted to a device, between those two calls, the read is unbounded.
- **Effect:** narrow and needs write access to the directory. The claim's "skipped over 1 MiB" is not enforced at the read itself; reading through a handle capped with `take(MAX_RECORD + 1)` would close it.
- **Certainty:** read only.

### F11. `--wait` with a huge value panics: `shell/mod.rs:86` (info)

- **Exact input:** `tacmon --demo --snapshot overview --wait 18446744073709551615`.
- **What happens:** `Instant::now() + wait` overflows, the panic message is "overflow when adding duration to instant", and the process aborts with exit 134. The CLI is trusted, so this is info only.
- **Related:** `shell/mod.rs:188` computes `header_height(..) + 2` in `u16` and can return `u16::MAX`. That would wrap in release and panic in debug, but the header has only about 15 groups, so it is not reachable.
- **Certainty:** ran it.

### F12. The build compiles C through a dependency's build script: `Cargo.toml:24` (reqwest `rustls`, which pulls in `aws-lc-rs`/`aws-lc-sys`) (info)

- **What:** `aws-lc-sys` runs a build script that compiles C, and its licence is a compound `ISC AND … AND MIT-0` expression.
- **Effect:** claim 3 is about this crate and stays true. But "no unsafe" leaves out a large native dependency in the TLS path.
- **Certainty:** `cargo tree`.

### F13. Smaller ways the screen can mislead (info)

- **The ETH balance:** converted through `f64` (`http.rs:127`). This is display-only, but it is the one floating-point amount in the program.
- **A failed health read:** drawn as an unknown "○" rather than as failed (`state.rs:110`, `widgets.rs:439`). The red "/info" text usually covers it, and the same goes for ETH "?".
- **Folded log lines:** lines are folded by `msg` and level alone (`state.rs:176-184`). Two different events with the same message, for example per-swap warnings, show as `×2` with only the last one's detail.
- **A swap with no readable deadline:** a deadline missing or in a non-integer format reads as 0 and is drawn "—" (`wire.rs:588`, `ui.rs:158`), so an open swap's deadline warning never fires.
- **Restarting `log_cmd`:** each restart replays `-n 2000` lines into the feed (`local.rs:55-86`), so the same lines appear again.

## 5. What I did not look at

- I could not run cargo-deny or the advisory database, so I did not check for advisories.
- I did not check the real board's limits on ad size or extra fields, which decide whether F1 is reachable.
- I did not exercise live mode against a local fake provider. That would need the binary to contact the default public board unless the config is changed, and I kept to `--demo` runs.
