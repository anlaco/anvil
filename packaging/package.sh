#!/bin/bash
# Builds the downloadable package: the tarball a user gets from the release
# page, plus its SHA256SUMS.
#
# It exists because the package used to be assembled by hand, and what ships is
# not "whatever is in target/": it is the engine, a folder of executors to
# install (ADR-0046 §4), the example sequences, and the demo department's
# modules, so the WASM demos in the package run without building anything.
#
# Usage (from the repo root):
#   ./packaging/package.sh            # version taken from the host's manifest
#   ./packaging/package.sh 0.4.0      # or given explicitly
#
# Needs the musl target (`rustup target add x86_64-unknown-linux-musl`) and the
# WASM guests, which are built here in the order ADR-0011/ADR-0015 require.
#
# Windows sibling: packaging/package.ps1 (ADR-0036). It packages the same
# engine binaries for x86_64-pc-windows-msvc. The Sequence Editor is a
# separate download on both platforms — an Electron app built by
# `npm run app:build` in `editor/` (ADR-0037), not part of this tarball.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=x86_64-unknown-linux-musl
WASM=wasm32-wasip2
VERSION="${1:-$(grep -m1 '^version' packaging/anvil-host/Cargo.toml | cut -d'"' -f2)}"
NAME="anvil-v$VERSION-x86_64-linux-musl"
OUT="dist"

echo "== building $NAME =="
# The guest carries the version `anvil --version` prints, so they are built
# first and from the current manifest — not reused from a previous bump.
cargo build --release --target $WASM -p motor
cargo build --release --target $WASM --manifest-path ejemplos/hola-paso/Cargo.toml
cargo build --release --target $WASM --manifest-path ejemplos/departamento/Cargo.toml
cargo build --release --target $TARGET --manifest-path executors/wasm/Cargo.toml
cargo build --release --target $TARGET --manifest-path packaging/anvil-host/Cargo.toml

rm -rf "$OUT/$NAME"
mkdir -p "$OUT/$NAME/ejemplos/departamento/dist" "$OUT/$NAME/executors/wasm"

cp packaging/anvil-host/target/$TARGET/release/anvil "$OUT/$NAME/"
cp README.md CHANGELOG.md "$OUT/$NAME/"
cp LICENSE "$OUT/$NAME/LICENSE"                    # anvil: AGPL-3.0-or-later
cp executors/LICENSE "$OUT/$NAME/LICENSE.executors" # anvil-exec-wasm: Apache-2.0
cp ejemplos/*.yseq ejemplos/*.yaml ejemplos/arrancar-banco.sh "$OUT/$NAME/ejemplos/"

# The executors to install (ADR-0046 §4). The package is the engine plus this
# folder, not a binary with a bridge beside it: an executor is installed once,
# under `~/.anvil/executors/<runtime>/`, and a `dev:` block naming `wasm`
# resolves there. `executor.json` is what makes that possible for a runtime the
# tooling knows nothing about — it says which flag takes the code.
cp executors/wasm/target/$TARGET/release/anvil-exec-wasm "$OUT/$NAME/executors/wasm/"
cp executors/wasm/executor.json "$OUT/$NAME/executors/wasm/"
cp executors/README-instalacion.md "$OUT/$NAME/executors/README.md"
cp packaging/install-executors.sh "$OUT/$NAME/executors/install.sh"
chmod +x "$OUT/$NAME/executors/install.sh" "$OUT/$NAME/ejemplos/arrancar-banco.sh"

# The example department: its **modules**, which is what the demos' `dev:`
# block points `code:` at. The binary that serves them is installed once, above
# — since ADR-0046 it is not copied next to every set of modules.
cp ejemplos/departamento/target/$WASM/release/*.wasm "$OUT/$NAME/ejemplos/departamento/dist/"
cp ejemplos/hola-paso/target/$WASM/release/*.wasm "$OUT/$NAME/ejemplos/departamento/dist/"

tar czf "$OUT/$NAME.tar.gz" -C "$OUT" "$NAME"
(cd "$OUT" && sha256sum "$NAME.tar.gz" > SHA256SUMS)

echo
echo "package → $OUT/$NAME.tar.gz"
cat "$OUT/SHA256SUMS"
