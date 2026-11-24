# Audits

How this project reviews its own security. The method lives here, in prose, so a reader can see what was run and
why without opening a script.

What is specific to this project lives in [`../SECURITY.md`](../SECURITY.md) (the threat model and the claims),
`SCOPE`, `../deny.toml` and, when there is one, `KNOWN_ANSWER.md`. Nothing in this file or in `prompts/` is.

## What an audit here is, and is not

**It is a self-review.** The people who wrote the code run it, with tools and language models. No outside firm is
involved, and nothing here should be quoted as if one were.

It has two parts, and they are different kinds of evidence:

- **Checks** have one right answer. The same commit gives the same result whoever runs it. A check that passes
  is a fact.
- **Reviews** are adversarial reads by language models, each in its own harness, told to break the claims in
  `SECURITY.md`. Independence is the point: two labs, two sets of blind spots. A review that finds nothing is
  not a fact; it is one reader who found nothing.

It is **not** a guarantee, and it does not replace tests. Where a property can be asserted, a test beats a
finding: a finding gets fixed once, a test refuses to let it come back.

### What "reproducible" means here

**The inputs are recorded; the review output is not deterministic.** Two runs of the same prompt over the same
code return different prose and sometimes different findings. What can be reproduced is the setup: the commit,
the scope, the prompts, the tools and the models are all recorded, so a later disagreement is about findings
and not about what was run.

## Layout

```
SECURITY.md                          the threat model and the numbered claims the reviews attack
deny.toml                            what the dependency tree is held to
audits/
  README.md                          this file
  SCOPE                              the files reviewed, one per line, and the exclusions with reasons
  KNOWN_ANSWER.md                    optional: a finding known before the run, to grade the legs
  prompts/
    leg.md                           the prompt each independent review receives
    rollup.md                        the prompt that reconciles a run
  <YYYY-MM-DD>/
    AUDIT.md                         the rollup: checks, findings, claims, provenance
    checks/                          SUMMARY.txt and every check's whole output
    rust-review/
      <harness>-<model>/             one directory per leg: report.md and anything it wrote to show a finding
    transcripts/                     everything each leg printed, tool calls included
```

The transcripts are the only evidence of what a leg read, so they are committed with the findings.

**Audits are append-only: a past audit is never edited**, because the next one cites it. A finding that turns out
to be wrong is corrected in the next `AUDIT.md`, marked as superseding. `make audit` enforces this: it refuses to
start if `audits/<date>/` exists. `AUDIT_DATE` is the escape hatch, not a routine setting.

## Checks

`make audit-checks` runs them alone, in under a minute once built. Run it before a release and whenever
`Cargo.lock` changes.

| Check | A pass means |
|---|---|
| `lockfile` | `Cargo.lock` is committed and matches `Cargo.toml`, so every other check describes what ships |
| `advisories` | no crate in the tree has a RustSec advisory, is yanked, or is unmaintained |
| `licences` | every dependency's licence is on the list in `deny.toml` |
| `bans` | no banned crate (a second TLS stack) and no wildcard version |
| `sources` | every dependency comes from crates.io: no git dependencies, no other registry |
| `unsafe` | every crate root carries `#![forbid(unsafe_code)]`, so the compiler rejects `unsafe` in this crate |
| `clippy` | clippy is clean with warnings denied |
| `tests` | the tests pass |
| `scope` | every tracked `.rs` file is in `SCOPE` or excluded there with a reason |

`panic-surface` is a **report**, not a check: a count of the places a panic or a silent wrap could start
(indexing, slicing, `expect`, arithmetic, narrowing casts). Most are provably fine, so it never fails. It is
there because the release build aborts on panic, and it gives a reviewer the list to walk.

An exception to any check is written in `deny.toml` or `SCOPE` with its reason, in the change that needs it.

What the checks do not cover: `unsafe` inside dependencies, a vulnerability no advisory describes yet, and a
dependency whose published crate differs from its repository.

