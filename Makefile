.PHONY: help init add
.DEFAULT_GOAL := help

help: ## List commands
	@awk -F ':+[^#]*## ' '/^[a-zA-Z0-9_-]+:+[^#]*## /{printf "  make %-12s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# `../shared` is an optional sibling checkout. With it, init and add use its scripts. Without it, init
# sets up the two things a contributor needs, and add says what it is missing.
SHARED := $(wildcard ../shared/scripts)

init: ## Init development: the devbox packages and the git hooks
ifneq ($(SHARED),)
	bash ../shared/scripts/init.sh $(if $(GSD),--gsd)
else
	@command -v devbox >/dev/null || { echo "make init needs devbox: https://www.jetify.com/devbox"; exit 1; }
	devbox install
	devbox run -- pre-commit install --install-hooks
endif

add: ## Add a template fragment: make add F=<name>; no F lists them
ifneq ($(SHARED),)
	@bash ../shared/scripts/add.sh $(F)
else
	@echo "make add needs the shared checkout beside this repo (../shared); nothing to do without it."
endif

include $(sort $(wildcard mk/*.mk))
