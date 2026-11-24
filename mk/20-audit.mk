# Security review: the checks with one right answer, then independent adversarial reviews of the
# files in audits/SCOPE, reconciled into audits/<date>/AUDIT.md. The method is audits/README.md;
# the prompts are audits/prompts/, in prose, deliberately outside the runner.
#
# LEGS names the harnesses that review. Two labs is the method; LEGS=claude runs one and the record
# says so. Models are variables so neither is relitigated in prose: CLAUDE_MODEL is an alias, and
# the provenance block records what it resolved to.
LEGS         ?= codex claude
CLAUDE_MODEL ?= opus
CODEX_MODEL  ?= gpt-5.6-sol

.PHONY: audit audit-checks

audit: ## Security review: the checks, then adversarial reviews of audits/SCOPE, reconciled
	@LEGS="$(LEGS)" CLAUDE_MODEL=$(CLAUDE_MODEL) CODEX_MODEL=$(CODEX_MODEL) bash scripts/audit.sh

audit-checks: ## The checks only: advisories, licences, sources, unsafe, clippy, tests, scope
	@bash scripts/audit-checks.sh
