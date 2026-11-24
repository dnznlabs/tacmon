# tacmon

<!-- Project-specific only. Org-wide rules belong in ../shared/ORG.md. -->

> `../shared` is an optional sibling checkout. Without it, only this file applies.

## Overview

`tacmon` is a terminal activity monitor for Tacit services, in the spirit of `htop` and `k9s`. Two levels: a
**monitor** per kind of service, then that monitor's **pages**. One monitor exists, `fast-bridge`, for a fast bridge
liquidity provider. Every monitor is read only: it never sees a service's key and never changes a service.

## Repo Structure

| Path | |
|---|---|
| `src/main.rs` | the composition root: arguments, config, monitors, shell |
| `src/config.rs` | the TOML file: `[theme]` plus one section per monitor |
| `src/fmt.rs` | formatting every monitor shares |
| `src/shell/` | the terminal, the event loop, the shared keys, the frame around a monitor |
| `src/shell/monitor.rs` | the `Monitor` trait: the port every monitor implements |
| `src/shell/widgets.rs` | drawing pieces monitors share, so pages look like one tool |
| `src/monitors/mod.rs` | `create_monitors`: the one place that lists the monitors |
| `src/monitors/fast_bridge/model.rs` | the domain: no I/O, no wire formats |
| `src/monitors/fast_bridge/ports.rs` | one trait per outside capability |
| `src/monitors/fast_bridge/adapters/` | `http`, `local`, `memory`; `wire.rs` reads the provider's JSON; `mod.rs` chooses |
| `src/monitors/fast_bridge/feed.rs` | one task per port, sending events |
| `src/monitors/fast_bridge/state.rs` | what the pages draw, and what each event does to it |
| `src/monitors/fast_bridge/advisor.rs` | fee and size arithmetic, pure |
| `src/monitors/fast_bridge/ui.rs` | the header and one function per page |

## Commands

- `make build`, `make test`, `make fmt`, `make lint`, `make run`, `make demo`
- `target/release/tacmon --snapshot [monitor/]<page>` draws one page to stdout as plain text: the way to check a page without a terminal. Add `--demo` for fixed data.
- `tacmon --print-config > config.example.toml` after changing a default.

## Conventions

- **Ports and adapters, in Rust terms.** A port is a trait in the monitor's `ports.rs`. An adapter is a struct in `adapters/` built by a `create_*` function. Only `adapters/mod.rs` and `main.rs` name concrete adapters; `feed`, `state` and `ui` see ports and the model.
- **The memory adapter is the contract.** Tests substitute it for the real ones. A new port gets its memory implementation in the same change.
- **A new monitor** is a directory under `src/monitors/` with the same shape, a section in `Config`, and a line in `create_monitors`. Nothing in `src/shell/` changes.
- **Wire formats stay in `adapters/wire.rs`.** The provider's log messages and record fields are not a stable interface: read them leniently, and show what is not recognised as it is.
- **A failed read is `None`, never a made-up zero.** The state keeps the last value and marks the source as not answering.
- Amounts are TAC base units (8 decimal places) in `u128`; format them with `model::tac`.
- Nothing leaves the machine that the operator did not configure.
- Tests are Arrange-Act-Assert, with a blank line between the three.

## Gotchas

- Port methods return boxed futures (`ports::Fut`) so the traits stay object-safe; `async fn` in a trait would not be.
- `Monitor::start` and `LogFeed::start` spawn tasks, so they must be called inside the tokio runtime.
- The provider's log is read through `journalctl` by default; the swap records need read access to the provider's `dataDir`.
- `--snapshot` output has no colour, so a colour-only change cannot be checked with it.
- `config.example.toml` is generated: edit the defaults in the `config.rs` files, not the file.
