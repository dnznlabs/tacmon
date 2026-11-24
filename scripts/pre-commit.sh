#!/bin/bash
# Checks only: a hook that rewrites files collides with the commit's index lock.
set -e
if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not on PATH, skipping the Rust checks"
    exit 0
fi
cargo fmt --check
cargo clippy --quiet --all-targets -- -D warnings
