# DNZN // TACMON

A terminal activity monitor for operators of Tacit services.

by **Ðenizen.** // dxnzn.wei

`tacmon` is for operators: it watches the services **you run** for [Tacit](https://tacit.finance), on the machine you run them on, in the spirit of `htop` and `k9s`. It is not a way to watch Tacit in general: without a service of your own beside it, it has nothing to show. See [Who It Is For](#who-it-is-for).

It is built in two levels: a **monitor** per kind of service, and each monitor's **pages**. Today there is one monitor, `fast-bridge`, for a fast bridge liquidity provider. It is read only: it never sees a service's key and never changes a service. Where keeping a service stocked needs the key, it prints the command and a separate helper runs it ([Maintenance](#maintenance)).

![tacmon running on its demo data: the start screen, then each page of the fast bridge monitor](docs/demo/demo.gif)

## Who It Is For

| | |
|:---|:---|
| **For** | someone running a Tacit service who wants its state on one screen: what it holds, what it is doing, what needs attention |
| **Not for** | watching the Tacit network, another operator's service, or your own bridges as a user. For those, use the app at [tacit.finance](https://tacit.finance) |

`tacmon` reads a service's own log, local API and records, so it runs **on the same machine as the service**, as a user allowed to read them. The public board is the only thing it reads from elsewhere, and it shows that from your service's point of view: your ad, your rivals, the bridges you could take.

`tacmon --demo` runs on made-up data and needs none of this. It is the way to look before you run anything.

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

For one fast bridge liquidity provider. It reads the provider's log (`journalctl` by default), its local API (`/info`, `/health`, `/reverse/info`), its swap records in both directions, and the public board.

#### What It Assumes

A provider installed as Tacit's own instructions describe, [Run one that stays on](https://github.com/src-company/tacit/blob/main/fastbridge-lp/README.md#run-one-that-stays-on), and then kept running as a systemd service with its data in `/var/lib`. Tacit's instructions stop at running it from a terminal; [A fast bridge provider as a service](docs/fast-bridge-service.md) is the step from there to what the defaults below expect. Set up that way, no configuration is needed.

| ASSUMPTION | DEFAULT | SETTING |
|:---|:---|:---|
| Linux with systemd, the provider running as a unit | unit `tacit-fastbridge-lp` | `fast_bridge.log_cmd` |
| The provider's local API answers on this machine | `http://127.0.0.1:8787` | `fast_bridge.lp_url` |
| Its data directory can be read by the user running `tacmon` | `/var/lib/tacit-fastbridge-lp` | `fast_bridge.data_dir` |
| That user can read the unit's journal | member of `systemd-journal`, or the unit's own user | |
| The public board can be reached over HTTPS | Tacit's board | `fast_bridge.boards` |
| A terminal with UTF-8 and 256 colours. Any width works; the header wraps on a narrow one | | |

The simplest way to meet the two reading rows is to run `tacmon` as the user the provider runs as.

A provider run another way works once those settings point at it. One started from a terminal as Tacit's instructions show keeps its data in `fastbridge-lp/data` inside the checkout, and writes its log to that terminal: set `data_dir` to the first, and `log_cmd` to any command that prints the provider's log lines and keeps printing, such as `tail -F` on a file the log is saved to. Linux is what `tacmon` is built and tested on; nothing else has been tried.

What a missing piece looks like: a red cross beside `health` (the API does not answer), beside `records` (the data directory cannot be read), or a `log_cmd` warning on the Activity page (the journal cannot be read).

| KEY | PAGE | SHOWS |
|:---|:---|:---|
| `1` | Overview | open and finished swaps of both directions (way, phase, amounts, fee, deadline), latest warnings |
| `2` | Log | the provider's log as a feed; `w` for warnings only |
| `3` | Demand | bridgers waiting on the board, and why you are or are not taking each |
| `4` | Market | every provider on the board, cheapest first; `d` switches between the forward terms and the reverse ones |
| `5` | Advisor | the forward fee and size against what is waiting now, and the reverse fee against other providers' |
| `6` | Maint | each direction's state, terms and what is at its address, the TAC forward swaps left at the main address, and what to do next, as commands |
| `7` | Tx | the Bitcoin transactions of your recent swaps and at your two addresses: what each is, its amount, whether it has confirmed, and its fee rate against the going one; with a pool contract set, the lock each forward swap put into the pool on Ethereum; `↑↓` picks one and `enter` copies its link on your explorer (its bare id with no explorer set) |

The header is on every page, one line per direction. Forward: TAC free / promised / held, TAC on Bitcoin received and incoming, fees earned. Reverse: on, off or not answering, the pool TAC it can take now, open sends, pool TAC received and incoming, fees earned. Then health, whether your ad is on the board, and whether the swap records can be read. The terms of each direction are on the Advisor and Maintenance pages.

A provider sells in both directions once its reverse side is on (`FB_LP_REVERSE=on`). Forward is a bridger's TAC on Bitcoin for pool TAC; reverse is pool TAC for a TAC note on Bitcoin, from the reverse side's own address. tacmon reads the reverse side's terms from `/reverse/info` and its swaps from `dataDir/reverse/swaps/`.

## Usage

There are no packaged releases yet. Build it with a Rust toolchain: `make build` leaves the binary at `target/release/tacmon`.

```bash
tacmon                        # Watch the services on this machine
tacmon --demo                 # Made-up data: nothing is read from this machine or the network
tacmon --snapshot demand      # One page as plain text, for scripts and bug reports (pages by tab name: log, maint, tx; the longer old names still work)
tacmon --no-splash             # Skip the animated start screen
tacmon --print-config         # The effective configuration, as TOML
```

### Configuration

Optional: `~/.config/tacmon/config.toml`, or `--config FILE`. [`config.example.toml`](config.example.toml) is the defaults. `[theme]` belongs to the tool, and each monitor has a section of its own (`[fast_bridge]`).

`fast_bridge.eth_rpc` is empty by default, so the pool account is sent to no RPC and its ETH balance is not shown. Set a URL to show it.

`fast_bridge.btc_explorer` is empty by default too, so the provider's Bitcoin addresses and its swaps' transaction ids are sent nowhere: the Maintenance page lists no coins, and the Transactions page lists what the swap records name with no status. Set an Esplora API (`https://mempool.space/api`) to read both. The explorer is then asked for what is unspent and what has happened lately at the two addresses, for the transactions of the 50 most recent swaps by id, and for the going fee rate. `fast_bridge.reverse_address` names the reverse side's address before the side is on; once it is, the log names it.

`fast_bridge.pool_contract` is empty by default as well, so the pool's log on Ethereum is not read. Set it to the pool's address (`0x000000000Ed1eabD231Be41d93b719056F7febFC` on mainnet), with `eth_rpc` set, and that RPC is asked each minute for the newest block's number and for the locks put into the pool since the last read, a day back at first. The request names the pool contract, which every provider shares, and nothing of yours: which locks are yours is told on this machine, from your swap records. `fast_bridge.eth_explorer` (`https://etherscan.io`) is only the start of the link the Transactions page copies; nothing is ever requested from it.

### Transactions

A row is a transaction one of your recent swaps names (the bridger's lock, your take and its commit; in reverse your lock, its commit and any refund), or one an explorer lists at your main or reverse address. TAC amounts come from the swap record, never the chain: an explorer sees a 546-sat output, not the TAC riding on it, so a transfer no swap names shows its sats and `note` where TAC may be riding.

An unconfirmed transaction is marked **stuck** when it has waited longer than `fast_bridge.advisor.tx_stuck_secs` (30 minutes) since tacmon first saw it, or sooner, after about a block (10 minutes), when it pays under 80% of the rate that confirms within about an hour. The margin is there because explorers' estimates differ several times over. Each stuck one of yours is also a line on the Maintenance page; a bridger's lock is the bridger's to bump and stays off it. tacmon bumps nothing: the helper's `bump` replaces a transaction the reverse key sent, and the page prints that command only then. Anything else is for the wallet that sent it.

With `pool_contract` set, a forward swap's **pool lock** is a row too: the Ethereum transaction that put the TAC you lend into the pool for the bridger, with its depth in blocks and the amount from the swap record (the pool hides amounts). That is the one Ethereum transaction tacmon can find without an indexer ([why](docs/ethereum-transactions.md)). The pool's log names a lock by its leaf, which the swap record carries; a claim or a refund is written to the log with nothing that names the swap, and a relay sends all three, so none of them is at your account either. What follows from that:

- A lock is listed for as long as the log is read back: a day, or less where the node allows less. Free public nodes allow anything from 50 blocks to about a day and a half; a node that refuses a day is asked for half as much, and half again, down to 32 blocks, after which tacmon has only what it saw while running. On such a node a pause longer than it allows (a closed lid) starts the read over, and locks from before the pause are no longer listed. The page's title says how far back the read reaches.
- A lock that failed leaves nothing in the log. A forward swap that tacmon has seen `locking` for longer than `tx_stuck_secs`, with no lock in a log read back to before the swap began, is a line on the Maintenance page.
- Reverse swaps have no Ethereum row: there the pool lock is the bridger's, and the record does not carry its leaf.

The copied link is `btc_explorer` with a trailing `/api` removed, then `/tx/<id>`: right for mempool.space and blockstream.info. An Esplora served at the root of its host has no such page, and the link opens its JSON. A pool lock's link is `eth_explorer`, then `/tx/<hash>`.

The rest of the Ethereum side is not shown: the provider records no Ethereum transaction hashes, and the pool's log names nothing but a lock's leaf.

### Maintenance

The provider's two directions stock each other. Forward swaps take TAC on Bitcoin at its main address and pay pool TAC; the reverse side, once on, sells TAC on Bitcoin from an address of its own and takes pool TAC. Pool TAC needs no handling. TAC on Bitcoin reaches the reverse address only when it is moved there, and the reverse address needs plain sats for its fees.

The Maintenance page works out what is short and prints the command that fixes it, with the coins it names already filled in. `↑` and `↓` scroll; `shift+↑` and `shift+↓` (or `K` and `J`) pick a command, and `c` (or Enter) copies it, whole and ready to paste, through the terminal's clipboard (OSC 52). Most terminals allow it; some ship with it off, and in tmux it needs `set-clipboard on`. The commands run `scripts/helpers/fast-bridge`, from this checkout:

```bash
scripts/helpers/fast-bridge status                 # the reverse address, and the TAC and sats at it (reads only)
scripts/helpers/fast-bridge fund …                 # move up to 25 TAC of a forward take to the reverse address
scripts/helpers/fast-bridge topup --coin <txid:vout>   # move a plain coin of the main address there, for fees
```

How much TAC the reverse address holds takes the key to read. To have the page show it, set `fast_bridge.status_cmd` to the helper's read-only `status`, with the full path:

```toml
[fast_bridge]
status_cmd = ["/path/to/tacmon/scripts/helpers/fast-bridge", "status"]
```

tacmon runs it every five minutes (`refresh.status_secs`; each run takes about a minute) and reads what it prints. The key stays in that child process, never in tacmon's. With no terminal, the helper's `sudo` cannot ask for a password, so run tacmon as a user that can read the environment file, or allow that one `sudo grep` without one.

The helper reads `FB_LP_KEY` from the provider's environment file (`/etc/tacit-fastbridge-lp/env`, through `sudo` if need be) and runs tacit's own tools with it, from a tacit checkout beside this one (`TACIT_DIR` to name another). Each tool that sends prints its plan and a token, and sends only when run again with `--send --confirm <token>`.

## Security

tacmon is read only: the binary never sees a key, signs nothing and writes nothing. The one script that handles a key, `scripts/helpers/fast-bridge`, is run by hand and reviewed with the code. It is reviewed by its own developer, not by an outside firm, and the work is in the open so it can be checked:

- [SECURITY.md](SECURITY.md) states the threat model and eleven claims, each with what holds it up.
- `make audit-checks` holds the dependency tree to RustSec advisories, a licence list and crates.io only, and the crate to no `unsafe`.
- `make audit` has language models from different labs attack those claims independently, and records what ran and what they found in [`audits/`](audits/README.md).

## Documentation

| DOCUMENT | DESCRIPTION |
|----------|-------------|
| [Working Conventions](AGENTS.md) | Repo structure, commands, conventions and gotchas for contributors, human or agent |
| [Example Configuration](config.example.toml) | Every setting, at its default |
| [A Fast Bridge Provider as a Service](docs/fast-bridge-service.md) | Running a provider under systemd the way the fast bridge monitor's defaults expect |
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
