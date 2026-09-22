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

**Told nothing, it serves the folder its own binary is in**; `--modules <dir>`
points it at another
([ADR-0027](../../docs/adr/0027-a-sequence-names-the-executor-not-the-module.md)).
That is what makes a department a **copyable folder** of `.wasm` modules. A
sequence names neither the folder nor a module: it names the address the
executor listens at (ADR-0046), so where the modules live never leaks into it.

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

It does not use it: **Anvil connects to an address**
([ADR-0046](../../docs/adr/0046-an-executor-is-an-address-and-nothing-brings-one-up.md)).
A sequence says there is an executor at `127.0.0.1:9101`, and this bridge is
one of the things that can be listening there. It used to be spawned by the
host for a `type: wasm` executor, on an ephemeral port, from a `path:` the
sequence carried; none of that exists any more, and neither does the `path:`.

So the bridge is **installed once**, in the folder where a logical runtime
name resolves, instead of being copied next to `anvil` (which ADR-0023 said,
and ADR-0046 §4 amended) or next to every set of modules:

```
~/.anvil/executors/wasm/    executor.json  anvil-exec-wasm
```

`make install-executors` puts it there from a source tree, and the release
package carries an `executors/` folder with its own `install.sh`. Starting it
is somebody's job — a service manager, the machine's boot, a person in a
terminal, or the Sequence Editor from the sequence's `dev:` block. See
[executors/README-instalacion.md](../README-instalacion.md).

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
anvil-exec-wasm --modules ejemplos/departamento/dist --list
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

A component that **traps** —a `panic!`, an `unwrap()` that fails— no longer
cuts the run ([#58](https://github.com/anlaco/anvil/issues/58), fixed after
0.4.0). The step answers `error` naming the trap, and the trapped instance is
dropped and reloaded lazily on the module's next call (`src/main.rs`,
`ModuleSet::call`). Run against 0.5.0: a step that panics comes back as
`error`, and the same module then answers the `cleanup` steps normally. The
message is the WebAssembly backtrace, so a step should still return `error`
itself and say why.

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

The bridge serves **any** WebAssembly component that exports the
`anvil:step@0.4.0` world in [`wit/`](wit/), whatever produced it — which
language wrote it is opaque to it (ADR-0013). The authoring surface written on
top of it today is the [Rust SDK](../rust/): `#[step]` on a function and
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