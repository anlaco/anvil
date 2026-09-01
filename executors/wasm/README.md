# The WASM executor (bridge)

`anvil-puente-wasm` — a native binary that serves a user's `.wasm` step
component over gRPC ([ADR-0015](../../docs/adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md)).
Your step is a WIT component exporting `run` and `describe` (`anvil:step@0.4.0`); this
process loads it into wasmtime and turns it into an executor the engine can
dispatch to. It is the sibling of [`../python/`](../python/): both listen on
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
For every `type: wasm` executor declared in the sequence, `anvil` looks the
bridge up next to its own executable and spawns it itself: one process per
`.wasm` path, on an ephemeral loopback port, with stdin piped so the bridge
exits when the host dies. Nothing to install, nothing to start — and the
same file you got with the release is the one you can copy to another
machine and run by hand. If the file is missing, `anvil` stops with the
path it looked at and how to get it there.

## Running it by hand

The binary has a CLI of its own — useful to try a component without a
sequence around it:

```sh
anvil-puente-wasm --wasm <path.wasm> [--port <port>] [--bind <ip>]
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