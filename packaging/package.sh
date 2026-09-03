#!/bin/bash
# Builds the downloadable package: the tarball a user gets from the release
# page, plus its SHA256SUMS.
#
# It exists because the package used to be assembled by hand, and what ships is
# not "whatever is in target/": it is two statically linked musl binaries, the
# example sequences, and — since ADR-0027 — an assembled **department**, so the
# WASM demos in the package run without building anything.
#
# Usage (from the repo root):
#   ./packaging/package.sh            # version taken from the host's manifest
#   ./packaging/package.sh 0.4.0      # or given explicitly
#
# Needs the musl target (`rustup target add x86_64-unknown-linux-musl`) and the
# WASM guests, which are built here in the order ADR-0011/ADR-0015 require.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=x86_64-unknown-linux-musl
WASM=wasm32-wasip2
VERSION="${1:-$(grep -m1 '^version' packaging/anvil-host/Cargo.toml | cut -d'"' -f2)}"
NAME="anvil-v$VERSION-x86_64-linux-musl"
OUT="dist"

echo "== building $NAME =="
# The guests carry the version `anvil --version` prints, so they are built
# first and from the current manifest — not reused from a previous bump.
cargo build --release --target $WASM -p motor -p ejecutor_pasos
cargo build --release --target $WASM --manifest-path ejemplos/hola-paso/Cargo.toml
cargo build --release --target $WASM --manifest-path ejemplos/departamento/Cargo.toml
cargo build --release --target $TARGET --manifest-path executors/wasm/Cargo.toml
cargo build --release --target $TARGET --manifest-path packaging/anvil-host/Cargo.toml

rm -rf "$OUT/$NAME"
mkdir -p "$OUT/$NAME/ejemplos/departamento/dist"

cp packaging/anvil-host/target/$TARGET/release/anvil "$OUT/$NAME/"
cp executors/wasm/target/$TARGET/release/anvil-exec-wasm "$OUT/$NAME/"
cp README.md CHANGELOG.md "$OUT/$NAME/"
cp LICENSE "$OUT/$NAME/LICENSE"                    # anvil: AGPL-3.0-or-later
cp executors/LICENSE "$OUT/$NAME/LICENSE.executors" # anvil-exec-wasm: Apache-2.0
cp ejemplos/*.yaml "$OUT/$NAME/ejemplos/"

# The example department: the executor's binary with its modules beside it,
# which is what the demos' `path:` points at (ADR-0027). Without this the two
# WASM demos in the package would name a folder that is not there.
cp executors/wasm/target/$TARGET/release/anvil-exec-wasm "$OUT/$NAME/ejemplos/departamento/dist/"
cp ejemplos/departamento/target/$WASM/release/*.wasm "$OUT/$NAME/ejemplos/departamento/dist/"
cp ejemplos/hola-paso/target/$WASM/release/*.wasm "$OUT/$NAME/ejemplos/departamento/dist/"

tar czf "$OUT/$NAME.tar.gz" -C "$OUT" "$NAME"
(cd "$OUT" && sha256sum "$NAME.tar.gz" > SHA256SUMS)

echo
echo "package → $OUT/$NAME.tar.gz"
cat "$OUT/SHA256SUMS"
