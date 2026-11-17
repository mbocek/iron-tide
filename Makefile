BLENDER ?= blender
MODELS_DIR := assets/models
PREVIEWS_DIR := assets/previews
BLENDER_SCRIPT := tools/blender/build_models.py
BLENDER_RUN := $(BLENDER) -b --factory-startup --python $(BLENDER_SCRIPT) -- --out $(MODELS_DIR) --previews $(PREVIEWS_DIR)
RELEASE_BIN := target/release/iron_tide
CAPTURE ?= screenshot.png
CAPTURE_AFTER ?= 5
CAPTURE_FOCUS ?=
ONLY ?=

.DEFAULT_GOAL := help
.PHONY: help run run-release build release models models-fast previews capture check lint fmt fmt-check test test-unit test-playability clean clean-models

help: ## List the available targets
	@grep -E '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "} {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

run: ## Run the game (dev build)
	cargo run

run-release: release ## Run the optimized game
	BEVY_ASSET_ROOT=$(CURDIR) ./$(RELEASE_BIN)

build: ## Dev build
	cargo build

release: ## Optimized build
	cargo build --release

models: ## Generate every model and preview in Blender (ONLY=tank,scout limits the set)
	$(BLENDER_RUN) $(if $(ONLY),--only $(ONLY))

models-fast: ## Export the models without rendering previews (works without a GPU)
	$(BLENDER_RUN) --no-previews $(if $(ONLY),--only $(ONLY))

capture: release ## Save one screenshot and quit (CAPTURE=, CAPTURE_AFTER=secs, CAPTURE_FOCUS=x,y)
	BEVY_ASSET_ROOT=$(CURDIR) IRON_TIDE_CAPTURE=$(abspath $(CAPTURE)) IRON_TIDE_CAPTURE_AFTER_SECS=$(CAPTURE_AFTER) \
		$(if $(CAPTURE_FOCUS),IRON_TIDE_CAPTURE_FOCUS_CELL=$(CAPTURE_FOCUS)) ./$(RELEASE_BIN)

test: ## All tests: logic unit tests and headless playability scenarios
	cargo test --release

test-unit: ## Unit tests of the game logic only
	cargo test --release --lib

test-playability: ## Headless matches: movement, economy, production, combat, AI, missions, objectives, save/load
	cargo test --release --test playability --test missions --test fog --test sound

check: fmt-check lint test ## Formatting, lints and tests

lint: ## Clippy
	cargo clippy --all-targets -- -D warnings

fmt: ## Format the code
	cargo fmt

fmt-check: ## Check formatting
	cargo fmt -- --check

clean: ## Remove build artifacts
	cargo clean

clean-models: ## Delete the generated models and previews
	rm -f $(MODELS_DIR)/*.glb $(PREVIEWS_DIR)/*.png
