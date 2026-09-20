// stderr, delivered line by line while the engine runs.
//
// This is the half of live progress that lives in JavaScript. Until it existed,
// `capture()` accumulated every chunk and decoded once the guest had returned,
// so the editor could only ever report at the end — the engine's `--events`
// stream (ADR-0029, ADR-0033) would have arrived all at once, after the fact,
// which is not progress.
//
// What is NOT tested here is a full run with `--events`: the engine connects to
// its executors at start-up whatever the steps are, and the socket shim throws
// outright when no bridge is wired (`src/wasi/sockets.mjs`). So a real run is
// not reachable from Node, and the shape of the event stream is asserted where
// it is produced instead — `crates/result_sink/src/events.rs`. What belongs
// here is the delivery.
//
// Run with: npm test   (requires `npm run transpile` first)

import { strict as assert } from "node:assert";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { runEngine as run } from "../src/engine.mjs";
import { exampleFiles } from "./ejemplos.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const load = (name) => readFile(join(HERE, "..", "generated", name));

/** Validates `basica.yseq`, collecting stderr both ways. */
async function valida() {
  const lineas = [];
  const r = await run({
    args: ["basica.yseq", "--validate"],
    files: await exampleFiles("basica.yseq"),
    load,
    onStderrLine: (l) => lineas.push(l),
  });
  return { ...r, lineas };
}

test("stderr reaches the caller as whole lines", async () => {
  const { lineas, stderr } = await valida();

  assert.ok(lineas.length > 0, `nothing was delivered; stderr was:\n${stderr}`);
  // A caller must never have to reassemble a line. Chunk boundaries are not
  // line boundaries — Rust's stderr is unbuffered, so one write can arrive in
  // pieces and two lines can arrive together — and half a line that happened to
  // parse as JSON would be worse than no line at all.
  for (const l of lineas) {
    assert.ok(!l.includes("\n"), `a line arrived with a newline inside: ${l}`);
  }
});

test("the delivered lines are exactly the text, in order", async () => {
  const { lineas, stderr } = await valida();

  // The streamed view and the accumulated one are two renderings of one thing.
  // If they can disagree, a live view and the report it is meant to match will
  // disagree too, and that is the drift ADR-0031 exists to prevent.
  assert.deepEqual(lineas, stderr.split("\n").filter(Boolean));
});

test("delivery happens during the run, not after it", async () => {
  const files = await exampleFiles("basica.yseq");
  let durante = 0;
  let terminado = false;

  const p = run({
    args: ["basica.yseq", "--validate"],
    files,
    load,
    onStderrLine: () => {
      if (!terminado) durante += 1;
    },
  });
  const r = await p;
  terminado = true;

  // Every line was handed over before `runEngine` resolved. That is the whole
  // point: a callback that only fires afterwards is the old behaviour wearing a
  // new signature, and no assertion about line *content* would notice.
  assert.ok(durante > 0, "no line arrived before the run finished");
  assert.equal(durante, r.stderr.split("\n").filter(Boolean).length);
});

test("without a callback nothing changes for the caller", async () => {
  const { exitCode, stderr } = await run({
    args: ["basica.yseq", "--validate"],
    files: await exampleFiles("basica.yseq"),
    load,
  });

  assert.equal(exitCode, 0);
  assert.match(stderr, /'basica' válida/);
});
