# Anvil — orquestación del build.
#
# `anvil` es un binario que embebe dos guests WASM y el puente `.wasm`
# (ADR-0011, ADR-0015), así que hay que compilar tres cosas en orden: los
# guests y el puente primero, el host al final (su `build.rs` los copia, no
# los construye). Esto existe para que ese orden no haya que recordarlo.
#
#   make build     todo en debug
#   make release   todo en release  ← el binario que se distribuye
#   make test      tests del core y del host
#   make check     fmt + clippy de los tres workspaces (lo que exigirá la CI)
#   make fmt       aplica el formato
#   make run       corre el ejemplo básico con el binario debug
#   make clean     limpia los tres targets
#
# Los tres workspaces son deliberadamente independientes (el core no arrastra
# wasmtime, ADR-0011), de ahí los `--manifest-path`.

HOST    := packaging/anvil-host/Cargo.toml
PUENTE  := packaging/anvil-puente-wasm/Cargo.toml
GUESTS  := -p motor -p ejecutor_pasos
TARGET  := wasm32-wasip2

ANVIL_DEBUG   := packaging/anvil-host/target/debug/anvil
ANVIL_RELEASE := packaging/anvil-host/target/release/anvil

.PHONY: all build release test test-core test-host check fmt run clean help

all: build

## Compila guests + puente + host en debug.
build:
	cargo build --target $(TARGET) $(GUESTS)
	cargo build --manifest-path $(PUENTE)
	cargo build --manifest-path $(HOST)
	@echo "listo → $(ANVIL_DEBUG)"

## Ídem en release. El binario de release arranca en ~1 s; el de debug tarda
## decenas de segundos porque wasmtime compila los guests sin optimizar.
release:
	cargo build --release --target $(TARGET) $(GUESTS)
	cargo build --release --manifest-path $(PUENTE)
	cargo build --release --manifest-path $(HOST)
	@echo "listo → $(ANVIL_RELEASE)"

test: test-core test-host

## Tests del workspace core (no necesitan red ni los guests compilados).
test-core:
	cargo test

## Tests del host (workspace aparte). Su `build.rs` exige los artifacts, así
## que compilamos antes.
test-host: build
	cargo test --manifest-path $(HOST)

## Formato y lints de los tres workspaces, tal como los exigirá la CI.
## Sin `rustfmt.toml`: el formato es el estilo oficial de Rust por defecto.
## Los lints están a cero, así que `-D warnings` corta: un aviso nuevo es un
## fallo, no ruido de fondo.
check:
	cargo fmt --check
	cargo fmt --check --manifest-path $(HOST)
	cargo fmt --check --manifest-path $(PUENTE)
	cargo clippy --all-targets -- -D warnings
	cargo clippy --manifest-path $(HOST) --all-targets -- -D warnings
	cargo clippy --manifest-path $(PUENTE) --all-targets -- -D warnings

## Aplica el formato (lo que `check` verifica).
fmt:
	cargo fmt
	cargo fmt --manifest-path $(HOST)
	cargo fmt --manifest-path $(PUENTE)

## Humo: corre el ejemplo básico con el binario recién construido.
run: build
	$(ANVIL_DEBUG) ejemplos/basica.yaml

clean:
	cargo clean
	cargo clean --manifest-path $(HOST)
	cargo clean --manifest-path $(PUENTE)

help:
	@grep -E '^##|^[a-z-]+:' $(MAKEFILE_LIST) | sed 's/^## /  /; s/:.*//'
