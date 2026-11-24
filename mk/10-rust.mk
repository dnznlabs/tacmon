.PHONY: build test fmt lint clean run demo

build:: ## Build the release binary (target/release/tacmon)
	cargo build --release

test:: ## Run the tests
	cargo test

fmt:: ## Format the code
	cargo fmt

lint: ## Check formatting and run clippy, as the pre-commit hook does
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

clean:: ## Remove build output
	cargo clean

run: ## Run tacmon against the services on this machine
	cargo run --release

demo: ## Run tacmon on made-up data
	cargo run --release -- --demo
