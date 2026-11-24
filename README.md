# DNZN // TACMON

A terminal activity monitor for Tacit services.

by **Ðenizen.** // dxnzn.wei

`tacmon` watches the services you run for [Tacit](https://github.com/src-company/tacit), in the spirit of `htop` and `k9s`. It is built in two levels: a **monitor** per kind of service, and each monitor's **pages**. Today there is one monitor, `fast-bridge`, for a fast bridge liquidity provider. It is read only: it never sees a service's key and never changes a service.

## Release Info

### Project Status

| STATUS | VERSION | MONITORS |
|:---|:---|:---|
| `pre-mvp` | `0.1.0-alpha` | `fast-bridge` |

This is pre-release software. The pages have been checked against one live provider and against made-up data (`--demo`); the interactive screen has had little use beyond that.

**Alpha software is alpha, act accordingly.** See [Disclaimer](#disclaimer).

### Disclaimer

Alpha software. No warranty. What `tacmon` shows is read from a provider's log, its local API, its swap records and the public board, none of which is a stable interface, so a figure can be stale or wrong after a provider update. Check a number against the provider itself before moving funds on it. The Advisor page describes what is waiting on the board now; it is not a forecast and not financial advice.

## Monitors

### Fast Bridge

For one fast bridge liquidity provider. It reads the provider's log (`journalctl` by default), its local API (`/info`, `/health`), its swap records, and the public board.

| KEY | PAGE | SHOWS |
|:---|:---|:---|
| `1` | Overview | open and finished swaps (phase, amounts, fee, deadline), latest warnings |
| `2` | Activity | the provider's log as a feed; `w` for warnings only |
| `3` | Demand | bridgers waiting on the board, and why you are or are not taking each |
| `4` | Market | every provider on the board, cheapest first |
| `5` | Advisor | fee and size against what is waiting now |

The header is on every page: TAC free / promised / held, TAC on Bitcoin (received, incoming), fees earned, health, whether your ad is on the board, and your terms.

## Usage

```bash
tacmon                        # Watch the services on this machine
tacmon --demo                 # Made-up data: nothing is read from this machine or the network
tacmon --snapshot demand      # One page as plain text, for scripts and bug reports
tacmon --no-splash             # Skip the animated start screen
tacmon --print-config         # The effective configuration, as TOML
```

### Configuration

Optional: `~/.config/tacmon/config.toml`, or `--config FILE`. [`config.example.toml`](config.example.toml) is the defaults. `[theme]` belongs to the tool, and each monitor has a section of its own (`[fast_bridge]`).

`fast_bridge.eth_rpc` is empty by default, so the pool account is sent to no RPC and its ETH balance is not shown. Set a URL to show it.

## Documentation

| DOCUMENT | DESCRIPTION |
|----------|-------------|
| [Working Conventions](AGENTS.md) | Repo structure, commands, conventions and gotchas for contributors, human or agent |
| [Example Configuration](config.example.toml) | Every setting, at its default |

## Development

### Common Helpers

```bash
make init           # Initialize the development environment (devbox, git hooks)
make build          # Build the release binary (target/release/tacmon)
make test           # Run the tests
make lint           # Check formatting and run clippy, as the pre-commit hook does
make run            # Run tacmon against the services on this machine
make demo           # Run tacmon on made-up data
make help           # List every command
```

Ports and adapters: a monitor's state and pages depend on its ports, never on where the data comes from, and an in-memory adapter backs the tests and `--demo`. See [Repo Structure](AGENTS.md#repo-structure).

## Credits

Tacit and its fast bridge are by Src Co / z0r0z; `tacmon` only watches them. The terminal interface is built on [ratatui](https://ratatui.rs).

## Development Team

| Name | GPG Key | GitHub | EOA |
|:-----|:--------|:-------|:----|
| Ðenizen. | [`0DC6C60CFBD5E456`](https://keys.openpgp.org/search?q=CD435BB6A7BED718D95616450DC6C60CFBD5E456) | [`dxnzn`](https://github.com/dxnzn) | [`0x3578...902B`](https://etherscan.io/address/0x357836fFcB45307FA7ff2A574F505753c961902B) *dxnzn.wei* |

## License

MIT — see [LICENSE](LICENSE).
