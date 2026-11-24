#!/usr/bin/env bash
#
# The checks that need no judgement: the same answer for the same commit, whoever runs them. Each one
# writes its whole output to <out>/<name>.txt and one line to <out>/SUMMARY.txt. audits/README.md
# § Checks says what each is for; deny.toml and audits/SCOPE hold what they are checked against.
#
#   make audit-checks                    print the summary, exit non-zero if a gate fails
#   bash scripts/audit-checks.sh <dir>   the same, keeping every output in <dir>
#
# A check is a GATE (pass or fail) or a REPORT (a count for the reviewers, never a failure).
#
set -uo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

OUT="${1:-}"
KEEP=1
if [ -z "$OUT" ]; then
  OUT="$(mktemp -d "${TMPDIR:-/tmp}/audit-checks-XXXXXX")"
  KEEP=0
fi
mkdir -p "$OUT"
: > "$OUT/SUMMARY.txt"
FAILED=0

# Not on PATH is tried through devbox before it is called missing: `make` is not always run from a
# shell direnv has set up, and a tool that is installed should not be reported as absent.
tool() {
  if command -v "$1" >/dev/null 2>&1; then
    "$@"
  elif command -v devbox >/dev/null 2>&1; then
    devbox run -- "$@"
  else
    echo "$1 not found, and no devbox to find it with"
    return 127
  fi
}

record() { printf '%-9s %-16s %s\n' "$1" "$2" "$3" | tee -a "$OUT/SUMMARY.txt"; }

# gate <name> <what a pass means> <command...>
gate() {
  local name="$1" what="$2"
  shift 2
  if "$@" > "$OUT/$name.txt" 2>&1; then
    record PASS "$name" "$what"
  else
    record FAIL "$name" "$what (exit $?, see $name.txt)"
    FAILED=1
  fi
}

# The lockfile is what was built and what every other check reads, so it is checked first: a
# Cargo.lock that no longer matches Cargo.toml means the checks below describe a tree nobody ships.
gate lockfile "Cargo.lock is committed and matches Cargo.toml" \
  bash -c 'git ls-files --error-unmatch Cargo.lock && cargo metadata --locked --format-version 1 > /dev/null'

gate advisories "no RustSec advisory, yanked or unmaintained crate in the tree" tool cargo-deny check advisories
gate licences   "every dependency's licence is on deny.toml's list"            tool cargo-deny check licenses
gate bans       "no banned crate, no wildcard version"                         tool cargo-deny check bans
gate sources    "every dependency comes from crates.io"                        tool cargo-deny check sources

# Every crate root must forbid unsafe, so the claim is the compiler's and not a grep's. The grep
# only confirms the attribute is there; rustc is what rejects an `unsafe` block behind it.
unsafe_forbidden() {
  local root missing=0
  while IFS= read -r root; do
    if grep -qE '^#!\[forbid\(unsafe_code\)\]' "$root"; then
      echo "forbid(unsafe_code)  $root"
    else
      echo "MISSING              $root"
      missing=1
    fi
  done < <(cargo metadata --locked --no-deps --format-version 1 \
    | tr ',' '\n' | sed -n 's/.*"src_path":"\([^"]*\)".*/\1/p' | sort -u | sed "s#^$REPO_ROOT/##")
  return "$missing"
}
gate unsafe "every crate root carries #![forbid(unsafe_code)]" unsafe_forbidden

gate clippy "clippy is clean with warnings denied" cargo clippy --locked --all-targets -- -D warnings
gate tests  "the tests pass"                       cargo test --locked

# Every Rust file is either in audits/SCOPE or excluded there with a reason. Without this a file
# added after the scope was written is simply never reviewed, and nothing says so.
scope_covers() {
  local f rc=0
  [ -f audits/SCOPE ] || { echo "audits/SCOPE is missing"; return 1; }
  while IFS= read -r f; do
    if grep -qxF "$f" audits/SCOPE; then
      echo "in scope   $f"
    elif grep -qE "^![[:space:]]*$f([[:space:]]|$)" audits/SCOPE; then
      echo "excluded   $f"
    else
      echo "UNLISTED   $f"
      rc=1
    fi
  # not audits/: a leg may leave a small Rust program there as evidence, and it is a record, not the code under review
  done < <(git ls-files '*.rs' ':!audits/')
  return "$rc"
}
gate scope "every tracked .rs file is in audits/SCOPE or excluded there with a reason" scope_covers

# REPORT, not a gate. With panic = "abort" a panic is the process ending, so every place one can
# start is worth a reviewer's look. Most are provably fine, which is why this counts and does not fail.
cargo clippy --locked --bins -- \
  -W clippy::unwrap_used -W clippy::expect_used -W clippy::panic -W clippy::indexing_slicing \
  -W clippy::string_slice -W clippy::arithmetic_side_effects -W clippy::cast_possible_truncation \
  -W clippy::cast_sign_loss > "$OUT/panic-surface.txt" 2>&1
record REPORT panic-surface "$(grep -cE '^warning: ' "$OUT/panic-surface.txt" | tr -d ' ') lint hits where a panic or a silent wrap could start (panic-surface.txt)"

echo
if [ "$KEEP" -eq 1 ]; then
  echo "outputs: $OUT"
elif [ "$FAILED" -ne 0 ]; then
  echo "a gate failed; outputs kept in $OUT"
else
  rm -rf "$OUT"
fi
exit "$FAILED"
