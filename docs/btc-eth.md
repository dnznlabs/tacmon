# The BTC-ETH monitor

`tacmon`'s monitor for one provider of Bitcoin for ETH: a fast bridge provider run in pair mode
(`"pair": "btc-eth"`), which sells pool ETH for sats and, with its reverse side on, sats for pool ETH. The short
version is in the [README](../README.md#btc-eth).

| | |
|:---|:---|
| [What it reads](#what-it-reads) | the provider's log, API and records, and what it assumes about them |
| [Units](#units) | sats one way, pool ETH the other |
| [Pages](#pages) | the seven pages and the header |
| [Advisor](#advisor) | where the fee stands, and whether it covers what a swap costs |
| [Maintenance](#maintenance) | what is short, and the commands that fix it |
| [Settings](#settings) | the RPC, the pool contract and the explorer, each off until set |
| [What it does not show yet](#what-it-does-not-show-yet) | an overdue pool lock, demand |

## What it reads

The provider's log (`journalctl` by default), its local API (`/info`, `/health`, `/reverse/info`), its swap
records in both directions (`dataDir/swaps/` and `dataDir/reverse/swaps/`), the names and times of its quote
files (`dataDir/quotes/` and `dataDir/reverse/quotes/`, never their contents), and the public board's ads. Nothing
else unless a setting below turns it on.

A provider that Tacit's `setup.mjs` made keeps its key in `dataDir/provider.key`. tacmon opens the swap records,
lists the quote directories, and touches nothing else in `dataDir`: that file is never read.

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
- **fee**: where its BTC→ETH fee stands among the board's providers that would take a swap now, at the least
  and the most it takes itself: `cheapest of 2 at 25,000`. The app takes the quote that pays the most, so this
  is whether it is picked.
- **last quote kept**: when the newest file in the provider's quote directories was written. It is not the last
  quote given: a BTC→ETH quote is written only once a user holds or locks for it, and removed again when the hold
  lapses with no lock, while an ETH→BTC quote is written when given. `none` says only that no file is kept now, not
  that the provider is not asked. The provider writes nowhere how many quotes it has given.
- **lights**: health, on the board, records read, and price. A transaction of the provider's own that is stuck
  is named beside them, in red. The price light is the log's word: red after the
  provider says it has no price (it then quotes nothing), hollow when the log this run has read does not say.

| KEY | PAGE | SHOWS |
|:---|:---|:---|
| `1` | Overview | open and finished swaps of both directions: phase, sats, ETH, the rate the swap trades at, the fee the provider's books counted for it, age and deadline; then the latest warnings |
| `2` | Log | the provider's log as a feed; `w` for warnings only |
| `3` | Books | the provider's own accounting: swaps counted, fees, costs, losses and net, its inventory and notes, the target, the lean and buybacks |
| `4` | Market | every provider of the pair on the board, cheapest first; `d` switches direction |
| `5` | Advisor | where its fee stands among the others at the smallest and the largest swap it takes, what a swap costs it, and from what size its fee covers that |
| `6` | Maint | both directions' state, what is short, and the commands that fix it; `c` copies the picked one |
| `7` | Tx | the Bitcoin transactions its swaps name and, with an explorer set, the ones at its addresses: what each is to its swap, sats, confirmations, fee rate, age; `enter` copies the picked one's link |

Market, Advisor, Maint and Tx are on the same keys as in the fast bridge monitor.

A swap's **fee earned** is what the provider's upkeep logged when it counted the swap (`stats: a swap is
counted`): sats for BTC→ETH, ETH for ETH→BTC. It is a dash until the swap has ended and been counted, and for a
swap counted before the log this run has read begins (`journalctl -n 2000` by default). The totals on the Books
page do not depend on the log.

The **Books** page is empty until the provider's upkeep has made its first look, about twenty seconds after it
starts. A dry run keeps no books.

### Transactions

The **Tx** page lists the Bitcoin transactions of the 50 most recent swaps, open ones first, as the swap records
name them: a user's lock and the provider's take of it (BTC→ETH), the provider's lock and its ways back
(ETH→BTC). Without `btc_explorer` it shows what the records themselves say: for a transaction the provider sent,
its fee rate, when it was sent and how often it was bumped (`sent · status unknown`: whether it confirmed is the
chain's to say), and for a user's lock that the provider saw it in a block.

With an explorer it adds each one's status, fee rate and age, and the other transactions at the provider's main
and reverse addresses (a top-up, a move between the two). Sats with a sign are what the explorer saw move at the
provider's addresses, fees included; sats without one are the swap record's word for a transaction that pays or
spends a script of the swap and not an address of the provider's.

With `eth_rpc` and `pool_contract` both set, a BTC→ETH swap also gets a row for the provider's lock of ETH in
the pool, on Ethereum: found in the pool contract's log by the leaf the swap record names, with its depth. The
log is read a day back at most, and the title says how far; the ETH on the row is the swap record's, since the
pool hides amounts.

An unconfirmed transaction is **stuck** when it has waited longer than `tx_stuck_secs`, or ten minutes while
paying under four fifths of the rate that confirms within the hour. An explorer does not say when a transaction
was broadcast, so the wait is counted from when the provider's record says it sent it, or for a transaction
the record gives no time for, from when this run of tacmon first saw it. A user's stuck lock is the
user's to bump: it is marked on the page and not in the header.

## Advisor

A page takes the quote that pays the user most, so a provider is picked at a size when its whole fee there is the
cheapest. The share and the flat fee weigh differently on a small swap and a large one, so the Advisor compares at
two sizes: the smallest swap the provider takes, and the largest it can take now.

The table on the left is BTC→ETH. Each row is a fee from `fee_ladder_bps` with the provider's own flat fee, and the
provider's own fee is always among them: what that fee takes at each size, what is left of it after the cost of a
swap (`NET`, red under zero), and whether it would be the cheapest on the board. A fee the app would mark as high
(over 5% of the swap) is named as that.

The right pane says the same in words for both directions, then:

- **What a swap costs you.** The provider's books count the Bitcoin fees of its own transactions and the relay's
  fees. The Advisor divides them by the swaps counted, both directions together, and puts the two parts together at
  the provider's own price. Against that it says from what size the fee covers the cost: at every size, from a
  size inside the range, or only over the max, in which case every swap costs more than it brings. Where it does
  not cover it at the min, it says what flat fee would, and whether that would still be the cheapest there. It
  says what covering the cost would take, never what to set: being picked comes before earning more per swap.
  The cost is an average of the past, and the first swaps weigh heavily in it. With no swap counted, or no price,
  it is not known and the `NET` columns are dashes.
- **Size.** Where the other providers' sizes reach under or over the provider's own.
- **Funding.** What a swap of the max takes in ETH at the provider's price, how many of those its free ETH
  covers, and whether one lock can draw that much from its notes.

Ads are unsigned: anyone can post one. A rival named here is worth a look on the Market page.

## Maintenance

The provider looks after most of itself: it merges its notes, moves take coins to its reverse address, and replaces
a slow take or lock. The Maintenance page lists each direction's state, then what is left for a hand, most urgent
first: a provider that does not answer or has no price, a swap that ended with nothing received, a stuck
transaction, a side that can take less than its min, ETH that has arrived at the pool account and is not shielded,
gas ETH under the provider's reserve, an ad that is not on the board. A step
that a command helps with has it printed underneath. `↑` and `↓` scroll; `shift+↑` and `shift+↓` (or `K` and `J`)
pick a command, and `c` (or Enter) copies it through the terminal's clipboard (OSC 52).

tacmon runs none of them. The commands are:

```bash
node fastbridge-lp/setup.mjs status --data <data_dir> --port <port>     # Tacit's own check of health, terms and ad (reads only)
FB_LP_KEY=$(cat <data_dir>/provider.key) node tools/fastbridge-lp-shield.mjs <ETH> --eth   # shield ETH at the pool account: a dry run without --send
sudo systemctl stop <unit> && { node fastbridge-lp/setup.mjs --data <data_dir> --port <port> --no-start; sudo systemctl start <unit>; }
${EDITOR:-nano} <data_dir>/config.json && sudo systemctl restart <unit>  # change the fee or the sizes
DATA_DIR=<data_dir> make backup DEST=<another disk>                      # from the tacmon checkout
```

The first three run from a tacit checkout, as the user the provider runs as. Setup reads the key file itself; the
shield tool takes the key in its environment, which the operator's shell fills from the key file. tacmon reads
neither.

ETH sent to the pool account is not lent until it is shielded into the pool. A provider keeps a reserve there for
gas and, with `FB_LP_AUTO_SHIELD=on` in its environment, shields what arrives over it by itself; without that
switch nothing does. The page takes the reserve, the smallest amount worth shielding and whether the provider
shields by itself from the provider's own `/info` (`refill`), and Tacit's defaults of 0.02 ETH each where a
provider does not say. With `eth_rpc` set:

- under the reserve, it says to send ETH for gas, which stays at the account and is not to be shielded;
- over the reserve by the smallest amount or more, it prints the shield tool's dry run for what is over, or says
  there is nothing to run where the provider shields by itself.

Setup does not shield for a provider that is already funded: it shields only while the provider holds less than
two swaps of its min. After a shield by hand, raise `quote.maxAmount`. The account's own ETH is shown cut to four
places, never rounded up: a figure just under the reserve does not read as the reserve.

Running setup again is Tacit's way to turn ETH→BTC on once Bitcoin has confirmed at the reverse address. It writes
the limits in `config.json` afresh from what is funded, so a size set by hand is set again; the fee is kept. The
provider is started again whatever setup says, so a setup that fails or is interrupted does not leave it stopped,
and the step prints no command while a swap is under way.

Under each address the page says what it holds, confirmed and not yet, by the explorer's list of its coins
(`btc_explorer`; without one it is a dash). While the reverse side is off the provider's log does not name its
address: set `btc_eth.reverse_address` (setup prints it) and the step names it.

`<data_dir>` is `btc_eth.data_dir`, `<unit>` is what `-u` names in `btc_eth.log_cmd`, and `<port>` is the one in
`btc_eth.lp_url`. With a log command that names no unit, the `systemctl` parts are left out. Nothing read from the
provider, the board or a chain is put in a command.

## Settings

`eth_rpc`, `pool_contract`, `eth_explorer` and `btc_explorer` can be set once under `[shared]` for every monitor instead: this monitor's own setting wins where it is not empty ([Configuration](../README.md#configuration)).

| SETTING | DEFAULT | WHAT IT TURNS ON, AND WHAT IT SENDS |
|:---|:---|:---|
| `eth_rpc` | empty | the pool account's own ETH (`gas ETH` in the header), which pays its gas when the relay does not. Sends the pool account to that RPC, in `eth_getBalance` |
| `pool_contract` | empty | with `eth_rpc`, the pool lock rows on the Tx page. Sends the pool contract's address, one event's topic and a range of blocks to that RPC, in `eth_getLogs`: nothing of this provider's |
| `eth_explorer` | empty | the site (`https://etherscan.io`) a pool lock's copied link points at. Nothing is ever requested from it |
| `btc_explorer` | empty | the status of the swaps' transactions and the provider's addresses on the Tx page, and what each address holds on the Maintenance page. An Esplora API (`https://mempool.space/api`, `https://blockstream.info/api`). Sends the provider's main and reverse addresses and the ids of its recent swaps' transactions to that explorer |
| `fee_ladder_bps` | `[5, 10, 20, 30, 50, 100, 150, 200]` | the fees the Advisor compares, in hundredths of a percent |
| `tx_stuck_secs` | `1800` | how long a transaction may wait unconfirmed before it is marked as stuck |
| `reverse_address` | empty | the reverse side's Bitcoin address, for before that side is on: the Maintenance page then names it as the address to fund. Only a tool with the key can derive it (Tacit's setup prints it); once the side is on, the log names it and this is not needed. With `btc_explorer` set, it is sent to that explorer like the main address |
| `pool_account` | empty | the account to ask about, when it should not be learned from the log's start line |
| `deadline_warn_secs` | `3600` | a swap's deadline turns red when it is closer than this |
| `refresh.*` | 5 s, 20 s, 5 s, 60 s, 120 s | how often the API, the board, the records, the RPC and the explorer are read. The explorer is never asked more often than every 30 s |

## What it does not show yet

- **A pool lock that never lands.** A lock has a row once the pool's log shows it; a swap whose lock is overdue
  is not yet called out.
- **Demand.** The board's waiting swaps are not read.
