# tacmon

<!-- This file stands alone: everything a contributor needs is here. -->

> `../shared` is an optional sibling checkout that most contributors will not have. Nothing here depends on it.

## Overview

`tacmon` is a terminal activity monitor for operators of Tacit services, in the spirit of `htop` and `k9s`: it watches what the operator runs, on the machine it runs on, and is not a view of the network. Two levels: a
**monitor** per kind of service, then that monitor's **pages**. One monitor exists, `fast-bridge`, for a fast bridge
liquidity provider. Every monitor is read only: it never sees a service's key and never changes a service. What needs the key, it prints as a
command for a helper in `scripts/helpers/`.

## Working Agreements

For anyone changing this repo, human or agent.

- **Ask when it is unclear.** Do not guess at intent or architecture. Working unattended, take the most reasonable reading, carry on, and write the assumption down.
- **Fit the solution to the problem.** The simplest thing that works. No flexibility that nothing needs yet.
- **Leave unrelated code alone**, but say so when you find something wrong in it, so it can be its own change.
- **Say what you are unsure of.** A small, low-risk experiment and its result is better than a confident guess.
- **Better ideas are welcome**, most of all ones that outlast the task at hand.
- **Report briefly.** Lead with the result. Length follows consequence, not effort; do not narrate the work.

## Repo Structure

| Path | |
|---|---|
| `src/main.rs` | the composition root: arguments, config, monitors, shell |
| `src/config.rs` | the TOML file: `[theme]` plus one section per monitor |
| `src/fmt.rs` | formatting every monitor shares |
| `src/shell/` | the terminal, the event loop, the shared keys, the frame around a monitor |
| `src/shell/monitor.rs` | the `Monitor` trait: the port every monitor implements |
| `src/shell/splash.rs` | the animated start screen: the Tacit mark in braille, drawn from the brand SVG's geometry |
| `src/shell/widgets.rs` | drawing pieces monitors share, so pages look like one tool |
| `src/monitors/mod.rs` | `create_monitors`: the one place that lists the monitors |
| `src/monitors/fast_bridge/model.rs` | the domain: no I/O, no wire formats |
| `src/monitors/fast_bridge/ports.rs` | one trait per outside capability |
| `src/monitors/fast_bridge/adapters/` | `http`, `local`, `memory`; `wire.rs` reads the provider's JSON; `mod.rs` chooses |
| `src/monitors/fast_bridge/feed.rs` | one task per port, sending events |
| `src/monitors/fast_bridge/state.rs` | what the pages draw, and what each event does to it |
| `src/monitors/fast_bridge/advisor.rs` | fee and size arithmetic, pure |
| `src/monitors/fast_bridge/ledger.rs` | the Bitcoin transactions of the swaps and the addresses as rows, and which are stuck, pure |
| `src/monitors/fast_bridge/upkeep.rs` | what keeping the provider stocked takes now, and the commands for it, pure |
| `src/monitors/fast_bridge/ui.rs` | the header and one function per page |
| `scripts/helpers/fast-bridge` | runs tacit's upkeep tools with the provider's key: the one file here that handles a key |
| `scripts/backup-swaps.sh` | snapshots a provider's swap records: the one piece of state a provider cannot lose |
| `SECURITY.md` | the threat model and the numbered claims the reviews attack |
| `audits/` | the review method, the scope, the prompts and one directory per past review |
| `deny.toml` | what the dependency tree is held to |

## Commands

- `make build`, `make test`, `make fmt`, `make lint`, `make run`, `make demo`
- `make demo-video` records `docs/demo/demo.gif` and `demo.mp4`: `scripts/record-demo.py` drives `tacmon --demo` in a pseudo-terminal, `agg` renders the GIF and `ffmpeg` converts it. Re-record after a change to a page or the start screen.
- `target/release/tacmon --snapshot [monitor/]<page>` draws one page to stdout as plain text: the way to check a page without a terminal. Add `--demo` for fixed data. `--snapshot splash` prints the start screen.
- `tacmon --print-config > config.example.toml` after changing a default.
- `make audit-checks` runs the security checks with one right answer; `make audit` adds the adversarial reviews and writes `audits/<date>/AUDIT.md`.

## Audits

- The method, prerequisites and provenance spec are `audits/README.md`. The claims under review are `SECURITY.md`.
- The scope is `audits/SCOPE`, one file per line. A new `.rs` file goes in it, or is excluded there with a reason, in the same change: `make audit-checks` fails otherwise.
- A change that makes a claim in `SECURITY.md` untrue changes the claim in the same commit.
- A new dependency licence, a git dependency or an ignored advisory is an edit to `deny.toml` with its reason.
- `make audit` scrubs this machine's paths from what it writes (`scripts/audit-scrub.sh`). Never commit an `audits/<date>/` that step did not pass.
- Audits are append-only: a past `audits/<date>/` is never edited. A wrong finding is corrected in the next `AUDIT.md`, marked as superseding.
- A fix for a finding comes with a test that fails without it, and a row in `audits/TRIAGE.md`.

## Conventions

