# Security

`tacmon` is pre-release software reviewed by the people who wrote it. No outside firm has audited it. What is
here is the reasoning, the checks and the review records, so the claims can be checked rather than taken on trust.

## Reporting a problem

Open a private security advisory on the repository (Security → Report a vulnerability). Please do not open a
public issue for something that could be used against an operator.

## What tacmon is trusted with

It watches a service that holds funds. It never holds them itself.

| It does | It never does |
|---|---|
| reads the provider's local API, swap records and log | read, ask for or store a private key |
| reads the public board over HTTPS | sign or send a transaction |
| optionally asks one Ethereum RPC for one balance | write to the provider's data directory, or to any file |
| runs the one log command the operator configured | call the provider's write endpoints |

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
| 1 | No private key is read, requested or stored. | review; there is no code path that takes one |
| 2 | Nothing is written to disk and no service is changed: every request is a read. | review |
| 3 | No `unsafe` in this crate. | `#![forbid(unsafe_code)]`, enforced by the compiler; `make audit-checks` |
| 4 | The only outbound connections are to hosts in the config, through a proxy where `HTTP_PROXY`, `HTTPS_PROXY` or `ALL_PROXY` names one. A redirect is never followed and its body is never read as an answer. `--demo` makes none. | tests; review |
| 5 | The only process started is `log_cmd`, with its arguments passed as a list and never through a shell. | review |
| 6 | TLS is rustls with the platform's certificate checks; there is no way to switch verification off. | `deny.toml` bans other TLS stacks; review |
| 7 | Untrusted text cannot move the cursor, change colours or otherwise control the terminal. | review |
| 8 | No input from an untrusted source can panic the process, hang it or grow memory without bound: an answer is refused over 4 MiB, a log line is cut at 64 KiB, a swap record is skipped over 1 MiB, and amounts that would overflow stop at the largest value. | tests; review; the `panic-surface` report lists every place a panic could start |
| 9 | A read that fails shows as failed. A stale or missing value is never drawn as a fresh one: a failed status or an answer without terms is a failed read, and a figure taken from the log says so and gives its age. | tests; review |
| 10 | Dependencies come from crates.io, carry no known advisory, and use permissive licences. | `cargo-deny` against `deny.toml` |

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
- **Text that only looks wrong.** Control characters never reach the terminal, but invisible and
  direction-changing Unicode characters in a name do.
- Bugs in dependencies that no advisory describes yet.
- Whether a number is *right*: the reviews look for ways to make tacmon show something false, not for errors
  in the provider it is reading.
