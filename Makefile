# Anvil — build orchestration.
#
# `anvil` is a binary that embeds two WASM guests and the `.wasm` bridge
# (ADR-0011, ADR-0015), so three things must be built in order: the guests and
# the bridge first, the host last (its `build.rs` copies them, it does not
# build them). This exists so that order does not have to be remembered.
#
#   make build     everything in debug
#   make release   everything in release  ← the binary that gets distributed
#   make test      tests of the core, the host and the Python executor
#   make check     fmt + clippy for the three workspaces (what CI will demand)
#   make fmt       applies the format
#   make run       runs the basic example with the debug binary
#   make clean     cleans the three targets
#
# The three workspaces are deliberately independent (the core does not drag in
# wasmtime, ADR-0011), hence the `--manifest-path` calls.

HOST    := packaging/anvil-host/Cargo.toml
BRIDGE  := executors/wasm/Cargo.toml
GUESTS  := -p motor -p ejecutor_pasos
TARGET  := wasm32-wasip2

ANVIL_DEBUG   := packaging/anvil-host/target/debug/anvil
ANVIL_RELEASE := packaging/anvil-host/target/release/anvil

.PHONY: all build release test test-core test-bridge test-host test-executors check fmt run clean help

all: build

## Builds guests + bridge + host in debug.
build:
	cargo build --target $(TARGET) $(GUESTS)
	cargo build --manifest-path $(BRIDGE)
	cargo build --manifest-path $(HOST)
	@echo "ready → $(ANVIL_DEBUG)"

## Same, in release. The release binary starts in ~1 s; the debug one takes
## tens of seconds because wasmtime compiles the guests unoptimized.
release:
	cargo build --release --target $(TARGET) $(GUESTS)
	cargo build --release --manifest-path $(BRIDGE)
	cargo build --release --manifest-path $(HOST)
	@echo "ready → $(ANVIL_RELEASE)"

test: test-core test-bridge test-host test-executors

## Tests of the core workspace (no network and no compiled guests needed).
test-core:
	cargo test

## Tests of the bridge (its own workspace, like the host, but with no build.rs
## that demands artifacts: they are unit tests, pure).
test-bridge:
	cargo test --manifest-path $(BRIDGE)

## Tests of the host (its own workspace). Its `build.rs` demands the artifacts,
## so we build first.
test-host: build
	cargo test --manifest-path $(HOST)

## Tests of the Python step-executor SDK (`anvil_step`). stdlib only: they
## need neither `grpcio` nor the generated stubs, because what they test is
## the surface you write a step with, not the wire. Without python3, a warning
## is printed and we carry on: the core does not depend on it.
test-executors:
	@if command -v python3 >/dev/null 2>&1; then \
		cd executors/python && python3 -m unittest discover -p 'test_*.py'; \
	else \
		echo "no python3: Python executor tests skipped"; \
	fi

## Format and lints of the three workspaces, exactly what CI will demand.
## No `rustfmt.toml`: the format is Rust's default official style. The lints
## are at zero, so `-D warnings` cuts: a new warning is a failure, not noise.
##
## It depends on `build` because clippy over the host **runs its `build.rs`**,
## and that one demands the guests and the bridge already built (`cargo fmt`
## does not: it runs no build scripts). Without this dependency, `make check`
## on a clean tree dies with `failed to run custom build command for anvil-host`.
check: build
	cargo fmt --check
	cargo fmt --check --manifest-path $(HOST)
	cargo fmt --check --manifest-path $(BRIDGE)
	cargo clippy --all-targets -- -D warnings
	cargo clippy --manifest-path $(HOST) --all-targets -- -D warnings
	cargo clippy --manifest-path $(BRIDGE) --all-targets -- -D warnings

## Applies the format (what `check` verifies).
fmt:
	cargo fmt
	cargo fmt --manifest-path $(HOST)
	cargo fmt --manifest-path $(BRIDGE)

## Smoke: runs the basic example with the freshly built binary.
run: build
	$(ANVIL_DEBUG) ejemplos/basica.yaml

clean:
	cargo clean
	cargo clean --manifest-path $(HOST)
	cargo clean --manifest-path $(BRIDGE)

help:
	@grep -E '^##|^[a-z-]+:' $(MAKEFILE_LIST) | sed 's/^## /  /; s/:.*//'