- **Ports and adapters, in Rust terms.** A port is a trait in the monitor's `ports.rs`. An adapter is a struct in `adapters/` built by a `create_*` function. Only `adapters/mod.rs` and `main.rs` name concrete adapters; `feed`, `state` and `ui` see ports and the model.
- **The memory adapter is the contract.** Tests substitute it for the real ones. A new port gets its memory implementation in the same change.
- **A new monitor** is a directory under `src/monitors/` with the same shape, a section in `Config`, and a line in `create_monitors`. Nothing in `src/shell/` changes.
- **Wire formats stay in `adapters/wire.rs`.** The provider's log messages and record fields are not a stable interface: read them leniently, and show what is not recognised as it is.
- **A failed read is `None`, never a made-up zero.** The state keeps the last value and marks the source as not answering. A failed status, or an answer missing what it must carry, is a failed read.
- **The Advisor is about being chosen, not about earning the most.** It marks the fee that matches the cheapest other provider and names a fee the app would flag as high (`advisor::HIGH_FEE_BPS`, the app's `warnFeeBps`); it never recommends raising a fee.
- **The board carries every pair.** An ad or a want is this provider's market only when its asset is the one `/info` names; `State::sift` keeps those and counts the rest.
- **TAC amounts come from swap records, never from the chain.** An explorer sees sats; a row no swap names shows sats and says a note may ride on it.
- **What an explorer is asked is bounded and said.** Two addresses, the transactions of `ledger::MAX_SWAPS` swaps, `feed::MAX_LOOKUPS` by id a round, none again once `ledger::SETTLED` deep, and whether a note sent away from the main address is spent for `feed::MAX_AWAY` transactions a round, none again once it is. The Ethereum RPC is asked for the pool's lock log over at most `ledger::POOL_LOOKBACK` blocks, by contract and topic only: whose a lock is gets decided here, from the swap record's leaf, and never by a filter sent to the node. A new kind of request is a change to claim 4 in `SECURITY.md`.
- **Amounts from outside never wrap or truncate.** Sum with `model::total`, multiply through `model::fee_on`, narrow with `try_from`.
- Amounts are TAC base units (8 decimal places) in `u128`; format them with `model::tac`.
- Nothing leaves the machine that the operator did not configure.
- **Swapping what is behind a port touches one directory.** If changing a transport or a data source forces edits in `feed`, `state` or `ui`, the port is leaking: fix that first.
- **The first adapter is a working local one, not a mock.** Tests swap an adapter, never the network, the clock or the filesystem. Reaching for heavy mocking means the boundary is in the wrong place.
- **Keep what is public small.** `pub` only where another module needs it. Shared types live in one place.
- **Set the pattern before the second copy.** The next monitor follows `fast_bridge`'s shape; change the shape for both rather than fork it.
- Tests are Arrange-Act-Assert, with a blank line between the three.

### Comments

- Say why, not what. Code that reads clearly needs none.
- Write down what the types do not show: a convention relied on, an ordering that matters, a failure swallowed on purpose, a fallback.
- One line where possible. No section banners. Doc comments (`///`) for what other modules call, not for private helpers.

### Documentation

A change is not done until the documents describing what it changed are updated in the same change: `README.md`, this file, `SECURITY.md`, `config.example.toml`. A change that touches none of them can say so.

## Commits

- **Conventional commits:** `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`, with an optional scope (`fix(advisor): …`). A breaking change takes `!` and a `BREAKING CHANGE:` footer saying what changed and how to migrate.
- **Subject under 72 characters.** For a small change that is the whole message.
- **A body only when the subject is not enough:** a sentence or two of why. Not the diff, not what was tried first, not how it was verified.
- Point at updated documents where it helps: `See: SECURITY.md`.
- An agent ends its commits with `Co-Authored-By: <model> <noreply@anthropic.com>`, and commits or pushes only when asked.
- Contributors work on a branch and open a pull request. CI (`.github/workflows/ci.yml`) runs `make lint`, `make test` and a locked release build on every pull request; all three must pass.
- **Hooks check and report; they never rewrite files.** `scripts/pre-commit.sh` runs `cargo fmt --check` and clippy. Run `make fmt` yourself.
- **Nothing private in a tracked file:** no keys, tokens, RPC URLs with credentials, or personal details. A hook scans each commit for secrets.
- **A plain clone must work.** A tracked file may reach for `../shared`, but a missing checkout is a no-op, never a failure. Without it `make init` installs the devbox packages and the git hooks itself.

## Gotchas

- `make audit` starts other agents in this tree and refuses to run while `.audit-leg/` exists.
- Port methods return boxed futures (`ports::Fut`) so the traits stay object-safe; `async fn` in a trait would not be.
- `Monitor::start` and `LogFeed::start` spawn tasks, so they must be called inside the tokio runtime.
- The provider's log is read through `journalctl` by default; the swap records need read access to the provider's `dataDir`, `swaps/` and `reverse/swaps/` both.
- A swap's direction decides its phases, when it counts as paid (reverse: at `claimed`, before the bridger takes the note) and which way `pays` and `gets` run. Use `Swap::paid` and `Swap::closed`, never a phase name.
- `--snapshot` output has no colour, so a colour-only change cannot be checked with it.
- `agg` needs a font with braille glyphs or the start screen records blank: pass one in `AGG_FLAGS` (DejaVu Sans Mono has none; DejaVu Sans as a fallback does).
- A command the Maintenance page prints is built only from values `wire.rs` held to hex, decimal or bech32. A new value in one gets the same check, and a test with shell characters in it.
- What needs the key comes through the `Inventory` port, behind `status_cmd`, and the `SilentCheck` port, behind `normalize_cmd`: a child process holds the key, tacmon reads its output. The only argument tacmon ever adds to either is `--txid <64 hex>`, which keeps the tool a dry run. Never read `FB_LP_KEY` or the environment file in the crate.
- tacit's upkeep tools exit silently when started through a symlinked path: the helper `cd`s to the real path first.
- `config.example.toml` is generated: edit the defaults in the `config.rs` files, not the file.
