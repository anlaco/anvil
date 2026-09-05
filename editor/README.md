# Anvil editor

The graphical sequence editor. It runs **the engine itself** — the same
`anvil-guest.wasm` the native binary embeds (ADR-0011) — in a JavaScript host,
so a sequence can be loaded, validated and reported on with no wasmtime, no
bridge and no bench (AP-07).

Status: **milestone 1**. The engine runs and validates. There is no interface
yet, and nothing executes steps.

## Getting it running

```sh
make release          # from the repo root: builds target/wasm32-wasip2/release/anvil-guest.wasm
cd editor
npm install
npm run transpile     # engine guest -> editor/generated/ (git-ignored)
npm test
```

`npm run transpile` must be re-run whenever the engine changes; `generated/` is
build output and is not committed.

## What runs where

```
editor/
  scripts/transpile.mjs   engine guest -> JavaScript, via jco
  src/engine.mjs          runs the guest: args in, exit code + stdout/stderr out
  src/wasi/               the WASI imports the guest is given
  generated/              transpiler output (git-ignored)
  test/                   the engine, exercised through the host
```

The engine is **not forked** for the editor. It is the same component, given a
different provider for its WASI imports. Anything that would require changing
the engine to suit the editor is a design mistake, and the reason this holds is
that the editor never asks the engine for something the engine does not already
do (AP-04).

## Four things that will bite you

Each of these cost time to find. They are written down so they cost it once.

**`jco` on npm is not jco.** The package named `jco` is a placeholder published
by a third party to head off dependency-confusion attacks; it contains no usable
code. The real one is `@bytecodealliance/jco`.

**We install `jco-transpile`, not `jco`.** The full `jco` package depends on
`componentize-js`, which exists to *build* components out of JavaScript —
something this project never does — and which pulls in `weval` and then
`decompress`, whose Zip Slip advisory has no patched version because the package
is abandoned. Depending only on `@bytecodealliance/jco-transpile` audits clean
(`npm audit`: 0 vulnerabilities) and does everything needed here. Do not
"simplify" this back to `jco`.

**The guest is single-use, so there is one instance per run.** It is a
`wasi:cli/run` command: it starts, runs and exits. Calling `run` twice on one
instance traps with `unreachable` — which surfaces as a confusing failure in the
*second* test, not the first. Hence `instantiation: "async"` in the transpiler
options and a fresh instance in `runEngine`. The native host has the same
constraint and answers it the same way, with a `Store` per guest (ADR-0011).

**We use the shim's browser build, even under Node.**
`@bytecodealliance/preview2-shim` resolves to a Node-specific build under Node's
`node` export condition, and that build routes stdio through an I/O worker whose
streams cannot be swapped for a capture. The browser build takes a plain
`{ write }` handler and is what actually ships, so `src/wasi/` re-exports it by
file path — the package's `exports` map admits no subpath that names it.

## Sockets

`src/wasi/sockets.mjs` is deliberately unimplemented and throws. The engine
reaches executors over gRPC on raw TCP (ADR-0006), and browsers have no TCP at
all, so the real implementation must tunnel to a bridge over a WebSocket
(ADR-0030). Nothing on the current path needs it: `--validate` opens no socket.

It throws rather than returning an error value on purpose. A stub that quietly
answered "connection refused" would let a *missing host* read as an executor
that was asked and said no — the false red of ADR-0019's Rule 2.

## Tests

`npm test` runs the engine through the host and asserts on what it says. The
house rule applies here as everywhere: **a regression test that has not been
seen to fail is not verified**. All four tests in `test/validate.test.mjs` were
verified by mutation — breaking the thing each one asserts, one at a time, and
confirming that test and only that test went red.