## Scope

**`audits/SCOPE`: one repo-relative path per line.** The runner names exactly those files to each leg and records
the same list in the provenance block, so what a leg was told to read and what the record says cannot drift.

A file left out is a line starting with `!`, then the path, then the reason. "Generated", "test fixture" and
"not shipped" are reasons; "not important" is not. The `scope` check fails on a `.rs` file that is neither
listed nor excluded, so a new file cannot go unreviewed by being forgotten.

## Legs

```
make audit                 # codex, then claude, then the rollup
make audit LEGS=claude     # one leg
```

Each leg gets `prompts/leg.md` with the scope substituted in, runs in the repo with no network of its own, and
writes `.audit-leg/report.md`. The runner collects that directory, counts how many scope files the report names,
and compares `git status` before and after: a leg is told to change nothing, and the comparison is what turns
that instruction into a fact.

**No leg sees another.** Each leg's output, and the checks' output, wait outside the tree until every leg has
run.

**Two legs from two labs is the method.** A run with one leg is allowed only when asked for by name. Its
provenance block says `ONE LEG`, and its `AUDIT.md` says in the first section that nothing was independently
confirmed. That is a spot check. It is worth having and it is not the same thing.

The rollup is written by `claude` under `CLAUDE_MODEL`, from `prompts/rollup.md`. It is a synthesis, so a model
writes it; the shell only runs things and records what happened.

`make audit` is complete when `AUDIT.md` exists and every leg's `report.md` is non-empty. **An empty leg is a
failed audit, not a clean one.**

Prerequisites: `claude`; `codex`, logged in, for the second leg; `cargo-deny` (in `devbox.json`); `node`,
optional, to read the Claude leg's model and tokens.

## Provenance

Every `AUDIT.md` carries a block recording what ran. Observed values, captured by the runner at run time:

```
audit date        2026-11-24
code commit       <git rev-parse HEAD>  (scope dirty: yes/no · tree dirty: yes/no)
toolchain         <rustc --version>
scope             <the file list actually passed, one per line>
checks            <n> passed · <n> failed
independence      <n legs, run without sight of each other | ONE LEG — …>
leg 1             <harness and version> · requested <model> → observed <model> · <wall> · <tokens> · <coverage> · <tree unchanged?>
```

`scope dirty` says whether someone else can check out the commit and get the bytes that were reviewed. `tree
dirty` says whether anything else was uncommitted, the method's own files included.

## After a run

1. Read `AUDIT.md`. Triage each finding: fix it, or write down why not.
2. A fix comes with a test that fails without it.
3. A claim in `SECURITY.md` that was broken is fixed in the code or withdrawn from the document. It does not
   stay as written.
4. Commit `audits/<date>/` whole, in its own commit, before any fix. The record is of the code as reviewed.

## Grading a run

Write `KNOWN_ANSWER.md`: a real finding found by hand before the run, with file, line and mechanism. The runner
gives it to the rollup only, which records per leg whether it was found. That is the one piece of evidence about
this method that is not an assertion. A leg can open a file in the tree whatever the prompt says, so for a
graded run keep the answer outside and pass `KNOWN_ANSWER_FILE=<path>`.

## Known limits

- **The legs are language models trained on overlapping public code.** Correlated blind spots are likely and
  unmeasured. More models is not the fix; other methods are: property tests, fuzzing, a human reading the diff.
- **A leg can read what the prompt tells it not to.** Past audits are in the tree the legs run in. The prompt
  says to leave them alone and the transcripts show whether it did; nothing prevents it. A blind run needs a
  separate tree holding only the scope, the prompts and `SCOPE`.
- **Nothing here runs the program against hostile input.** There is no fuzzing and no dynamic analysis. For the
  parsers of untrusted data that is the largest gap.
- **A clean run means the legs found nothing**, on that day, with those models.
