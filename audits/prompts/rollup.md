# Rollup prompt

The prompt that reconciles a run into `audits/<date>/AUDIT.md`. `scripts/audit.sh` passes this file
verbatim and then appends a short `## This run` block naming the paths the run produced: one
directory per leg, the checks directory, the provenance file and the output file. When the project
has written one, `audits/KNOWN_ANSWER.md` follows under `## Known answer`. Those are the only
run-specific values; everything above them is the method, and the method is the same every time.

The rollup is a synthesis, so a model writes it and not the shell. The shell's job is to run the
checks and the legs, put the bytes in the right directory, and record what happened.

The layout, the scope and the provenance spec are in [`../README.md`](../README.md).

---

A security review run has finished: a set of checks with one right answer, and one or more
adversarial reviews of the same files. Reconcile them into one document. You are not re-auditing the
code; you are reading the outputs and producing the artifact a human will act on.

## Before anything else: check the inputs are real

Read every leg directory and the checks directory named in `## This run`.

- **An empty leg directory is a failed audit, not a clean one.** If any leg directory is empty, or
  its `report.md` is missing or empty, **stop**. Write `AUDIT.md` containing the provenance block
  and one line saying which leg produced nothing and that this audit is void. Do not summarise a
  surviving leg as if it were the audit.
- If a leg says it could not do part of what it was asked, it still counts. Carry its own statement
  into § Legs, closely enough to be checked.

Say in the document how many findings you read from each leg, before deduplication. A count is what
proves you read a non-empty input.

## The document

`AUDIT.md`, in this order.

**1. What this is.** Two or three sentences, the first of which says this is a self-review by the
project's own developers using language models, not an audit by an outside firm. If the provenance
block says the run had one leg, say here that nothing in it was independently confirmed.

**2. Provenance.** The block from the provenance file named in `## This run`, reproduced
**verbatim** in a code block. It was captured at run time by the runner. Do not retype it, do not
reformat it, and do not fill a field from your own knowledge. If a field is absent, say `not
reported`; never guess one.

**3. Checks.** The lines of the checks directory's `SUMMARY.txt`, as a table. A `FAIL` is a finding
and also appears in § Findings with the detail from that check's output file. A `REPORT` is a count,
not a result: give the number and do not call it a pass.

**4. Findings.** One entry per distinct issue, most severe first. A finding is one issue even when
two legs describe it in different words; a file and a line is what makes that recognisable. Each
entry carries:

- a title, the file and line, and the severity
- **which legs found it**: all, or which alone
- **the severity each leg gave it**, where they differ, and which one this document adopts, with the
  reason. Do not average two severities. Do not quietly take the higher one either; say why.
- what it is and what it costs, in the shortest form that is still checkable against the code

**5. Claims.** One row per numbered claim in `SECURITY.md`: what each leg concluded, and whether the
claim stands. A claim one leg broke and another held is reported as a disagreement, not settled
here.

**6. Agreement and disagreement.** With more than one leg: what every leg found, what only one
found, and whether the single-leg findings look like a miss by the others or like a false positive.
With one leg: say that this section has nothing to measure.

**7. Grading against the known answer.** Only when `## This run` is followed by `## Known answer`: a
finding the project knew before the run, found by hand. State per leg whether it found it, judged on
location and mechanism and not on wording. **A miss is a publishable result, not something to
hide.** Without a known answer, leave this section out and say in § Legs that the run was not
graded.

**8. Legs.** One short paragraph per leg: what it ran, how many findings, anything it said it could
not do. Link the leg directory.

## What this is not

This document does not fix anything and does not propose a patch. Every finding is carried on
untouched. Do not edit any file other than `AUDIT.md`.

A past audit is never edited. A finding that turns out to be wrong is corrected in the *next*
audit's `AUDIT.md`, marked as superseding.
