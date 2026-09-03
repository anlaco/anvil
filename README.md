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

## Run the example

**One binary** (`anvil`, ADR-0011) hosts wasmtime and the two WASM guests in
a sandbox. The package also carries `anvil-exec-wasm` next to it — the
executor that serves your `.wasm` steps (ADR-0023). You copy that file into a
folder together with your `.wasm` modules — that folder is a *department* —
and a sequence names its binary in `path:`; `anvil` spawns it (ADR-0027). Both
are statically linked against musl: they need no Rust, no cargo, no glibc,
nothing installed on the system.

```sh
curl -LO https://github.com/anlaco/anvil/releases/download/v0.4.0/anvil-v0.4.0-x86_64-linux-musl.tar.gz
tar xzf anvil-v0.4.0-x86_64-linux-musl.tar.gz
cd anvil-v0.4.0-x86_64-linux-musl

./anvil ejemplos/subsecuencia.yaml --json ./out.json --csv ./out.csv
```

Linux x86_64, any libc. The [release page][rel] publishes the SHA256 of the
`.tar.gz`; to check it, `sha256sum -c SHA256SUMS` with the second asset
downloaded alongside. The `.yaml` files ship in the package because
`subsecuencia.yaml` invokes `medir_fuentes.yaml` by relative path.

[rel]: https://github.com/anlaco/anvil/releases/latest

## Building from source

Only needed if you are going to touch the code; to *use* Anvil, download the
binary above.

> **Before building: clone `wasi-grpc` next to this repo.** The gRPC stack is
> referenced by relative path and is not published on crates.io yet, so
> without it `cargo` **cannot even read the manifest**. The repo is public
> and Apache-2.0: cloning it is all it takes.
>
> ```sh
> git clone https://github.com/anlaco/anvil
> git clone https://github.com/anlaco/wasi-grpc   # sibling, not inside
> cd anvil
> ```
>
> It is a stopgap and is acknowledged as such: [#25](https://github.com/anlaco/anvil/issues/25).

```sh
make release   # WASM guests → bridge → host, in that order

./packaging/anvil-host/target/release/anvil ejemplos/subsecuencia.yaml --json ./out.json --csv ./out.csv
```

These are three chained builds (the host's `build.rs` copies the artifacts,
it does not build them), and the order matters; the `Makefile` exists so you
do not have to remember it. By hand:

```sh
cargo build --release --target wasm32-wasip2 -p motor -p ejecutor_pasos      # guests
cargo build --release --manifest-path executors/wasm/Cargo.toml              # bridge (ADR-0015)
cargo build --release --manifest-path packaging/anvil-host/Cargo.toml        # host (embedded wasmtime)
```

That leaves a binary linked against your machine's glibc, which is what you
want for development. The **binary that gets published** in the releases is
another matter: it is built for the `x86_64-unknown-linux-musl` target so it
runs on any Linux. It requires a C compiler for musl, because `wasmtime`
drags in `zstd-sys`; `musl-gcc` or `zig cc -target x86_64-linux-musl` work
after `rustup target add x86_64-unknown-linux-musl`. The bridge must be
copied to `executors/wasm/target/release/` before building the host: its
`build.rs` looks for the artifacts there, and does not consider the
target-triple subdirectory.

`make build` does the same in debug. Use it for development, but expect that
binary to **start in tens of seconds**: wasmtime compiles the guests
unoptimized every time. The release one starts in ~1 s.

To debug the guests on their own with the wasmtime CLI (two terminals):

```sh
cargo build --target wasm32-wasip2 -p ejecutor_pasos -p motor
# terminal 1
wasmtime -S cli -S tcp=y -S inherit-network=y \
  target/wasm32-wasip2/debug/ejecutor_pasos.wasm
# terminal 2
wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
  target/wasm32-wasip2/debug/anvil-guest.wasm ejemplos/basica.yaml
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
  pasos_demo/      the example sequence's steps
  pasos_scpi/      a real step over SCPI on TCP (ADR-0017)
  ejecutor_pasos/  gRPC server: dispatches steps by name
  motor/           gRPC client: walks the sequence (bin `anvil-guest`)
packaging/
  anvil-host/      native host: one binary hosting wasmtime + the two guests
                   (its own workspace; the core drags no wasmtime)
executors/
  python/          the Python executor: a downloadable module (ADR-0012)
  wasm/            the WASM executor: the gRPC ↔ user's `.wasm` component
                   bridge (ADR-0015); its own workspace, shipped as a file
                   next to `anvil` (ADR-0023)
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
make test               # 369 core tests + 9 bridge + 26 host + the Python executor's
make check              # clippy for the three workspaces
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