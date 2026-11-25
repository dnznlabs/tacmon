.PHONY: build test fmt lint clean run demo demo-video

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

# AGG_FLAGS: a font with braille for the start screen. Defaults to devbox's DejaVu: Sans Mono for text, Sans for the braille it lacks
DEVBOX_FONTS := $(wildcard .devbox/nix/profile/default/share/fonts/truetype)
# in a variable of its own: a comma written inside $(if) would split its arguments
DEVBOX_FONT_FLAGS = --font-dir $(DEVBOX_FONTS) --font-family "DejaVu Sans Mono,DejaVu Sans"
AGG_FLAGS ?= $(if $(DEVBOX_FONTS),$(DEVBOX_FONT_FLAGS))
demo-video: build ## Record docs/demo/demo.gif and demo.mp4 from tacmon --demo (needs python3, agg and ffmpeg)
	python3 scripts/record-demo.py target/release/tacmon docs/demo/demo.cast
	agg $(AGG_FLAGS) --font-size 14 --theme monokai --fps-cap 30 docs/demo/demo.cast docs/demo/demo.gif
	ffmpeg -y -loglevel error -i docs/demo/demo.gif -movflags +faststart -pix_fmt yuv420p -vf "fps=30,scale=trunc(iw/2)*2:trunc(ih/2)*2" -c:v libx264 -crf 20 docs/demo/demo.mp4

.PHONY: backup
backup: ## Snapshot the provider's swap records: make backup DEST=<dir on another disk>
	@bash scripts/backup-swaps.sh "$(DEST)"
