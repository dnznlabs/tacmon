# Security

`tacmon` is pre-release software reviewed by the people who wrote it. No outside firm has audited it. What is
here is the reasoning, the checks and the review records, so the claims can be checked rather than taken on trust.

## Reporting a problem

Open an issue on the repository. The tacmon binary holds no keys and changes nothing, so a problem in it is best fixed
in the open. A problem in `scripts/helpers/fast-bridge`, which does handle the provider's key, may be worth reporting
privately first. A pull request with a test that fails without the fix is better still.

## What tacmon is trusted with

It watches a service that holds funds. It never holds them itself.

| It does | It never does |
|---|---|
| reads the provider's local API, swap records of both directions and log | read, ask for or store a private key |
| reads the public board over HTTPS | sign or send a transaction |
| optionally asks one Ethereum RPC for one balance | write to the provider's data directory, or to any file |
| optionally asks one Bitcoin explorer what is unspent and what has happened lately at the provider's two addresses, how its recent swaps' transactions stand, and the going fee rate | call the provider's write endpoints |
| runs the log command, and the status command where one is set, that the operator configured | run the commands its Maintenance page prints |

`scripts/helpers/fast-bridge` is the exception to the left-hand column, kept outside the binary. It is a shell script the
operator runs by hand. It reads `FB_LP_KEY` from the provider's environment file and gives it to tacit's own upkeep tools
(`tools/lp-run.sh`) in their environment, never on a command line or to the terminal. Every one of those tools that
sends is a dry run until it is run again with the token that dry run printed.

`status_cmd`, empty by default, is the one way tacmon gets what only the key can read: the TAC at the reverse address.
tacmon starts the helper's read-only `status` as a child process and reads what it prints. The key is read and used in
that child, never in tacmon's own process: tacmon does not open the environment file, and does not pass the key on.

## Threat model

**Trusted:** the operator, the machine, the config file and the command line. The config names a command to run
(`log_cmd`), so anyone who can edit it can already run code as the operator; it is as trusted as a shell profile.

**Not trusted:** everything tacmon reads at run time.

| Source | Who controls the bytes |
|---|---|
| the board (`/ads`, `/wants`) | anyone on the internet: ads and wants are posted without permission |
| the provider's log and swap records | the provider, but they carry values a counterparty supplied |
| the provider's local API | whatever answers on that port |
| the Ethereum RPC | the RPC's operator |
| the Bitcoin explorer | the explorer's operator |
| what `status_cmd` prints | tacit's status tool, which reads the pool and the explorers |

**What an attacker could want**, most serious first:

1. **To make the operator act on false numbers.** A monitor that shows a healthy provider when it is not, or a
   wrong fee, balance or deadline, costs real money without touching a key.
2. **To run code or write to the terminal.** Text from the board or a log reaching the terminal with its control
   characters intact could rewrite the screen or worse.
3. **To take the monitor down.** A crafted answer that panics it, hangs it or exhausts memory blinds the
   operator at the moment a swap needs attention.
4. **To learn something.** What tacmon sends out says which provider is being watched and from where.

## The claims

These are what the review is told to break. Each names what holds it up.

