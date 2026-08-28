# Language executors

One gRPC server per language that speaks the
[`paso.proto`](../crates/modelo/paso.proto) contract. Anvil's engine sees them
as endpoints: it dispatches by name→endpoint and does not know what sits
behind ([ADR-0012](../docs/adr/0012-executores-de-lenguaje-como-modulos.md)).

| | |
|---|---|
| [`python/`](python/) | The first one. You write a step as a function and drop it in a folder. |
| [`wasm/`](wasm/) | The WASM bridge. Serves your Rust steps compiled to components; Anvil brings it up by itself. |
| LabVIEW, MATLAB, … | Future ones. Each in its own subdirectory, with the same shape. |

They are **alternatives, not layers**: you pick the one you need, you can run
several at once, and mix them in the same sequence. The WASM executor Anvil
ships by default (`crates/ejecutor_pasos`) is a different piece: it is part
of the core, lives in `crates/`, and goes embedded in the binary. What does
live here is its **bridge** — the process that takes a user's `.wasm` step
component and serves it over gRPC like any other executor:
[`wasm/`](wasm/).

## License: **Apache-2.0**, and not the rest of the repo's

> Everything under this directory is **Apache-2.0** ([`LICENSE`](LICENSE)),
> not AGPL-3.0. Anvil —the sequencer, the repo root— is AGPL.

The border is not a whim and lives in
[ADR-0004](../docs/adr/0004-licencia-dual-agpl-apache.md): **what you *use* is
AGPL; what you *link* is Apache.**

Anvil is *used*: you hand it a sequence and it hands you back a verdict. A
language executor is not: its SDK goes **inside your code** the moment you
write `from anvil_step import step`. Copyleft there would be copyleft over
your test steps, which is exactly what ADR-0004 decides to avoid — same as
with `wasi-grpc` and `wasi-visa`.

**Your steps and your sequences are yours**, under whatever license you want,
and they catch nothing. The acceptance limits and the product know-how that
live in a sequence stay yours.

The files also carry their `SPDX-License-Identifier` in their header: this
directory sits inside an AGPL repository, and a single file someone copies
out has to keep saying what license it travels under.