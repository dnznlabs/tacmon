# The BTC-ETH monitor

`tacmon`'s monitor for one provider of Bitcoin for ETH: a fast bridge provider run in pair mode
(`"pair": "btc-eth"`), which sells pool ETH for sats and, with its reverse side on, sats for pool ETH. The short
version is in the [README](../README.md#btc-eth).

| | |
|:---|:---|
| [What it reads](#what-it-reads) | the provider's log, API and records, and what it assumes about them |
| [Units](#units) | sats one way, pool ETH the other |
| [Pages](#pages) | the five pages and the header |
| [Settings](#settings) | the RPC and the explorer, each off until set |
| [What it does not show yet](#what-it-does-not-show-yet) | pool locks, upkeep commands, demand |

## What it reads

The provider's log (`journalctl` by default), its local API (`/info`, `/health`, `/reverse/info`), its swap
records in both directions (`dataDir/swaps/` and `dataDir/reverse/swaps/`), and the public board's ads. Nothing
else unless a setting below turns it on.

A provider that Tacit's `setup.mjs` made keeps its key in `dataDir/provider.key`. tacmon opens the two record
directories and nothing else in `dataDir`: that file is never read.

It is the same daemon as the TAC provider, with a key and a data directory of its own, so it is watched by a
monitor of its own. Run both and `m` switches between them.

### What it assumes

A provider set up with Tacit's [setup](https://github.com/src-company/tacit/blob/main/fastbridge-lp/README.md#quick-start-a-bitcoin-for-eth-provider)
and then kept running as a systemd service.

| ASSUMPTION | DEFAULT | SETTING |
|:---|:---|:---|
| Linux with systemd, the provider running as a unit | unit `tacit-fastbridge-lp-btc` | `btc_eth.log_cmd` |
| its API on this machine | `http://127.0.0.1:8788` | `btc_eth.lp_url` |
| its data directory, readable by you | `/var/lib/tacit-fastbridge-lp-btc` | `btc_eth.data_dir` |
| the public board | `https://tacit-fast-board.onrender.com` | `btc_eth.boards` |

A unit that fits the defaults:

```ini
[Service]
User=you
WorkingDirectory=/path/to/tacit
ExecStart=/path/to/node fastbridge-lp/setup.mjs run --data /var/lib/tacit-fastbridge-lp-btc --port 8788
Restart=always
```

## Units

Two, and every figure says which. **Sats** are whole. **ETH** is the pool's, counted to eight places and shown
to five, cut and never rounded up. BTC→ETH terms are in sats (what a user locks); ETH→BTC terms are in ETH.
A price is ETH per BTC.

tacmon reads no price of its own. The one in the header is the provider's: what its upkeep last valued its sats
at, or failing that the price it started with, each with its age.

## Pages

The header has a line for each direction, one for swaps and money, the status lights and where the provider is.

- **BTC→ETH**: what it can take now in sats (`/info`), then its pool ETH as the log's last `capacity` line put
  it, with that line's age. Under its own minimum it is listed as full by the app, and the header says so.
  `one swap can draw` is the most ETH a single lock can be given: a lock takes sixteen notes at most, so this
  can be under the total until the provider merges its notes. A provider whose count says it has no notes while
  it holds ETH has not counted them: that is a dash, not a provider that can fund nothing.
- **ETH→BTC**: on or off, what it can take in ETH, how many swaps are open of how many it allows, and the sats
  it sells from as its upkeep last counted them. While the side is off nothing counts them, and none are shown.
- **swaps**: the sats and ETH of every swap under way, then the provider's own books: fees earned, and the net
  after costs and losses. A loss is written in red, in capitals.
- **lights**: health, on the board, records read, and price. A transaction of the provider's own that is stuck
  is named beside them, in red. The price light is the log's word: red after the
  provider says it has no price (it then quotes nothing), hollow when the log this run has read does not say.

| KEY | PAGE | SHOWS |
|:---|:---|:---|
| `1` | Overview | open and finished swaps of both directions: phase, sats, ETH, the rate the swap trades at, the fee the provider's books counted for it, age and deadline; then the latest warnings |
| `2` | Log | the provider's log as a feed; `w` for warnings only |
| `3` | Market | every provider of the pair on the board, cheapest first; `d` switches direction |
| `4` | Books | the provider's own accounting: swaps counted, fees, costs, losses and net, its inventory and notes, the target, the lean and buybacks |
| `5` | Tx | the Bitcoin transactions its swaps name and, with an explorer set, the ones at its addresses: what each is to its swap, sats, confirmations, fee rate, age; `enter` copies the picked one's link |

A swap's **fee earned** is what the provider's upkeep logged when it counted the swap (`stats: a swap is
counted`): sats for BTC→ETH, ETH for ETH→BTC. It is a dash until the swap has ended and been counted, and for a
swap counted before the log this run has read begins (`journalctl -n 2000` by default). The totals on the Books
page do not depend on the log.

The **Books** page is empty until the provider's upkeep has made its first look, about twenty seconds after it
starts. A dry run keeps no books.

### Transactions

The **Tx** page lists the Bitcoin transactions of the 50 most recent swaps, open ones first, as the swap records
name them: a user's lock and the provider's take of it (BTC→ETH), the provider's lock and its ways back
(ETH→BTC). Without `btc_explorer` that is all it lists, with no status.

With an explorer it adds each one's status, fee rate and age, and the other transactions at the provider's main
and reverse addresses (a top-up, a move between the two). Sats with a sign are what the explorer saw move at the
provider's addresses, fees included; sats without one are the swap record's word for a transaction that pays or
spends a script of the swap and not an address of the provider's.

An unconfirmed transaction is **stuck** when it has waited longer than `tx_stuck_secs`, or ten minutes while
paying under four fifths of the rate that confirms within the hour. An explorer does not say when a transaction
was broadcast, so the wait is counted from when this run of tacmon first saw it. A user's stuck lock is the
user's to bump: it is marked on the page and not in the header.

## Settings

| SETTING | DEFAULT | WHAT IT TURNS ON, AND WHAT IT SENDS |
|:---|:---|:---|
| `eth_rpc` | empty | the pool account's own ETH (`gas ETH` in the header), which pays its gas when the relay does not. Sends the pool account to that RPC, in `eth_getBalance` |
| `btc_explorer` | empty | the status of the swaps' transactions and the provider's addresses on the Tx page. An Esplora API (`https://mempool.space/api`, `https://blockstream.info/api`). Sends the provider's main and reverse addresses and the ids of its recent swaps' transactions to that explorer |
| `tx_stuck_secs` | `1800` | how long a transaction may wait unconfirmed before it is marked as stuck |
| `pool_account` | empty | the account to ask about, when it should not be learned from the log's start line |
| `deadline_warn_secs` | `3600` | a swap's deadline turns red when it is closer than this |
| `refresh.*` | 5 s, 20 s, 5 s, 60 s, 120 s | how often the API, the board, the records, the RPC and the explorer are read. The explorer is never asked more often than every 30 s |

## What it does not show yet

- **Pool locks.** The provider's lock of ETH in the pool, on Ethereum, has no row on the Tx page: the pool's
  log is not read.
- **Upkeep commands.** The provider merges its own notes and moves its own coins; where a hand is needed (a
  top-up, a stuck send) this monitor names nothing to run.
- **Demand.** The board's waiting swaps are not read.
