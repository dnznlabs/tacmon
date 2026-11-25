# The fast bridge monitor

`tacmon`'s monitor for one fast bridge liquidity provider: what it reads, what each page shows, the settings that
turn on what is off by default, and how to keep the provider stocked from the Maintenance page. The short version
is in the [README](../README.md#fast-bridge).

| | |
|:---|:---|
| [What it reads](#what-it-reads) | the provider's log, API and records, and what it assumes about them |
| [Pages](#pages) | the seven pages and the header |
| [Settings](#settings) | the RPC, the explorer and the pool contract, each off until set |
| [Transactions](#transactions) | the Tx page: Bitcoin rows, stuck ones, pool locks |
| [Silent sends to the reverse side](#silent-sends-to-the-reverse-side) | TAC waiting for `normalize` |
| [Maintenance](#maintenance) | the commands the page prints, and the helper that runs them |
| [The provider as a service](#the-provider-as-a-service) | systemd, the way the defaults expect |
| [What the Ethereum side can show](#what-the-ethereum-side-can-show) | why only the pool lock is listed |

## What it reads

The provider's log (`journalctl` by default), its local API (`/info`, `/health`, `/reverse/info`), its swap records in both directions, and the public board. Nothing else unless a setting below turns it on.

A provider sells in both directions once its reverse side is on (`FB_LP_REVERSE=on`). Forward is a bridger's TAC on Bitcoin for pool TAC; reverse is pool TAC for a TAC note on Bitcoin, from the reverse side's own address. tacmon reads the reverse side's terms from `/reverse/info` and its swaps from `dataDir/reverse/swaps/`.

### What it assumes

A provider installed as Tacit's own instructions describe, [Run one that stays on](https://github.com/src-company/tacit/blob/main/fastbridge-lp/README.md#run-one-that-stays-on), and then kept running as a systemd service with its data in `/var/lib`. Tacit's instructions stop at running it from a terminal; [The provider as a service](#the-provider-as-a-service) below is the step from there to what the defaults expect. Set up that way, no configuration is needed.

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

## Pages

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

## Settings

Optional: `~/.config/tacmon/config.toml`, or `--config FILE`, under `[fast_bridge]`. [`config.example.toml`](../config.example.toml) is every setting at its default. Each of these sends something somewhere, so each is empty until you set it. Two more run a command with the provider's key and are empty by default too: `status_cmd` ([Maintenance](#maintenance)) and `normalize_cmd` ([Silent sends](#silent-sends-to-the-reverse-side)).

`fast_bridge.eth_rpc` is empty by default, so the pool account is sent to no RPC and its ETH balance is not shown. Set a URL to show it.

`fast_bridge.btc_explorer` is empty by default too, so the provider's Bitcoin addresses and its swaps' transaction ids are sent nowhere: the Maintenance page lists no coins, and the Transactions page lists what the swap records name with no status. Set an Esplora API (`https://mempool.space/api`) to read both. The explorer is then asked for what is unspent and what has happened lately at the two addresses, for the transactions of the 50 most recent swaps by id, and for the going fee rate. `fast_bridge.reverse_address` names the reverse side's address before the side is on; once it is, the log names it.

`fast_bridge.pool_contract` is empty by default as well, so the pool's log on Ethereum is not read. Set it to the pool's address (`0x000000000Ed1eabD231Be41d93b719056F7febFC` on mainnet), with `eth_rpc` set, and that RPC is asked each minute for the newest block's number and for the locks put into the pool since the last read, a day back at first. The request names the pool contract, which every provider shares, and nothing of yours: which locks are yours is told on this machine, from your swap records. `fast_bridge.eth_explorer` (`https://etherscan.io`) is only the start of the link the Transactions page copies; nothing is ever requested from it.

## Transactions

A row is a transaction one of your recent swaps names (the bridger's lock, your take and its commit; in reverse your lock, its commit and any refund), or one an explorer lists at your main or reverse address. TAC amounts come from the swap record, never the chain: an explorer sees a 546-sat output, not the TAC riding on it, so a transfer no swap names shows its sats and `note` where TAC may be riding.

An unconfirmed transaction is marked **stuck** when it has waited longer than `fast_bridge.advisor.tx_stuck_secs` (30 minutes) since tacmon first saw it, or sooner, after about a block (10 minutes), when it pays under 80% of the rate that confirms within about an hour. The margin is there because explorers' estimates differ several times over. Each stuck one of yours is also a line on the Maintenance page; a bridger's lock is the bridger's to bump and stays off it. tacmon bumps nothing: the helper's `bump` replaces a transaction the reverse key sent, and the page prints that command only then. Anything else is for the wallet that sent it.

With `pool_contract` set, a forward swap's **pool lock** is a row too: the Ethereum transaction that put the TAC you lend into the pool for the bridger, with its depth in blocks and the amount from the swap record (the pool hides amounts). That is the one Ethereum transaction tacmon can find without an indexer ([why](#what-the-ethereum-side-can-show)). The pool's log names a lock by its leaf, which the swap record carries; a claim or a refund is written to the log with nothing that names the swap, and a relay sends all three, so none of them is at your account either. What follows from that:

- A lock is listed for as long as the log is read back: a day, or less where the node allows less. Free public nodes allow anything from 50 blocks to about a day and a half; a node that refuses a day is asked for half as much, and half again, down to 32 blocks, after which tacmon has only what it saw while running. On such a node a pause longer than it allows (a closed lid) starts the read over, and locks from before the pause are no longer listed. The page's title says how far back the read reaches.
- A lock that failed leaves nothing in the log. A forward swap that tacmon has seen `locking` for longer than `tx_stuck_secs`, with no lock in a log read back to before the swap began, is a line on the Maintenance page.
- Reverse swaps have no Ethereum row: there the pool lock is the bridger's, and the record does not carry its leaf.

The copied link is `btc_explorer` with a trailing `/api` removed, then `/tx/<id>`: right for mempool.space and blockstream.info. An Esplora served at the root of its host has no such page, and the link opens its JSON. A pool lock's link is `eth_explorer`, then `/tx/<hash>`.

The rest of the Ethereum side is not shown: see [What the Ethereum side can show](#what-the-ethereum-side-can-show).

## Silent sends to the reverse side

TAC sent to the reverse side silently (to its `tcs1` address, or by a wallet that sends that way) lands at a one-time address of the reverse key, which the reverse side does not count until `normalize` moves it. Only the key can tell such a send from a note you sent to someone else: on the chain both are a transaction at your main address that spends a note and pays a note-sized output to an address that is neither of yours. So tacmon flags nothing by that shape alone.

With `fast_bridge.normalize_cmd` set (`["/path/to/tacmon/scripts/helpers/fast-bridge", "normalize"]`), tacmon runs that command once for each such transaction that is two blocks deep and whose output is still unspent, adding `--txid <id>` and nothing else: the tool's dry run, which sends nothing. Where the tool finds TAC for the reverse key, the Maintenance page says how much and prints the `normalize` command; where it finds none, the transaction is never mentioned. The line goes once the note has moved. A command that gives no answer (a wrong path, a refused `sudo`, an explorer that is down) is a line on the Maintenance page too, and is tried again every ten minutes.

Without `normalize_cmd` nothing is run and no such send is flagged. A silent send from a wallet other than your main address leaves nothing at your addresses for tacmon to start from.

## Maintenance

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

## The provider as a service

Tacit's own instructions, [Run one that stays on](https://github.com/src-company/tacit/blob/main/fastbridge-lp/README.md#run-one-that-stays-on),
get a provider running from a terminal with its data beside the code. `tacmon`'s defaults expect one step more:
the provider kept running by systemd, with its data in `/var/lib`. This is that step. It is one way to do it, not
the only one, and nothing here is part of Tacit.

Follow Tacit's instructions first, up to a `config.json` and a key, and start it once by hand to see it work.

### 1. A user, a data directory, a key file, the config

Use a user that exists for this and nothing else. `lp` below.

```bash
sudo install -d -o lp -g lp -m 700 /var/lib/tacit-fastbridge-lp
sudo install -d -o lp -g lp -m 700 /etc/tacit-fastbridge-lp
sudo install -o lp -g lp -m 600 /dev/null /etc/tacit-fastbridge-lp/env
sudo install -o lp -g lp -m 600 fastbridge-lp/config.json /etc/tacit-fastbridge-lp/config.json
sudoedit /etc/tacit-fastbridge-lp/env
```

The env file holds the key, the alert webhook if you use one, and where the config is:

```
FB_LP_KEY=<the key>
FB_LP_ALERT_WEBHOOK=<optional>
FB_LP_CONFIG=/etc/tacit-fastbridge-lp/config.json
```

Only that user can read it. The key is never in the unit file, the shell history or the config.

In `/etc/tacit-fastbridge-lp/config.json`, set `"dataDir": "/var/lib/tacit-fastbridge-lp"` and leave `"port": 8787`.

**Why the config goes there.** The provider reads one file: the path in `FB_LP_CONFIG`, or without it
`fastbridge-lp/config.json` under whatever directory it was started in. It looks nowhere else, and exits if the
file is missing. Beside the secrets, the config does not depend on the working directory and is out of the code
checkout. Remove or rename the copy in the checkout so nobody edits the one that is not read.

**Keep every setting in that file.** A `FB_LP_*` environment variable overrides the matching key without a
word, so a setting left in the env file makes a later edit to the config do nothing. The env file is for the
secrets and `FB_LP_CONFIG`. Both directions' terms belong in the config: the forward ones under `quote`, the
reverse ones under `reverse`, with `"enabled": true` to switch that side on.

### 2. The unit

`/etc/systemd/system/tacit-fastbridge-lp.service`:

```ini
[Unit]
Description=Tacit fast bridge liquidity provider
Wants=network-online.target
After=network-online.target

[Service]
User=lp
WorkingDirectory=/home/lp/tacit
EnvironmentFile=/etc/tacit-fastbridge-lp/env
ExecStart=/usr/bin/node fastbridge-lp/server.mjs
Restart=always
RestartSec=10
TimeoutStopSec=60
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
```

`WorkingDirectory` is the Tacit checkout and `ExecStart` the full path to `node` (`command -v node` as that user).

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now tacit-fastbridge-lp
journalctl -u tacit-fastbridge-lp -f      # the start line names the addresses to fund
```

### 3. tacmon

Run `tacmon` as the same user. With the unit named `tacit-fastbridge-lp`, the data in
`/var/lib/tacit-fastbridge-lp` and the port at 8787, it needs no configuration.

### Before you rely on it

- **The machine must stay up.** A provider that is off when a swap needs it loses that swap's funds. A laptop
  that sleeps, or a virtual machine on one, is not a place for this.
- **Back up the swap records**, to another disk: `make backup DEST=<dir>` in this repo, on a timer.
- **To change a setting**, edit `/etc/tacit-fastbridge-lp/config.json` and
  `sudo systemctl restart tacit-fastbridge-lp`. Look that no swap is open first, and read the terms back
  afterwards: `curl -s localhost:8787/info`, and `/reverse/info` with that side on.
- **To stop**, set the maximum amount to zero, wait for the open swaps to finish, then stop the unit. Tacit's
  instructions say how.

## What the Ethereum side can show

Written for [#6](https://github.com/dnznlabs/tacmon/issues/6), which asked whether the provider's Ethereum
transactions can be listed from the configured RPC alone, with no indexer and no change to the provider.
Checked against the mainnet pool in November 2026.

### The short answer

One of them can: the lock a forward swap puts into the pool. Nothing else.

### What was checked

A forward swap has three Ethereum transactions: the provider's lock of pool TAC for the bridger, the bridger's
claim of it, and the provider's refund if the deadline passes. The provider's own account also sends a few of its
own: a pull from a treasury, a deposit into the pool, a settle when the relay is not sending them.

| | findable from the RPC | why |
|---|---|---|
| pool lock (forward) | yes | the pool emits `LockLeavesInserted(uint256 indexed firstLockIndex, bytes32[] lockLeaves)`, and the swap record carries the same leaf as `lock.lockLeaf` |
| claim, refund | no | they spend a lock nullifier, which the pool writes to storage and emits nothing for. The provider finds them by reading the calldata of every pool transaction in the window |
| the account's own sends | no | an RPC has no list of an account's transactions. Token transfers are indexed by account, but they are a part of it, and the swap transactions are not the account's at all |
| a transaction that reverted | no | it leaves no log |
| pool lock (reverse) | no | the lock is the bridger's, and the provider's record does not carry its leaf |

A relay sends the lock, the claim and the refund. None of them is at the provider's account, so nothing indexed by
account leads to them, and none of them is the operator's to speed up.

### How far back a public node serves the log

`eth_getLogs` for one contract and one topic, on free endpoints with no key:

| node | allowed |
|---|---|
| publicnode | about 10,000 blocks (33 hours); a wider range needs a token |
| drpc | 100 blocks |
| 1rpc | 50 blocks |
| ankr, cloudflare, llamarpc | a key, a refusal, or no answer |

A refusal comes back as a JSON-RPC error with a 200, a 400 or a 403 depending on the node. A day of the whole
pool's lock events is under a hundred entries and some 20 KiB.

So tacmon reads a day back, halves that each time a node refuses, and on a narrow node has only what it saw
while it was running.

### What it would take to show the rest

- **An indexer** (an Etherscan-style API) lists the account's own sends. Decided against
  ([#9](https://github.com/dnznlabs/tacmon/issues/9)): it needs a key and tells one more party which account is
  watched, for a handful of transactions a provider's account sends in its lifetime, none of them a swap's.
- **The provider recording the hashes it sends** in its swap records would make every one of them a lookup by
  hash, reverted ones included. That is a change to tacit, not to tacmon: asked for upstream, and what tacmon
  would then build is [#12](https://github.com/dnznlabs/tacmon/issues/12).
