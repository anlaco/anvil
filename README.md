# anvil

A test sequencer: it runs step sequences against real equipment, retries the
ones that fail, and reports the outcome. Written in **Rust compiled to WASM**
(`wasm32-wasip2`, on wasmtime).

The sequence is **data**, not code: the engine walks it without knowing what
each step does, and each step is invoked **over gRPC by name** — never with a
direct call. That isolates the steps from one another and leaves the door
open to writing them in any language.

## Documentation

The product documentation (vision, requirements, architecture, ADRs, domain
design, licensing and roadmap) lives in [`docs/`](docs/README.md). Start at
[`docs/vision.md`](docs/vision.md).

To learn to *use* Anvil from nothing — install it, write steps in C#, write and
run sequences, read the reports — follow [The Anvil Book](docs/book/README.md).

## Run the example

**One binary** (`anvil`, ADR-0011) hosts wasmtime and the engine's WASM guest
in a sandbox. It carries no step executor of its own (ADR-0041): the package
also carries `anvil-exec-wasm` next to it — the executor that serves `.wasm`
steps (ADR-0023) — and a demo bench built with it, which the examples run
against. You copy that file into a
folder together with your `.wasm` modules — that folder is a *department* —
and a sequence names its binary in `path:`; `anvil` spawns it (ADR-0027). Both
are statically linked against musl: they need no Rust, no cargo, no glibc,
nothing installed on the system.

```sh
curl -LO https://github.com/anlaco/anvil/releases/download/v0.6.3/anvil-v0.6.3-x86_64-linux-musl.tar.gz
tar xzf anvil-v0.6.3-x86_64-linux-musl.tar.gz
cd anvil-v0.6.3-x86_64-linux-musl

./anvil ejemplos/subsecuencia.yaml --json ./out.json --csv ./out.csv
```

Linux x86_64, any libc. The [release page][rel] publishes one `SHA256SUMS`
for every download; to check the tarball, download it alongside and run
`sha256sum -c --ignore-missing SHA256SUMS` (it lists the Windows downloads
too, which you did not fetch). The `.yaml` files ship in the package because
`subsecuencia.yaml` invokes `medir_fuentes.yaml` by relative path, and the demo
bench ships in `ejemplos/departamento/dist/` because every example names it.

**Windows** (ADR-0036): the [release page][rel] also publishes
`anvil-vX.Y.Z-x86_64-windows.zip` — the same engine, `anvil.exe` and
`anvil-exec-wasm.exe`, statically linked against the CRT so it needs no
Visual C++ redistributable installed. Same contents and same commands as
above, with `.exe`.

## Sequence Editor

The engine binary above runs a sequence headless — CI, a bench, scripting.
To write one, download the Sequence Editor instead: an Electron app wrapping
the same editor that also runs standalone in any browser (`editor/`,
ADR-0031). It carries its own Chromium, so it needs no browser installed and
behaves the same on Linux and Windows (ADR-0037) — the reason it is not the
system's webview is that on Linux that would mean depending on whichever
`libwebkit2gtk` the machine happens to have. The [release page][rel]
publishes it as `anvil-editor-vX.Y.Z-x86_64-linux.AppImage`,
`anvil-editor-vX.Y.Z-amd64.deb` and
`anvil-editor-vX.Y.Z-x86_64-windows-setup.exe`. It does not carry the engine:
to run a sequence it uses the `anvil` installed on the machine — the one on
`PATH`, or the one you point it at with **File ▸ Locate Anvil Engine…**, which
it remembers. On Windows, a `PATH` set in one terminal is not seen by an editor
started from the Start menu, so locate `anvil.exe` once. To run it from source:
`cd editor && npm install && npm run app`, which uses the engine built in this
checkout.

[rel]: https://github.com/anlaco/anvil/releases/latest

## Building from source

Only needed if you are going to touch the code; to *use* Anvil, download the
binary above.

