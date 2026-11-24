#!/usr/bin/env bash
#
# A security review run: the checks that have one right answer, then independent adversarial reviews
# of the files in audits/SCOPE, captured separately and reconciled. The method is audits/README.md;
# the prompts are audits/prompts/. This file runs them and records what happened. It decides nothing.
#
# Ordered: preflight, checks, one leg per harness, provenance, rollup, completion.
#
#   make audit
#   make audit LEGS=claude                      one leg, recorded as one leg
#   make audit CLAUDE_MODEL=opus CODEX_MODEL=gpt-5.6-sol
#
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

LEGS="${LEGS:-codex claude}"
CLAUDE_MODEL="${CLAUDE_MODEL:-opus}"
CODEX_MODEL="${CODEX_MODEL:-gpt-5.6-sol}"
AUDIT_DATE="${AUDIT_DATE:-$(date +%Y-%m-%d)}"
LEG_TIMEOUT="${LEG_TIMEOUT:-5400}"   # seconds, per leg and for the rollup

OUT_DIR="$REPO_ROOT/audits/$AUDIT_DATE"
LEGS_DIR="$OUT_DIR/rust-review"
LEG_OUT="$REPO_ROOT/.audit-leg"      # where a leg writes; audits/prompts/leg.md § Where to leave it
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/audit-XXXXXX")"
# Each leg's harvest and the checks wait here until every leg has run: anything in the tree is a
# file the next leg can read, and a leg that has read another's report is no longer independent.
STAGE="$SCRATCH/legs"

# Kept on any non-zero exit, removed only on success: the scratch holds the transcripts, and a run
# that failed is exactly the one that has to be diagnosed from them.
cleanup() {
  local rc=$?
  if [ "$rc" -eq 0 ]; then
    rm -rf "$SCRATCH"
  else
    printf '\n\033[1maudit: exit %s — scratch kept for diagnosis: %s\033[0m\n' "$rc" "$SCRATCH" >&2
  fi
}
trap cleanup EXIT

say()  { printf '\n\033[1m== %s\033[0m\n' "$*"; }
info() { printf '   %s\n' "$*"; }
die()  { printf '\n\033[1;31maudit: %s\033[0m\n' "$*" >&2; exit 1; }

# ---- preflight ----------------------------------------------------------------------------------
# Fails loudly and early. A run that starts and silently produces one leg is worse than one that
# refuses to start.

say "preflight"

