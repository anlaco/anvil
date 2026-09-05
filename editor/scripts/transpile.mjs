// Transpiles the engine guest (`anvil-guest.wasm`) into JavaScript so it can be
// hosted somewhere that is not wasmtime — a browser tab, or Node for testing.
//
// The guest is unchanged: this is the same component `packaging/anvil-host`
// embeds (ADR-0011). What differs is who provides its WASI imports.
//
// Uses `@bytecodealliance/jco-transpile` rather than the full `jco` package on
// purpose: `jco` pulls in `componentize-js`, which exists to *build* components
// out of JavaScript — something this project never does — and which drags an
// unpatchable Zip Slip advisory in through `weval` -> `decompress`. Transpiling
// alone audits clean.

import { readFile, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { transpileBytes, writeFiles } from "@bytecodealliance/jco-transpile";

const HERE = dirname(fileURLToPath(import.meta.url));
const EDITOR = resolve(HERE, "..");
const REPO = resolve(EDITOR, "..");

// Release, not debug: the debug guest is 23 MB against 1.2 MB and takes tens of
// seconds just to start (see the note in the Makefile).
const GUEST = join(REPO, "target", "wasm32-wasip2", "release", "anvil-guest.wasm");
const OUT = join(EDITOR, "generated");

const wasm = await readFile(GUEST).catch(() => {
  throw new Error(
    `engine guest not found at ${GUEST}\n` +
      `build it first with:  make release   (from ${REPO})`,
  );
});

await rm(OUT, { recursive: true, force: true });

// Point every WASI import at src/wasi/, which re-exports the shim's *browser*
// build. Left alone, the generated module asks for `@bytecodealliance/
// preview2-shim/cli`, which resolves to the Node build under Node — a build
// whose stdio goes through an I/O worker and which we never ship. Routing
// through our own modules means Node tests and the browser exercise the same
// code, and it is where the `wasi:sockets` implementation of ADR-0030 will go
// when the bridge lands. `--validate` opens no socket, so nothing needs it yet.
const { files } = await transpileBytes(wasm, {
  name: "anvil",
  // One instance per run, not one per page load. The guest is a `wasi:cli/run`
  // command: it starts, runs and exits, and calling `run` twice on the same
  // instance traps with `unreachable` — found by running it, not by reading it.
  // The native host has the same constraint and answers it the same way, with a
  // fresh `Store` per guest (ADR-0011). `instantiation: "async"` gives us a
  // factory instead of a module that runs on import, so the compiled core
  // module is reused while each run gets clean linear memory.
  instantiation: "async",
  map: {
    "wasi:cli/*": "../src/wasi/cli.mjs#*",
    "wasi:clocks/*": "../src/wasi/clocks.mjs#*",
    "wasi:filesystem/*": "../src/wasi/filesystem.mjs#*",
    "wasi:io/*": "../src/wasi/io.mjs#*",
    "wasi:random/*": "../src/wasi/random.mjs#*",
    "wasi:sockets/*": "../src/wasi/sockets.mjs#*",
  },
  noTypescriptNamespaces: true,
});

// `writeFiles` takes options, not a path: without `baseDir` it writes the
// generated tree into the current working directory.
await writeFiles(files, { baseDir: OUT });

const total = Object.values(files).reduce((n, b) => n + b.byteLength, 0);
console.log(
  `transpiled ${(wasm.length / 1e6).toFixed(2)} MB guest -> ` +
    `${Object.keys(files).length} files, ${(total / 1e6).toFixed(2)} MB in editor/generated/`,
);