A clone of this repository is all it takes. The gRPC stack,
[`wasi-grpc`](https://github.com/anlaco/wasi-grpc), is a git dependency
pinned to a tag, and cargo fetches it (#25). To build against a local
checkout of it while working on both, use a `[patch]` section rather than
editing the dependency.

```sh
git clone https://github.com/anlaco/anvil
cd anvil
```

```sh
make release   # WASM guest and example components → bridge → host, in that order

./packaging/anvil-host/target/release/anvil ejemplos/subsecuencia.yaml --json ./out.json --csv ./out.csv
```

These are three chained builds (the host's `build.rs` copies the artifacts,
it does not build them), and the order matters; the `Makefile` exists so you
do not have to remember it. By hand:

```sh
cargo build --release --target wasm32-wasip2 -p motor                        # guest
cargo build --release --manifest-path executors/wasm/Cargo.toml              # bridge (ADR-0015)
cargo build --release --manifest-path packaging/anvil-host/Cargo.toml        # host (embedded wasmtime)
```

That leaves a binary linked against your machine's glibc, which is what you
want for development. The **binary that gets published** in the releases is
another matter: it is built for the `x86_64-unknown-linux-musl` target so it
runs on any Linux. It requires a C compiler for musl, because `wasmtime`
drags in `zstd-sys`; `musl-gcc` or `zig cc -target x86_64-linux-musl` work
after `rustup target add x86_64-unknown-linux-musl`. On Windows the
equivalent target is `x86_64-pc-windows-msvc`, built natively (no
cross-compiling): a runner or machine with the MSVC Build Tools already
resolves the same `zstd-sys` dependency, and `packaging/package.ps1` is the
Windows sibling of `packaging/package.sh` (ADR-0036). Both scripts build the
bridge for their target triple, and the host's `build.rs` finds it there
(#72). Releases are built by `.github/workflows/release.yml`, which runs each
script on the platform it is for and drafts the Release.

`make build` does the same in debug. Use it for development, but expect that
binary to **start in tens of seconds**: wasmtime compiles the guest
unoptimized every time. The release one starts in ~1 s.

To debug the engine guest on its own with the wasmtime CLI (two terminals),
start the demo bench's executor by hand and point the guest at it — the host
is what would otherwise start it and pass that `--executor`:

```sh
make release
# terminal 1
ejemplos/departamento/dist/anvil-exec-wasm --port 9300
# terminal 2
wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
  target/wasm32-wasip2/release/anvil-guest.wasm ejemplos/basica.yaml \
  --executor demo=127.0.0.1:9300
```

The wasmtime flags are not optional: without `-S tcp=y -S
inherit-network=y` the guest cannot touch the network. More in the
[quick-start guide](docs/guia-inicio-rapido.md).

## Layout

```
crates/
  modelo/          data model + paso.proto messages (prost)
  cargador/        YAML → model: validates, resolves paths, detects cycles
  expr/            expression engine (a Julia-syntax subset)
  result_sink/     report sinks: console, JSON, CSV
  motor/           gRPC client: walks the sequence (bin `anvil-guest`)
packaging/
  anvil-host/      native host: one binary hosting wasmtime + the engine guest
                   (its own workspace; the core drags no wasmtime)
executors/
  python/          the Python executor: a downloadable module (ADR-0012)
  rust/            the Rust step SDK: `#[step]` on a function, compiled to
                   a WASM component (ADR-0024)
  wasm/            the WASM executor: the gRPC ↔ user's `.wasm` component
                   bridge (ADR-0015); its own workspace, shipped as a file
                   next to `anvil` (ADR-0023)
  csharp/          the C# step SDK: `[Step]` on a method, served by the
                   user's own process (ADR-0038)
editor/            the Sequence Editor: a browser SPA, wrapped by Electron
                   for download (ADR-0031, ADR-0037)
```

The gRPC stack lives apart, in
[`anlaco/wasi-grpc`](https://github.com/anlaco/wasi-grpc): gRPC over native
WASI sockets, because `tonic`/`tokio` do not compile to WASM. anvil is its
first consumer and dogfoods it.

## The specification

These are the decisions that define the product. Do not touch them without
meaning to:

- **Execution semantics.** Setup → Main (only if Setup passed) →
  Cleanup. Main **stops at the first failure**; Cleanup **always runs** — an
  instrument left switched on is worse than a sequence that failed.
- **Retries per step.** Each step declares how many attempts it allows. The
  attempt number reaches the step, which may use it.
- **A closed vocabulary of statuses:** `pass`, `fail`, `error` and `skipped`.
  In the sequence aggregate an `error` wins over a `fail`, and the engine may
  add `inconclusive` when it could not judge (ADR-0019).
- **The contract** lives in `crates/modelo/paso.proto`: `StepRequest`,
  `StepResult` and `service StepExecutor { rpc Invoke, rpc Describe }`. It is
  the source of truth; the `prost` structs of `crates/modelo/src/proto.rs`
  mirror it by hand (wasi-grpc v0.1 has no codegen). `Describe` returns the
  executor's catalog —which steps it serves and with what signature— and is
  what lets `--validate --with-executors` catch a mistyped name without
  executing anything (ADR-0021).

## Verify

```sh
make test               # core, bridge, host, the Python, Rust and C# step SDKs, the editor
make check              # fmt + clippy for the Rust workspaces, dotnet format for C#
```

## License

**anvil is AGPL-3.0-or-later** (see [LICENSE](LICENSE)). anvil is the
product: it is *used*, not linked. The AGPL prevents anyone from closing it
and reselling it, and **it does not affect your test sequences** — they are
data you hand the sequencer, not a derivative work of it. The acceptance
limits and product know-how inside a sequence are yours and stay yours.

The libraries it rests on are deliberately **Apache-2.0**:

| Piece | License | Why |
|---|---|---|
| WIT interfaces | Apache-2.0 | We want them adopted as a reference |
| `wasi-grpc`, `wasi-visa` | Apache-2.0 | They get linked in someone else's code |
| `executors/` | Apache-2.0 | Their SDK enters your steps' code ([its own LICENSE](executors/LICENSE)) |
| anvil | AGPL-3.0 | It is the product |

A test step **links** with the libraries, so copyleft there would infect the
code of whoever uses them. In the sequencer it does not happen.