# The scratch holds the staged legs, the transcripts and the known answer. Inside the tree the legs
# run in, it would hand a leg every one of those.
case "$(realpath "$SCRATCH")/" in
  "$(realpath "$REPO_ROOT")"/*) die "scratch $SCRATCH is inside the repo — point TMPDIR outside it" ;;
esac

# One list is what the leg prompt names and what the provenance block records, so the two cannot
# drift. `!` lines are exclusions with their reasons: read by audit-checks.sh, never sent to a leg.
[ -f "$REPO_ROOT/audits/SCOPE" ] || die "audits/SCOPE is missing — name the files to review, one per line"
mapfile -t SCOPE < <(grep -vE '^[[:space:]]*(#|!|$)' "$REPO_ROOT/audits/SCOPE" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
[ "${#SCOPE[@]}" -gt 0 ] || die "audits/SCOPE names no files"
for f in "${SCOPE[@]}"; do
  [ -f "$REPO_ROOT/$f" ] || die "scope file missing: $f"
done
info "scope   ${#SCOPE[@]} files, $(cat "${SCOPE[@]}" | wc -l) lines"

# The legs to run. Two harnesses from two labs is the method; one is allowed only when it is asked
# for by name, and the provenance block then says the run had one leg.
read -r -a LEG_LIST <<< "$LEGS"
[ "${#LEG_LIST[@]}" -gt 0 ] || die "LEGS is empty"
for leg in "${LEG_LIST[@]}"; do
  case "$leg" in
    codex|claude) ;;
    *) die "unknown leg '$leg' — LEGS takes: codex claude" ;;
  esac
  command -v "$leg" >/dev/null 2>&1 || die "$leg not on PATH — install it, or name the legs to run: make audit LEGS=claude
       a run with one leg is recorded as one leg (audits/README.md § Legs)"
done
# The rollup is written by claude whichever legs ran.
command -v claude >/dev/null 2>&1 || die "claude not on PATH — the rollup needs it"

CLAUDE_VERSION="$(claude --version 2>&1 | head -1)"
info "claude  $CLAUDE_VERSION"
CODEX_VERSION="not run"
if [[ " $LEGS " == *" codex "* ]]; then
  CODEX_VERSION="$(codex --version 2>&1 | head -1)"
  info "codex   $CODEX_VERSION"
  # Exit code, not output: `codex login status` exits non-zero when logged out.
  if ! CODEX_LOGIN="$(codex login status 2>&1)"; then
    die "codex is not logged in — run 'codex login'. It said: ${CODEX_LOGIN//$'\n'/ }"
  fi
fi
info "legs    ${LEG_LIST[*]}"

[ -f "$REPO_ROOT/audits/prompts/leg.md" ]    || die "audits/prompts/leg.md is missing"
[ -f "$REPO_ROOT/audits/prompts/rollup.md" ] || die "audits/prompts/rollup.md is missing"
grep -qxF '<!-- SCOPE -->' "$REPO_ROOT/audits/prompts/leg.md" || die "audits/prompts/leg.md has no <!-- SCOPE --> line"

# A report left by a run that died would be harvested as this run's.
[ -e "$LEG_OUT" ] && die ".audit-leg/ exists from an earlier run — look at it, then remove it"

# Audits are append-only (audits/README.md § Layout): a second run on one date would merge into the
# first. AUDIT_DATE is the escape hatch.
[ -e "$OUT_DIR" ] && die "audits/$AUDIT_DATE already exists — audits are append-only
       to re-run today: move it aside, or pass AUDIT_DATE=<something-else>"
info "output  audits/$AUDIT_DATE (new)"

# Captured before anything writes a byte. Two flags, because they answer two questions: can someone
# else check out this commit and get the bytes that were reviewed (scope), and was anything else in
# the tree uncommitted, the method's own files included (tree).
CODE_COMMIT="$(git rev-parse HEAD)"
if [ -n "$(git status --porcelain -- "${SCOPE[@]}")" ]; then SCOPE_DIRTY=yes; else SCOPE_DIRTY=no; fi
if [ -n "$(git status --porcelain)" ]; then TREE_DIRTY=yes; else TREE_DIRTY=no; fi
info "commit  ${CODE_COMMIT:0:12} (scope dirty: $SCOPE_DIRTY · tree dirty: $TREE_DIRTY)"
RUSTC_VERSION="$(rustc --version 2>&1 | head -1)"

# ---- checks -------------------------------------------------------------------------------------
# A failing gate does not stop the run: it is a finding, and the rollup carries it. The outputs wait
# in the scratch with the legs.

say "checks"
set +e
bash "$REPO_ROOT/scripts/audit-checks.sh" "$SCRATCH/checks"
CHECKS_RC=$?
set -e
CHECKS_PASS="$(grep -c '^PASS' "$SCRATCH/checks/SUMMARY.txt" || true)"
CHECKS_FAIL="$(grep -c '^FAIL' "$SCRATCH/checks/SUMMARY.txt" || true)"
[ "$((CHECKS_PASS + CHECKS_FAIL))" -gt 0 ] || die "the checks produced no result (exit $CHECKS_RC)"

# ---- legs ---------------------------------------------------------------------------------------

# The one substitution any prompt gets: the line that is exactly the SCOPE marker becomes the scope.
LEG_PROMPT="$(SCOPE_TEXT="$(printf '%s\n' "${SCOPE[@]}")" \
  awk '$0 == "<!-- SCOPE -->" { print ENVIRON["SCOPE_TEXT"]; next } { print }' "$REPO_ROOT/audits/prompts/leg.md")"

# The legs are told to change nothing outside .audit-leg/, which is ignored. Comparing this before
# and after is what turns that instruction into a fact.
tree_state() { git status --porcelain; }

# harvest <leg-name> <stage-dir>
# A failed leg is a failed audit: this exits non-zero rather than leave an empty directory for the
# rollup to read as "no findings".
harvest() {
  local name="$1" dest="$2" f named=0
  [ -s "$LEG_OUT/report.md" ] || die "$name: .audit-leg/report.md is missing or empty — a failed leg is a failed audit"
  mkdir -p "$(dirname "$dest")"
  mv -T "$LEG_OUT" "$dest"
  for f in "${SCOPE[@]}"; do
    if grep -qF "$f" "$dest/report.md"; then named=$((named + 1)); fi
  done
  COVERAGE="$named of ${#SCOPE[@]} scope files named in the report"
  info "$name: $(wc -l < "$dest/report.md") lines of report, $COVERAGE"
}

LEG_NAMES=()   # directory name under rust-review/, in run order
LEG_LINES=()   # one provenance line per leg
LEG_LOGS=()    # transcript in the scratch
N=0

for leg in "${LEG_LIST[@]}"; do
  N=$((N + 1))
  BEFORE="$(tree_state)"
  START=$(date +%s)

  if [ "$leg" = codex ]; then
    say "leg $N — codex $CODEX_MODEL, effort high"
    NAME="codex-$CODEX_MODEL"
    LOG="$SCRATCH/leg-$N-codex.log"
    set +e
    timeout "$LEG_TIMEOUT" codex exec \
      -m "$CODEX_MODEL" \
      -c model_reasoning_effort="high" \
      -s workspace-write \
      -C "$REPO_ROOT" \
      "$LEG_PROMPT" < /dev/null 2>&1 | tee "$LOG"
    RC=${PIPESTATUS[0]}
    set -e
  else
    say "leg $N — claude $CLAUDE_MODEL"
    NAME="claude-$CLAUDE_MODEL"
    # stream-json, not json: json is the final result alone, and a leg whose tool calls are not on
    # disk cannot evidence what it read. stream-json needs --verbose under -p.
    LOG="$SCRATCH/leg-$N-claude.jsonl"
    set +e
    timeout "$LEG_TIMEOUT" claude -p \
      --model "$CLAUDE_MODEL" \
      --dangerously-skip-permissions \
      --verbose \
      --output-format stream-json \
      "$LEG_PROMPT" < /dev/null > "$LOG" 2>"$SCRATCH/leg-$N-claude.err"
    RC=$?
    set -e
    [ "$RC" -eq 0 ] || sed -n '1,40p' "$SCRATCH/leg-$N-claude.err" >&2 || true
  fi

  SECONDS_TAKEN=$(( $(date +%s) - START ))
  [ "$RC" -eq 0 ] || die "$leg leg exited $RC after ${SECONDS_TAKEN}s — a failed leg is a failed audit"
  harvest "leg $N" "$STAGE/$NAME"

  # Checked after the harvest, so the leg's own output directory is already out of the tree.
  AFTER="$(tree_state)"
  if [ "$BEFORE" = "$AFTER" ]; then TOUCHED="tree unchanged"; else TOUCHED="TREE CHANGED BY THE LEG"; fi
  [ "$BEFORE" = "$AFTER" ] || printf '\033[1;31m   leg %s changed the tree outside .audit-leg/:\033[0m\n%s\n' "$N" "$(diff <(echo "$BEFORE") <(echo "$AFTER") || true)" >&2

  # Observed, from what the run printed — not from the variable, which is what was asked for rather
  # than what answered.
  if [ "$leg" = codex ]; then
    OBSERVED="$(awk -F': *' '/^model:/ {print $2; exit}' "$LOG")"
    EFFORT="$(awk -F': *' '/^reasoning effort:/ {print $2; exit}' "$LOG")"
    TOKENS="$(awk '/^tokens used/ {getline; print; exit}' "$LOG" | tr -d ' ')"
    LEG_LINES+=("$(printf 'leg %s             codex %s · requested %s → observed %s · effort %s · %ss wall · tokens %s · %s · %s' \
      "$N" "$CODEX_VERSION" "$CODEX_MODEL" "${OBSERVED:-not reported}" "${EFFORT:-not reported}" "$SECONDS_TAKEN" "${TOKENS:-not reported}" "$COVERAGE" "$TOUCHED")")
  else
    # Every model in modelUsage, sorted — never its first key: a leg that fans out to sub-agents
    # reports more than one, in insertion order. node, because jq is not a dependency this repo
    # may assume; without node both are recorded not-reported.
    read -r OBSERVED TOKENS <<EOT
$(node -e '
  const j = require("fs").readFileSync(process.argv[1], "utf8").trim().split("\n")
    .map(l => { try { return JSON.parse(l); } catch { return {}; } }).findLast(m => m.type === "result");
  const u = j.usage || {};
  const models = Object.keys(j.modelUsage || {}).sort();
  const tok = ["input_tokens","output_tokens","cache_creation_input_tokens","cache_read_input_tokens"]
    .map(k => u[k]).filter(n => typeof n === "number").reduce((a,b) => a+b, 0);
  console.log((models.join(",") || "not-reported") + " " + (tok || "not-reported"));
' "$LOG" 2>/dev/null || echo "not-reported not-reported")
EOT
    LEG_LINES+=("$(printf 'leg %s             claude %s · requested %s → observed %s · %ss wall · tokens %s · %s · %s' \
      "$N" "$CLAUDE_VERSION" "$CLAUDE_MODEL" "$OBSERVED" "$SECONDS_TAKEN" "$TOKENS" "$COVERAGE" "$TOUCHED")")
  fi
  info "${LEG_LINES[-1]#leg * }"
  LEG_NAMES+=("$NAME")
  LEG_LOGS+=("$LOG")
done

# Every leg has run, so none can read another any more.
mkdir -p "$LEGS_DIR"
for NAME in "${LEG_NAMES[@]}"; do
  mv -T "$STAGE/$NAME" "$LEGS_DIR/$NAME"
done
mv -T "$SCRATCH/checks" "$OUT_DIR/checks"

# ---- provenance ---------------------------------------------------------------------------------
# Observed values, captured at run time. The rollup reproduces this block verbatim rather than
# composing one, so no field is ever a model's guess.

if [ "${#LEG_NAMES[@]}" -gt 1 ]; then
  INDEPENDENCE="${#LEG_NAMES[@]} legs, run without sight of each other"
else
  INDEPENDENCE="ONE LEG — nothing in this run was independently confirmed"
fi

PROVENANCE="$SCRATCH/provenance.txt"
{
  printf 'audit date        %s\n' "$AUDIT_DATE"
  printf 'code commit       %s  (scope dirty: %s · tree dirty: %s)\n' "$CODE_COMMIT" "$SCOPE_DIRTY" "$TREE_DIRTY"
  printf 'toolchain         %s\n' "$RUSTC_VERSION"
  printf 'scope             %s\n' "${SCOPE[0]}"
  for f in "${SCOPE[@]:1}"; do printf '                  %s\n' "$f"; done
  printf 'checks            %s passed · %s failed\n' "$CHECKS_PASS" "$CHECKS_FAIL"
  printf 'independence      %s\n' "$INDEPENDENCE"
  printf '%s\n' "${LEG_LINES[@]}"
} > "$PROVENANCE"

say "provenance"
cat "$PROVENANCE"

# ---- rollup -------------------------------------------------------------------------------------
# A synthesis, so a model writes it. The shell appends only the run-specific paths.

say "rollup — claude $CLAUDE_MODEL"
ROLLUP_PROMPT="$SCRATCH/rollup-prompt.md"
{
  cat "$REPO_ROOT/audits/prompts/rollup.md"
  printf '\n\n## This run\n\n'
  for NAME in "${LEG_NAMES[@]}"; do
    printf -- '- leg `%s`: `%s`\n' "$NAME" "audits/$AUDIT_DATE/rust-review/$NAME"
  done
  printf -- '- checks: `%s`\n' "audits/$AUDIT_DATE/checks"
  printf -- '- provenance file (reproduce verbatim): `%s`\n' "$PROVENANCE"
  printf -- '- write the document to: `%s`\n' "audits/$AUDIT_DATE/AUDIT.md"
  # Optional. Overridable because a blinded tree deliberately does not hold this file: the shell
  # reads it here and inlines it, so the answer reaches the rollup without being where a leg can
  # open it.
  KNOWN_ANSWER="${KNOWN_ANSWER_FILE:-$REPO_ROOT/audits/KNOWN_ANSWER.md}"
  if [ -s "$KNOWN_ANSWER" ]; then
    printf '\n## Known answer\n\n'
    cat "$KNOWN_ANSWER"
  fi
} > "$ROLLUP_PROMPT"

# On stdin, not as an argument: `--add-dir` is variadic and would take a positional prompt as one
# more directory.
ROLLUP_START=$(date +%s)
set +e
timeout "$LEG_TIMEOUT" claude -p \
  --model "$CLAUDE_MODEL" \
  --dangerously-skip-permissions \
  --add-dir "$SCRATCH" \
  < "$ROLLUP_PROMPT"
ROLLUP_RC=$?
set -e
ROLLUP_SECONDS=$(( $(date +%s) - ROLLUP_START ))
[ "$ROLLUP_RC" -eq 0 ] || die "rollup exited $ROLLUP_RC after ${ROLLUP_SECONDS}s"

# ---- completion ---------------------------------------------------------------------------------

say "check"
[ -s "$OUT_DIR/AUDIT.md" ] || die "audits/$AUDIT_DATE/AUDIT.md was not written, or is empty"
for NAME in "${LEG_NAMES[@]}"; do
  [ -s "$LEGS_DIR/$NAME/report.md" ] || die "empty leg: $NAME — an empty leg is a failed audit, not a clean one"
done

# The transcripts are the only record of what each leg read. Moved in last, after the rollup, so no
# leg and no rollup ever had them in the tree. rollup-prompt.md stays behind: it may carry the
# known answer.
mkdir -p "$OUT_DIR/transcripts"
for LOG in "${LEG_LOGS[@]}"; do
  mv "$LOG" "$OUT_DIR/transcripts/"
done

info "AUDIT.md    $(wc -l < "$OUT_DIR/AUDIT.md") lines"
info "checks      $CHECKS_PASS passed, $CHECKS_FAIL failed"
info "legs        ${LEG_NAMES[*]}"
info "rollup      ${ROLLUP_SECONDS}s wall"
info "transcripts $(du -sh "$OUT_DIR/transcripts" | cut -f1) in audits/$AUDIT_DATE/transcripts"

printf '\n\033[1maudit complete: audits/%s/AUDIT.md\033[0m\n' "$AUDIT_DATE"
