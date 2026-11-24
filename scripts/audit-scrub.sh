#!/usr/bin/env bash
#
# Makes an audit's output fit to publish: what was read, run and found stays, the description of the
# machine it ran on goes. audits/README.md § What is published says what is removed and why.
#
#   bash scripts/audit-scrub.sh <file-or-directory> [<scratch-dir>]     rewrites in place
#
# Exits non-zero, leaving that file as it was, if a path it was meant to remove is still there.
#
set -euo pipefail

TARGET="$1"
SCRATCH="${2:-}"
REPO_ROOT="$(git rev-parse --show-toplevel)"

# Longest first: the repo may sit under the home directory, and the reviewer may have seen it by its
# real path while git names it through a link.
PATHS="$(
  {
    printf '%s\t.\n' "$(realpath "$REPO_ROOT")" "$REPO_ROOT"
    [ -n "$SCRATCH" ] && printf '%s\t<scratch>\n' "$(realpath "$SCRATCH")" "$SCRATCH"
    printf '%s\t~\n' "$(realpath "$HOME")" "$HOME"
  } | awk -F'\t' '!seen[$1]++ { print length($1) "\t" $0 }' | sort -rn | cut -f2-
)"

OUT="$(mktemp "${TMPDIR:-/tmp}/audit-scrub-XXXXXX")"
trap 'rm -f "$OUT"' EXIT

scrub() {
  local file="$1" from
  # A transcript's start-up record describes the machine and its installed tools, not the review, so
  # the whole line goes. Every other line is kept with the paths above replaced as plain text.
  PATHS="$PATHS" awk '
    BEGIN { n = split(ENVIRON["PATHS"], row, "\n"); for (i = 1; i <= n; i++) { split(row[i], p, "\t"); from[i] = p[1]; to[i] = p[2] } }
    FILENAME ~ /\.jsonl$/ && /"type":"system"/ && /"subtype":"init"/ { next }
    {
      for (i = 1; i <= n; i++) {
        out = ""
        while ((at = index($0, from[i])) > 0) { out = out substr($0, 1, at - 1) to[i]; $0 = substr($0, at + length(from[i])) }
        $0 = out $0
      }
      print
    }
  ' "$file" > "$OUT"
  while IFS=$'\t' read -r from _; do
    if grep -qF "$from" "$OUT"; then
      echo "audit-scrub: $from is still in $file after scrubbing" >&2
      return 1
    fi
  done <<< "$PATHS"
  # only a file that changed is rewritten, so a second pass over a clean directory touches nothing
  cmp -s "$OUT" "$file" || cat "$OUT" > "$file"
}

if [ -d "$TARGET" ]; then
  # text files only: grep -I skips anything binary a leg may have left as evidence
  while IFS= read -r f; do
    scrub "$f"
  done < <(grep -rIl '' "$TARGET")
else
  scrub "$TARGET"
fi
