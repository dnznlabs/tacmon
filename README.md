# DNZN // TACMON

A terminal activity monitor for operators of Tacit services.

by **Ðenizen.** // dxnzn.wei

`tacmon` is for operators: it watches the services **you run** for [Tacit](https://tacit.finance), on the machine you run them on, in the spirit of `htop` and `k9s`. It is not a way to watch Tacit in general: without a service of your own beside it, it has nothing to show. See [Who It Is For](#who-it-is-for).

It is built in two levels: a **monitor** per kind of service, and each monitor's **pages**. Today there are two: `fast-bridge`, for a fast bridge liquidity provider, and `btc-eth`, for a provider of Bitcoin for ETH. Each is read only: it never sees a service's key and never changes a service. Where keeping a service stocked needs the key, it prints the command and a separate helper runs it ([Maintenance](docs/fast-bridge.md#maintenance)).

![tacmon running on its demo data: the start screen, then each page of the fast bridge monitor](docs/demo/demo.gif)

## Who It Is For

| | |
|:---|:---|
| **For** | someone running a Tacit service who wants its state on one screen: what it holds, what it is doing, what needs attention |
| **Not for** | watching the Tacit network, another operator's service, or your own bridges as a user. For those, use the app at [tacit.finance](https://tacit.finance) |

`tacmon` reads a service's own log, local API and records, so it runs **on the same machine as the service**, as a user allowed to read them. The public board is the only thing it reads from elsewhere, and it shows that from your service's point of view: your ad, your rivals, the bridges you could take.

Every monitor is **off until you turn it on** in the configuration, because each needs something of yours to watch. Run with none on and `tacmon` says where the configuration goes and which line turns each monitor on ([Configuration](#configuration)).

`tacmon --demo` runs every monitor on made-up data and needs none of this. It is the way to look before you run anything.

## Release Info

### Project Status

| STATUS | VERSION | MONITORS |
|:---|:---|:---|
| `pre-mvp` | `0.1.0-alpha` | `fast-bridge`, `btc-eth` |

This is pre-release software. The fast bridge pages have been checked against one live provider and against made-up data (`--demo`); the interactive screen has had little use beyond that. The `btc-eth` monitor is newer still: it has been checked against made-up data and a provider that had not yet taken a swap.

**Alpha software is alpha, act accordingly.** See [Disclaimer](#disclaimer).

### Disclaimer

Alpha software. No warranty. What `tacmon` shows is read from a provider's log, its local API, its swap records and the public board, none of which is a stable interface, so a figure can be stale or wrong after a provider update. Check a number against the provider itself before moving funds on it. The Advisor page describes what is waiting on the board now; it is not a forecast and not financial advice.

## Monitors

### Fast Bridge

For one fast bridge liquidity provider, in both directions: a bridger's TAC on Bitcoin for pool TAC (forward), and pool TAC for a TAC note on Bitcoin (reverse). It reads the provider's log, its local API, its swap records and the public board.

Turn it on with `enabled = true` under `[fast_bridge]`. It expects a provider set up as Tacit's instructions describe and kept running as a systemd unit named `tacit-fastbridge-lp`, with its API on `127.0.0.1:8787` and its data in `/var/lib/tacit-fastbridge-lp`. Set up that way, that one line is the whole configuration; anything else is a setting.

| KEY | PAGE | SHOWS |
|:---|:---|:---|
| `1` | Overview | open and finished swaps of both directions (way, phase, amounts, fee, deadline), latest warnings |
| `2` | Log | the provider's log as a feed; `w` for warnings only |
| `3` | Demand | bridgers waiting on the board, and why you are or are not taking each |
| `4` | Market | every provider on the board, cheapest first; `d` switches between the forward terms and the reverse ones |
| `5` | Advisor | the forward fee and size against what is waiting now, and the reverse fee against other providers' |
| `6` | Maint | each direction's state, terms and what is at its address, the TAC forward swaps left at the main address, and what to do next, as commands |
| `7` | Tx | the Bitcoin transactions of your recent swaps and at your two addresses, with their status and fee rate, and each forward swap's pool lock on Ethereum; `enter` copies a link to the picked one |

The Maintenance page prints the commands that keep the two directions stocked; `scripts/helpers/fast-bridge`, which holds the key, runs them, never tacmon.

**[The fast bridge monitor](docs/fast-bridge.md)** has the rest: what it assumes and how to point it at a provider run another way, every page, the settings, stuck and silent transactions, the Maintenance commands, and running the provider as a service.

### BTC-ETH

For one provider of Bitcoin for ETH (a fast bridge provider in pair mode), in both directions: a user's sats for pool ETH, and pool ETH for sats. It reads the provider's log, its local API, its swap records and the public board.

Turn it on with `enabled = true` under `[btc_eth]`. It expects a provider made with Tacit's `setup.mjs` and kept running as a systemd unit named `tacit-fastbridge-lp-btc`, with its API on `127.0.0.1:8788` and its data in `/var/lib/tacit-fastbridge-lp-btc`.

| KEY | PAGE | SHOWS |
|:---|:---|:---|
| `1` | Overview | open and finished swaps of both directions (way, phase, sats, ETH, the rate each trades at, the fee counted for it, deadline), latest warnings |
| `2` | Log | the provider's log as a feed; `w` for warnings only |
| `3` | Market | every provider of the pair on the board, cheapest first; `d` switches direction |
| `4` | Books | the provider's own accounting: fees, costs, losses and net, its ETH notes and sats, the target, the lean and buybacks |
| `5` | Tx | the Bitcoin transactions of its swaps and, with an explorer set, of its addresses: status, fee rate, and which are stuck |

**[The BTC-ETH monitor](docs/btc-eth.md)** has the rest: what it assumes, the two units, the header, the settings, and what it does not show yet.

## Usage

There are no packaged releases yet. Build it with a Rust toolchain: `make build` leaves the binary at `target/release/tacmon`.

```bash
tacmon                        # Watch the services on this machine: the monitors your configuration turns on
tacmon --demo                 # Made-up data: nothing is read from this machine or the network
tacmon --snapshot demand      # One page as plain text, for scripts and bug reports (pages by tab name: log, maint, tx; the longer old names still work)
tacmon --no-splash             # Skip the animated start screen
tacmon --print-config         # The effective configuration, as TOML
```

### Configuration

`~/.config/tacmon/config.toml`, or `--config FILE`. [`config.example.toml`](config.example.toml) is the defaults. `[theme]` belongs to the tool, and each monitor has a section of its own (`[fast_bridge]`, `[btc_eth]`).

Every monitor is off by default, so the file needs at least the line that turns one on:

```bash
mkdir -p ~/.config/tacmon
tacmon --print-config > ~/.config/tacmon/config.toml    # every setting at its default, the same as config.example.toml
```

```toml
[fast_bridge]
enabled = true
```

With no monitor on, `tacmon` starts nothing and reads nothing: it prints where the configuration is or goes and the line for each monitor, and exits with status 2. With more than one on, `m` switches between them.

Nothing leaves the machine that you did not configure. The settings that ask an outside service for something (an Ethereum RPC, a Bitcoin explorer) or run a command with the provider's key are empty by default, and each monitor's guide ([fast bridge](docs/fast-bridge.md#settings), [BTC-ETH](docs/btc-eth.md#settings)) says what each one sends.

## Security

tacmon is read only: the binary never sees a key, signs nothing and writes nothing. The one script that handles a key, `scripts/helpers/fast-bridge`, is run by hand (and by tacmon only in its two read-only forms, where you configure them) and reviewed with the code. It is reviewed by its own developer, not by an outside firm, and the work is in the open so it can be checked:

- [SECURITY.md](SECURITY.md) states the threat model and eleven claims, each with what holds it up.
- `make audit-checks` holds the dependency tree to RustSec advisories, a licence list and crates.io only, and the crate to no `unsafe`.
- `make audit` has language models from different labs attack those claims independently, and records what ran and what they found in [`audits/`](audits/README.md).

## Documentation

| DOCUMENT | DESCRIPTION |
|----------|-------------|
| [Working Conventions](AGENTS.md) | Repo structure, commands, conventions and gotchas for contributors, human or agent |
| [Example Configuration](config.example.toml) | Every setting, at its default |
| [The Fast Bridge Monitor](docs/fast-bridge.md) | What it reads and assumes, its pages and settings, transactions, Maintenance, running the provider as a service, and what of the Ethereum side can be shown |
| [The BTC-ETH Monitor](docs/btc-eth.md) | What it reads and assumes, its units, pages and settings, and what it does not show yet |
| [Security](SECURITY.md) | What tacmon is trusted with, the threat model, and the claims the reviews attack |
| [Audits](audits/README.md) | How the self-review works, its limits, and every past review |

## Development

### Common Helpers

```bash
make init           # Initialize the development environment (devbox, git hooks)
make build          # Build the release binary (target/release/tacmon)
make test           # Run the tests
make lint           # Check formatting and run clippy, as the pre-commit hook does
make run            # Run tacmon against the services on this machine
make demo           # Run tacmon on made-up data
make demo-video     # Record docs/demo/ (GIF and MP4) from the demo
make backup DEST=…  # Snapshot the provider's swap records to another disk (scripts/backup-swaps.sh)
make audit-checks   # Security checks: advisories, licences, sources, unsafe, clippy, tests
make audit          # The checks, then adversarial reviews, reconciled into audits/<date>/AUDIT.md
make help           # List every command
```

Ports and adapters: a monitor's state and pages depend on its ports, never on where the data comes from, and an in-memory adapter backs the tests and `--demo`. See [Repo Structure](AGENTS.md#repo-structure).

## Credits

[Tacit](https://tacit.finance) and its fast bridge ([source](https://github.com/src-company/tacit)) are by Src Co / z0r0z; `tacmon` only watches them. The terminal interface is built on [ratatui](https://ratatui.rs).

## Development Team

| Name | GPG Key | GitHub | EOA |
|:-----|:--------|:-------|:----|
| Ðenizen. | [`0DC6C60CFBD5E456`](https://keys.openpgp.org/search?q=CD435BB6A7BED718D95616450DC6C60CFBD5E456) | [`dxnzn`](https://github.com/dxnzn) | [`0x3578...902B`](https://etherscan.io/address/0x357836fFcB45307FA7ff2A574F505753c961902B) *dxnzn.wei* |

## License

MIT — see [LICENSE](LICENSE).