| # | Claim | Held up by |
|---|---|---|
| 1 | The binary reads, requests and stores no private key. `scripts/helpers/fast-bridge` reads the provider's, and passes it only to the environment of the tacit tool it runs. | review; there is no code path in the crate that takes one |
| 2 | Nothing is written to disk and no service is changed: every request is a read. | review |
| 3 | No `unsafe` in this crate. | `#![forbid(unsafe_code)]`, enforced by the compiler; `make audit-checks` |
| 4 | The only outbound connections are to hosts in the config, through a proxy where `HTTP_PROXY`, `HTTPS_PROXY` or `ALL_PROXY` names one. A redirect is never followed and its body is never read as an answer. The explorer is sent the provider's main and reverse addresses and the ids of the Bitcoin transactions its 50 most recent swaps name, and nothing else; only an address that reads as one, or 64 hex digits, goes into its URL. `--demo` makes none. | tests; review |
| 5 | The only processes started are `log_cmd` and, where set, `status_cmd`, each with its arguments passed as a list and never through a shell, with no stdin, and a status run's output held to 64 KiB and three minutes. | tests; review |
| 6 | TLS is rustls with the platform's certificate checks; there is no way to switch verification off. | `deny.toml` bans other TLS stacks; review |
| 7 | Untrusted text cannot move the cursor, change colours or otherwise control the terminal. The one escape tacmon writes itself is OSC 52, to copy a Maintenance command or a transaction's explorer link: its text is built only from the values claim 11 holds and, for a link, the explorer address in the config, stripped of control characters other than newlines, capped at 8 KiB, and sent base64-encoded. | a test that draws control characters; review |
| 8 | No input from an untrusted source can panic the process, hang it or grow memory without bound: an answer is refused over 256 KiB (an address's transaction list over 1 MiB), a log line is cut at 64 KiB, a swap record is skipped over 1 MiB and a link is never read as one, readers wait on bounded queues, and amounts that would overflow stop at the largest value. | tests; review; the `panic-surface` report lists every place a panic could start |
| 9 | A read that fails shows as failed. A stale or missing value is never drawn as a fresh one: a failed status or an answer without terms is a failed read, a figure nothing has reported is a dash and never a zero, and a figure taken from the log says so and gives its age. | tests; review |
| 10 | Dependencies come from crates.io, carry no known advisory, and use permissive licences. | `cargo-deny` against `deny.toml` |
| 11 | A command the Maintenance page prints carries nothing a shell would read as more than a word: transaction ids are 64 hex digits, addresses lower-case bech32, amounts and output numbers decimal. Anything else from a source is dropped, not quoted. | tests; review |

A claim that turns out false is a finding. The latest review says which ones held.

## How it is checked

- **`make audit-checks`** runs the checks with one right answer: RustSec advisories, licences, banned crates and
  sources through `cargo-deny`, the lockfile, `forbid(unsafe_code)`, clippy, the tests, and that every source
  file is in the review scope.
- **`make audit`** runs those, then independent adversarial reviews of every file in `audits/SCOPE` by language
  models from different labs, and reconciles them into `audits/<date>/AUDIT.md` with a record of what ran.
- Every commit is scanned for secrets by a pre-commit hook.

The method, its limits and every past review are in [`audits/`](audits/README.md).

## What this does not cover

- The services tacmon watches, and the machine it runs on.
- An attacker who can already edit the config file or the binary.
- **Another user on the same machine.** Whatever answers on the provider's port is believed, so a local user who
  takes that port while the provider is down can show a healthy one. Run the provider and tacmon on a machine
  you do not share.
- **An ad posted under your key.** The board checks no signature. tacmon says when more than one ad uses the
  provider's key and does not take its name from one that carries other terms, but it cannot remove it.
- **A false ad or a false want.** tacmon verifies no signature on either, and anyone can make a key. The Advisor compares only against
  ads that could take the same bridges and names the one it compares with, but a careful false ad still moves
  its advice, and false wants still move the demand it reports. Treat both pages as what the board says.
- **Text that only looks wrong.** Control characters never reach the terminal, but invisible and
  direction-changing Unicode characters in a name do.
- **A false coin from the explorer.** The Maintenance page believes what the explorer lists. The commands it prints
  are dry runs, and the tools they run check every coin named against the explorers themselves before spending it,
  so a false coin ends in an error, not a send. A false list can still hide a coin or suggest a move that is not
  needed.
- **An address's transaction list pushed over its limit.** The list carries every input's witness, so anyone can pay
  dust to an address from transactions large enough to put its latest 75 over 1 MiB. The read then fails, and shows as
  failed, until newer transactions push them out; swap transactions are still asked about by id meanwhile.
- **A false status from the explorer.** The Transactions page believes what the explorer says of a transaction: it
  can call a confirmed one stuck, or a stuck one confirmed. It moves nothing on that word; a bump is the operator's,
  through a tool that reads the chain itself.
- **Which output of a take holds its TAC.** That takes the key to tell. The page names every note-sized output of a
  take and leaves the choice to `fund`, and counts a take only while all its outputs are unspent.
- Bugs in dependencies that no advisory describes yet.
- Whether a number is *right*: the reviews look for ways to make tacmon show something false, not for errors
  in the provider it is reading.
