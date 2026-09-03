# The WASM executor (bridge)

`anvil-exec-wasm` — a native binary that serves a user's `.wasm` step
components over gRPC ([ADR-0015](../../docs/adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md)).
Your step is a WIT component exporting `run` and `describe` (`anvil:step@0.4.0`); this
process loads it into wasmtime and turns it into an executor the engine can
dispatch to.

**It serves a folder of modules, not a file**
([ADR-0025](../../docs/adr/0025-the-executor-is-a-department-modules-by-logical-name.md)).
Every `*.wasm` it serves is a *module*, addressed by the **logical name** of
its file stem: `multimetro.wasm` is `multimetro`, and a sequence names a step
`multimetro/medir_voltaje`. Neither the extension nor the path ever reaches the
YAML, so the department can reorganise itself — or rewrite a module in another
language — without editing anybody's sequence.

**Told nothing, it serves the folder its own binary is in**
([ADR-0027](../../docs/adr/0027-a-sequence-names-the-executor-not-the-module.md)).
That is what makes a department a **copyable folder**: this binary with its
`.wasm` beside it. A sequence points `path:` at *this executor* and never at a
module, so where the modules live never leaks into it.

The qualified name travels inside `StepRequest.name`, which is an opaque
string as far as `paso.proto` is concerned: serving many modules costs no
contract change, no engine change and no WIT change.

It is the sibling of [`../python/`](../python/): both listen on
gRPC, speak [`paso.proto`](../../crates/modelo/paso.proto), answer the
engine's contract echo (ADR-0020 §4b), and the engine cannot tell them apart
— sequences mix both freely.

The component knows nothing about gRPC, protobuf or contract versions: it is
a function. The bridge is the only translator, which is why the bridge — not
your component — answers the contract echo.

## How Anvil uses it

The bridge ships as a **file next to `anvil`** — the release carries both,
and `make release` leaves them together in the target directory too
([ADR-0023](../../docs/adr/0023-the-bridge-ships-as-a-file-next-to-anvil.md)).
For every `type: wasm` executor declared in the sequence, `anvil` spawns
**the binary that sequence names in its `path:`** — one process per declared
executor, on an ephemeral loopback port, with stdin piped so the bridge exits
when the host dies. It is passed the port and nothing else: which modules it
serves is its own business. Nothing to install, nothing to start — and the
same file you got with the release is the one you copy into a department, or to
another machine to run by hand. If the `path:` a sequence names is not there —
or is a `.wasm` instead of the executor — `anvil` stops saying so.

## Running it by hand

The binary has a CLI of its own — useful to try a component without a
sequence around it:

```sh
anvil-exec-wasm [--modules <dir>... | --wasm <path.wasm>] \
    [--port <port>] [--bind <ip>] [--list]
```

With neither flag it serves the folder it is in, which is what Anvil relies on.
`--modules` is repeatable and points it at other directories; `--wasm` serves a
single file and then steps keep their **bare** names — a by-hand facility only,
since Anvil never passes it, so a step served through Anvil is always
qualified.

`--list` prints what is served — each module with its SHA-256 and each step
with its signature — and exits without listening. It is the *enumerate* door
an editor needs, and the way to answer "which steps does this executor serve?"
without starting a bench:

```sh
./ejemplos/departamento/dist/anvil-exec-wasm --list
```

`--bind 0.0.0.0` is what makes the remote case (the executor on another
machine — the Raspberry Pi case ADR-0015 anticipated) possible without
changing anything. Built by hand:

```sh
cargo build --release --manifest-path executors/wasm/Cargo.toml
```

## What it does not speak (yet)

The bridge implements `anvil:step@0.4.0`. Since 0.4.0 it **does** serve the
step catalog: the component publishes it through `describe` and the bridge
translates it, so a WASM step is checked before the run like any other
([ADR-0021](../../docs/adr/0021-el-ejecutor-describe-su-catalogo.md),
[ADR-0024](../../docs/adr/0024-the-signature-is-the-catalog-in-rust-too.md)).
That was the first WIT break, and it recompiled every component in the wild
(ADR-0020 §4d).

What it still does not speak is **object references**
([ADR-0022](../../docs/adr/0022-la-referencia-a-objeto-es-un-cuarto-tipo-y-nombra-una-ranura.md)),
which the Python executor does serve: a component is a function with no state
between calls, so it has nowhere to keep an open instrument session, and a
reference that reaches the bridge is rejected with that as the reason.

And one thing it gets wrong: a component that **traps** —a `panic!`, an
`unwrap()` that fails— takes its instance with it, and the bridge does not
reinstantiate, so the engine sees the stream close without an answer and the
run is cut. Verified 2026-09-01, tracked as
[#58](https://github.com/anlaco/anvil/issues/58).

There is no compatibility shim: the version lives in the package name and
travels with the artifact — wasmtime refuses to instantiate a component that
does not match, and the rule is to recompile (ADR-0020 §4d).

And the **artifact hash is not in the report yet**. The bridge computes the
SHA-256 of every module it serves, logs it on every start and shows it in
`--list`, but ADR-0025 §6 asks for it to be recorded in each run's report, and
there is no field in `paso.proto` to carry it. That needs a contract change and
its own ADR: until then, do not read a green report as saying which artifact
produced it.

## Loading

Modules in a **directory** load on demand: the bridge reads their names and
hashes at start-up (which compiles nothing) and compiles a component the first
time something needs it. So a broken `.wasm` sitting in the folder no longer
takes the whole bridge down — only whoever uses it fails — and a module that
cannot describe itself leaves its steps out of the catalog with the reason on
stderr, rather than sinking everyone else's (ADR-0021 §4).

A single `--wasm` loads **eagerly**, on purpose: with one file pointed at by
hand there are no others to protect, and finding out at the first step that it
is not a component would be finding out with the unit already on the bench.

Assembling a department is copying this binary and the modules into one folder.
`make build` does it for the repo's example, in
[`ejemplos/departamento/dist/`](../../ejemplos/departamento/). Use the
**release** binary: the debug one carries an unoptimised wasmtime and weighs
about thirteen times more.

Two files with the same stem make the bridge refuse to start, naming both:
serving the wrong module is worse than not starting.

## Reference

Steps are written with the [Rust SDK](../rust/): `#[step]` on a function and
`cargo build --target wasm32-wasip2`. The complete hello-world is
[`ejemplos/hola-paso/`](../../ejemplos/hola-paso/), the official reference for
the [quick-start guide](../../docs/guia-inicio-rapido.md#writing-your-own-step-in-rust-adr-0015-adr-0024).

## License

**Apache-2.0**, like everything under
[`executors/`](../README.md#license-apache-20-and-not-the-rest-of-the-repos)
([ADR-0004](../../docs/adr/0004-licencia-dual-agpl-apache.md)): what you
*use* is AGPL; what you *link* is Apache. Your `.wasm` links the
Apache-2.0 SDK — never this bridge — and stays yours, under whatever license
you want.