// Runs the Anvil engine guest in a JavaScript host.
//
// This is the same component the native binary embeds (ADR-0011); only the
// provider of its WASI imports changes — here, the browser build of the shim,
// re-exported from ./wasi/. The engine cannot tell the difference, and that is
// the point: no fork of the engine exists for the editor.
//
// What this does NOT do is open a socket. `--validate` never connects
// (crates/motor/src/bin/anvil.rs:288-305), so the catalog and execution paths —
// which need the bridge of ADR-0030 — stay out of here.

import { instantiate } from "../generated/anvil.js";

import * as cliShim from "./wasi/cli.mjs";
import * as clocksShim from "./wasi/clocks.mjs";
import * as filesystemShim from "./wasi/filesystem.mjs";
import * as ioShim from "./wasi/io.mjs";
import * as randomShim from "./wasi/random.mjs";
import * as socketsShim from "./wasi/sockets.mjs";

// The generated module asks for its imports under the specifiers the transpiler
// was given in `scripts/transpile.mjs`.
const IMPORTS = {
  "../src/wasi/cli.mjs": cliShim,
  "../src/wasi/clocks.mjs": clocksShim,
  "../src/wasi/filesystem.mjs": filesystemShim,
  "../src/wasi/io.mjs": ioShim,
  "../src/wasi/random.mjs": randomShim,
  "../src/wasi/sockets.mjs": socketsShim,
};

const CORE_MODULES = ["anvil.core.wasm", "anvil.core2.wasm", "anvil.core3.wasm"];

/** Compiled once and reused; only the instance is per-run. */
let compiled = null;

/**
 * Loads and compiles the guest's core modules.
 *
 * `load` reads one generated file by name and returns its bytes. It is injected
 * because Node reads them from disk and a browser fetches them, and the engine
 * should not know which it is.
 */
async function compile(load) {
  if (compiled) return compiled;
  const modules = new Map();
  for (const name of CORE_MODULES) {
    modules.set(name, await WebAssembly.compile(await load(name)));
  }
  compiled = modules;
  return compiled;
}

/**
 * Collects everything written to a WASI output stream as text.
 *
 * The browser shim takes a plain `{ write }` handler, which is why ./wasi/ uses
 * that build; the Node build routes stdio through an I/O worker whose streams
 * cannot be swapped.
 */
function capture(onLine) {
  const chunks = [];
  // Chunk boundaries are not line boundaries: Rust's stderr is unbuffered, so
  // one `write_all` can reach the shim in pieces and two lines can arrive
  // together. The tail of a chunk is held until its newline shows up, and the
  // decoder is told the stream continues — otherwise a multi-byte character
  // straddling a boundary decodes as a replacement char.
  const decoder = onLine ? new TextDecoder() : null;
  let pending = "";
  return {
    handler: {
      write(contents) {
        chunks.push(contents.slice());
        if (!onLine) return;
        pending += decoder.decode(contents, { stream: true });
        let nl;
        while ((nl = pending.indexOf("\n")) >= 0) {
          const line = pending.slice(0, nl);
          pending = pending.slice(nl + 1);
          if (line) onLine(line);
        }
      },
    },
    text() {
      const total = chunks.reduce((n, c) => n + c.byteLength, 0);
      const all = new Uint8Array(total);
      let at = 0;
      for (const c of chunks) {
        all.set(c, at);
        at += c.byteLength;
      }
      return new TextDecoder().decode(all);
    },
  };
}

/**
 * Builds the in-memory filesystem the engine sees, from a flat map of
 * `path -> contents`.
 *
 * Paths may contain `/` and intermediate directories are created, so
 * `{ "ejemplos/basica.yaml": "..." }` produces the tree the engine expects when
 * given `ejemplos/basica.yaml` as its sequence argument.
 */
export function fileTree(files) {
  const root = { dir: {} };
  for (const [path, contents] of Object.entries(files)) {
    const parts = path.split("/").filter(Boolean);
    const name = parts.pop();
    let at = root;
    for (const part of parts) {
      at.dir[part] ??= { dir: {} };
      at = at.dir[part];
    }
    at.dir[name] = {
      source: typeof contents === "string" ? new TextEncoder().encode(contents) : contents,
    };
  }
  return root;
}

/**
 * Runs the engine once and returns what it said.
 *
 * `args` are the engine's own CLI arguments without `argv[0]`; the host supplies
 * that, the same way `packaging/anvil-host` does (main.rs:604-608).
 * `files` is a flat `path -> contents` map mounted as the preopened directory.
 * `load` reads a generated core module by name.
 *
 * Returns `{ exitCode, stdout, stderr }`. A non-zero exit code is an answer, not
 * a failure: for `--validate` it means the sequence was rejected, and the reason
 * is in `stderr`.
 *
 * `onStderrLine` is called with each complete stderr line **as it is written**,
 * which is what makes live progress possible: with `--events` those lines are
 * the NDJSON of ADR-0033. The full text still comes back at the end, so a caller
 * that only wants the report can ignore it. The callback runs inside the guest's
 * synchronous `run()`, so it must be cheap — hand the line on and return.
 *
 * A **fresh instance per call**. The guest is a `wasi:cli/run` command that
 * starts, runs and exits; running it twice on one instance traps with
 * `unreachable`. The native host has the same constraint and answers it the
 * same way, with a `Store` per guest (ADR-0011).
 */
export async function runEngine({ args = [], files = {}, load, onStderrLine }) {
  const modules = await compile(load);

  const out = capture();
  const err = capture(onStderrLine);

  cliShim._setArgs(["anvil", ...args]);
  cliShim._setStdout(out.handler);
  cliShim._setStderr(err.handler);
  filesystemShim._setFileData(fileTree(files));

  const instance = await instantiate((name) => modules.get(name), IMPORTS);

  let exitCode = 0;
  try {
    instance.run.run();
  } catch (e) {
    // The engine states its verdict by exiting, so a non-zero exit arrives as a
    // throw. Anything that is not an exit is rethrown: swallowing it would turn
    // a broken host into a failed sequence, which is the false red that Rule 2
    // of ADR-0019 exists to prevent.
    if (e?.exitError) {
      exitCode = e.code ?? 1;
    } else {
      throw e;
    }
  } finally {
    // The guest has exited, so what it left open closes with it — which is what
    // the kernel does for the native host and what nothing does here. An exit
    // runs no destructors, so the engine never drops its own sockets; leaving
    // them open holds the executor for good. See closeOpenSockets.
    socketsShim.closeOpenSockets();
  }

  return { exitCode, stdout: out.text(), stderr: err.text() };
}
