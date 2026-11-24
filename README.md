# tac-mon (`tacmon`)

A terminal activity monitor for [Tacit](https://github.com/src-company/tacit) services, in the spirit of `htop` and `k9s`.

It is built in two levels: **monitors**, one per kind of service, and each monitor's **pages**. Today there is one
monitor, `fast-bridge`, for a fast bridge liquidity provider.

Read only. It never sees a service's key and never changes a service.

## Fast bridge

| Key | Page | Shows |
|---|---|---|
| `1` | Overview | open and finished swaps (phase, amounts, fee, deadline), latest warnings |
| `2` | Activity | the provider's log as a feed; `w` for warnings only |
| `3` | Demand | bridgers waiting on the board, and why you are or are not taking each |
| `4` | Market | every provider on the board, cheapest first |
| `5` | Advisor | fee and size against what is waiting now |

The header is on every page: TAC free / promised / held, TAC on Bitcoin (received, incoming), fees earned, health,
whether your ad is on the board, and your terms.

It reads the provider's log (`journalctl` by default), its local API (`/info`, `/health`), its swap records, and the
public board.

## Run

```
make build
./target/release/tacmon
./target/release/tacmon --demo                # made-up data: nothing is read from this machine or the network
./target/release/tacmon --snapshot demand     # one page as plain text, for scripts and bug reports
```

## Configure

Optional: `~/.config/tacmon/config.toml` (or `--config FILE`). `tacmon --print-config` prints the effective settings;
`config.example.toml` is the defaults. `[theme]` belongs to the tool, and each monitor has a section of its own
(`[fast_bridge]`).

`fast_bridge.eth_rpc` is empty by default, so the pool account is sent to no RPC and its ETH balance is not shown.
Set a URL to show it.

## Layout

Ports and adapters. A monitor's state and pages depend on its ports, never on where the data comes from.

| Path | |
|---|---|
| `src/main.rs` | the composition root: config, monitors, shell |
| `src/shell/` | the terminal, the shared keys, the frame; `monitor.rs` is the port a monitor implements |
| `src/monitors/mod.rs` | the monitors this build knows |
| `src/monitors/fast_bridge/ports.rs` | what the monitor needs from outside: provider, board, swap records, gas balance, log |
| `src/monitors/fast_bridge/adapters/` | HTTP, local (command and files), and in-memory (tests and `--demo`) |
| `src/monitors/fast_bridge/` | `model` (the domain), `feed` (drives the ports), `state`, `advisor`, `ui` |
