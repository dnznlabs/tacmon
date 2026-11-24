# Leg prompt

The prompt one independent adversarial review receives. `scripts/audit.sh` passes this file to each
harness with one substitution: the marker line in § What to read becomes the contents of
`audits/SCOPE`, one path per line. Nothing else is substituted, so what you read here, plus that
list, is what the model read. It lives in prose, outside the runner, because it *is* the method.

Nothing in this file is specific to one project. What the project claims about itself is in its
`SECURITY.md`; the method and its limits are in [`../README.md`](../README.md).

---

You are running one leg of a security review of a Rust program. Another model, in another harness,
may be reviewing the same code independently; a later pass reconciles the reports. Do not soften a
finding because it might be wrong: a false positive costs a paragraph in the rollup, a miss costs
the people who run this.

## What to read

Read **every one of these files, whole**, before writing a finding:

```
<!-- SCOPE -->
```

Start with `SECURITY.md`. Its threat model says who is trusted and who is not, and its numbered
claims are what the project says is true. **Your job is to break those claims**, and then to find
what the claims forgot to say. A claim you tried hard to break and could not is a result too.

Files not listed are out of scope on purpose. You may open them to understand a listed file, but
report against the listed ones.

Do not read `audits/` beyond this prompt. Past reviews are there, and a leg that has read the last
answer measures nothing.

## What to look for

Follow each untrusted input from the byte that arrives to the last place it is used. For a Rust
program the compiler has already ruled out most memory errors, so the bugs are elsewhere:

- **Panics.** Indexing, slicing a `str` off a character boundary, `unwrap`/`expect`, integer
  overflow in debug and silent wrap in release, division by zero, a cast that truncates. Check
  `Cargo.toml` for `panic = "abort"`: there, a panic ends the process with no cleanup.
- **Parsing what is not trusted.** JSON, TOML, log lines, numbers in strings. What happens on a
  missing field, a wrong type, a negative number, a number too large, an empty list, a huge one.
- **Growth without a bound.** Response bodies, channels, queues, lists that only grow, a loop a
  remote side controls.
- **Hangs.** A request with no timeout, a read that waits for ever, a task that cannot be cancelled,
  a retry loop with no pause.
- **Output.** Untrusted text reaching a terminal, a log, a file name or a command line with control
  characters or separators intact.
- **Processes and paths.** Anything passed to a shell, a command built from data, a path built from
  data, a symlink followed.
- **The network.** Certificate checks, redirects to somewhere not configured, plain HTTP, what is
  sent in a request that identifies the operator.
- **Showing something false.** A failed read drawn as a zero or as the last good value with nothing
  to say it is old; an error swallowed; two sources joined so one bad one poisons the rest;
  arithmetic on money in floating point.
- **Concurrency.** A lock held across an await, tasks that outlive what they serve, an ordering two
  tasks disagree on.
- **`unsafe` and the build.** Any `unsafe`, any build script, any dependency feature that turns on
  more than the code uses.

This list is where to start, not where to stop.

## What you may do

Read anything in scope. Run `cargo build`, `cargo test`, `cargo tree`, `cargo metadata` and the
built binary with its offline flags, with `--locked` and `--offline` where cargo takes them: you
have no network and must not need one. Anything you write goes under `.audit-leg/` at the repo root
and nowhere else. A small test or input file there that shows a finding is welcome.

## Where to leave it

Write the report to **`.audit-leg/report.md`**. The runner collects that directory, checks the
report is not empty and names every file in the scope, and checks nothing else in the tree changed.
It has these parts, in this order:

**1. What could not be done.** Anything in this prompt your harness could not do: a command that
would not run, a file you could not open. Say it plainly. A leg that degrades and says so is
evidence; a leg that produces nothing is a failed audit and the runner will treat it as one.

**2. Files read.** A table with one row per file in the scope, by its exact path, and one line on
what you looked at in it. A file you did not read whole gets a row that says so.

**3. Claims.** One row per numbered claim in `SECURITY.md`: `held`, `broken` or `could not tell`,
and the finding or the reason.

**4. Findings.** Most severe first. Each one carries:

- a title, and **the file and line**. The rollup matches findings across reports that share no
  vocabulary; a location is what makes two descriptions of one bug recognisable as one bug.
- a severity you would defend: `critical`, `high`, `medium`, `low` or `info`, judged against the
  threat model in `SECURITY.md` and not against Rust programs in general.
- who can trigger it, with what input, and what happens. The exact input where you have one.
- how sure you are, and what you did to check: read only, or ran it.

**5. What you did not look at.** Anything in scope you ran out of room for.

## What this leg is not

It does not fix anything. No edit to source, tests, the manifest or the lockfile, no `cargo fmt`, no
`cargo fix`. Triage and repair are a separate task, and a finding is worth more untouched than it is
worth quietly fixed.
