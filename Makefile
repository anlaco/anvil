# Anvil — build orchestration.
#
# `anvil` is a binary that embeds two WASM guests and the `.wasm` bridge
# (ADR-0011, ADR-0015), so three things must be built in order: the guests and
# the bridge first, the host last (its `build.rs` copies them, it does not
# build them). This exists so that order does not have to be remembered.
#
#   make build     everything in debug
#   make release   everything in release  ← the binary that gets distributed
#   make test      tests of the core, the host and the two executor SDKs
#   make check     fmt + clippy for the four workspaces (what CI will demand)
#   make fmt       applies the format
#   make run       runs the basic example with the debug binary
#   make clean     cleans the four targets
#
# The four workspaces are deliberately independent (the core does not drag in
# wasmtime, ADR-0011; the Rust step SDK links nothing of Anvil's, ADR-0024),
# hence the `--manifest-path` calls.

HOST    := packaging/anvil-host/Cargo.toml
BRIDGE  := executors/wasm/Cargo.toml
RUSTSDK := executors/rust/Cargo.toml
EXAMPLE := ejemplos/hola-paso/Cargo.toml
# The two-module department (`ejemplos/departamento`), what
# `demo_departamento.yaml` loads (ADR-0025).
DEPT    := ejemplos/departamento/Cargo.toml
GUESTS  := -p motor -p ejecutor_pasos
TARGET  := wasm32-wasip2

ANVIL_DEBUG   := packaging/anvil-host/target/debug/anvil
ANVIL_RELEASE := packaging/anvil-host/target/release/anvil

.PHONY: all build release test test-core test-bridge test-host test-executors \
        test-executors-rust example check fmt run clean help

all: build

## Builds guests + bridge + host in debug.
build: example
	cargo build --target $(TARGET) $(GUESTS)
	cargo build --manifest-path $(BRIDGE)
	cargo build --manifest-path $(HOST)
	@echo "ready → $(ANVIL_DEBUG)"

## The reference step component (`ejemplos/hola-paso`), the one
## `demo_wasm.yaml` loads. It builds with the plain toolchain: the SDK carries
## the WIT and the bindings, so there is no `cargo component` to install
## (ADR-0024). Until the SDK existed nothing built it — not the Makefile and
## not CI — and it was a manual acceptance criterion.
example:
	cargo build --target $(TARGET) --manifest-path $(EXAMPLE)
	cargo build --target $(TARGET) --manifest-path $(DEPT)

## Same, in release. The release binary starts in ~1 s; the debug one takes
## tens of seconds because wasmtime compiles the guests unoptimized.
release: example
	cargo build --release --target $(TARGET) $(GUESTS)
	cargo build --release --manifest-path $(BRIDGE)
	cargo build --release --manifest-path $(HOST)
	@echo "ready → $(ANVIL_RELEASE)"

test: test-core test-bridge test-host test-executors test-executors-rust

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

## Tests of the Rust step-authoring SDK (`anvil-step`). Native: what they test
## is the surface you write a step with, so none of them needs WASM — the same
## thing that lets a step's own unit tests call it directly.
test-executors-rust:
	cargo test --manifest-path $(RUSTSDK)

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
	cargo fmt --check --all --manifest-path $(RUSTSDK)
	cargo fmt --check --manifest-path $(EXAMPLE)
	cargo fmt --check --all --manifest-path $(DEPT)
	cargo clippy --all-targets -- -D warnings
	cargo clippy --manifest-path $(HOST) --all-targets -- -D warnings
	cargo clippy --manifest-path $(BRIDGE) --all-targets -- -D warnings
	cargo clippy --manifest-path $(RUSTSDK) --all-targets -- -D warnings
	cargo clippy --target $(TARGET) --manifest-path $(EXAMPLE) -- -D warnings
	cargo clippy --target $(TARGET) --manifest-path $(DEPT) -- -D warnings

## Applies the format (what `check` verifies).
fmt:
	cargo fmt
	cargo fmt --manifest-path $(HOST)
	cargo fmt --manifest-path $(BRIDGE)
	cargo fmt --all --manifest-path $(RUSTSDK)
	cargo fmt --manifest-path $(EXAMPLE)
	cargo fmt --all --manifest-path $(DEPT)

## Smoke: runs the basic example with the freshly built binary.
run: build
	$(ANVIL_DEBUG) ejemplos/basica.yaml

clean:
	cargo clean
	cargo clean --manifest-path $(HOST)
	cargo clean --manifest-path $(BRIDGE)
	cargo clean --manifest-path $(RUSTSDK)
	cargo clean --manifest-path $(EXAMPLE)
	cargo clean --manifest-path $(DEPT)

help:
	@grep -E '^##|^[a-z-]+:' $(MAKEFILE_LIST) | sed 's/^## /  /; s/:.*//'