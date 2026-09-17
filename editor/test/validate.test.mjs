// The engine guest, run in a JavaScript host, validating sequences.
//
// This is the load-bearing test of the editor: it proves the same component the
// native binary embeds (ADR-0011) answers correctly with no wasmtime anywhere —
// which is what lets the editor validate offline, with no bridge and no bench
// (AP-07). If this breaks, nothing above it is worth debugging.
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

// Node reads the generated core modules from disk; the browser will fetch them.
// The engine host takes this as a parameter so it need not know which it is.
const load = (name) => readFile(join(HERE, "..", "generated", name));

const runEngine = (opts) => run({ ...opts, load });

test("a valid sequence validates clean", async () => {
  const { exitCode, stderr } = await runEngine({
    args: ["basica.yaml", "--validate"],
    files: await exampleFiles("basica.yaml"),
  });

  assert.equal(exitCode, 0, `expected a clean exit, got ${exitCode}:\n${stderr}`);
  assert.match(stderr, /'basica' válida/);
  // The engine counts what it loaded, and the count is part of the answer: a
  // sequence that validates while having read no steps would be a false green.
  assert.match(stderr, /2 paso\(s\) en main/);
});

test("an unknown field is named, located and corrected", async () => {
  // `steps` is in the loader's alias table (crates/cargador/src/lib.rs:643-663):
  // rejected, but recognised well enough to say what was meant. That suggestion
  // is what the editor will show, so it is what this asserts.
  const { exitCode, stderr } = await runEngine({
    args: ["broken.yaml", "--validate"],
    files: { "broken.yaml": "name: broken\nsteps:\n  - name: measure\n    type: pass_fail\n    module: measure\n" },
  });

  assert.equal(exitCode, 1, "a sequence with an unknown field must be rejected");
  assert.match(stderr, /campo desconocido 'steps'/);
  assert.match(stderr, /en la raíz/);
  assert.match(stderr, /¿querías 'main'\?/);
});

test("a sequence with no main is rejected", async () => {
  // `main` is required and non-empty (crates/cargador/src/lib.rs:2126-2146).
  const { exitCode, stderr } = await runEngine({
    args: ["empty.yaml", "--validate"],
    files: { "empty.yaml": "name: empty\nsetup:\n  - name: connect\n    type: pass_fail\n    module: connect\n" },
  });

  assert.equal(exitCode, 1, "a sequence without `main` must be rejected");
  assert.notEqual(stderr.trim(), "", "a rejection must say why");
});

test("the network is refused rather than faked", async () => {
  // `--validate --with-executors` connects (crates/motor/src/bin/anvil.rs:288-305),
  // and with no bridge attached there is nothing to connect through (ADR-0030).
  // What matters is that it fails loudly and says what is missing: answering
  // "connection refused" would let an absent *host* read as an executor that
  // was asked and said no — ADR-0019's Rule 2, exactly.
  //
  // A `grpc` executor, because that is what the engine itself connects to: a
  // `wasm` one reaches it only as the `--executor` override a host adds.
  const yaml =
    "name: remote\nexecutors:\n  - { name: bench, type: grpc, host: 127.0.0.1, port: 9101 }\n" +
    "main:\n  - name: measure\n    type: pass_fail\n    module: measure\n    executor: bench\n";

  await assert.rejects(
    () =>
      runEngine({
        args: ["remote.yaml", "--validate", "--with-executors"],
        files: { "remote.yaml": yaml },
      }),
    /no bridge is connected/,
    "reaching the network with no bridge must throw, not degrade quietly",
  );
});

// The editor offers a new document as `sequence.yseq` and hands the engine that
// name (ADR-0039). The engine guest is the same loader as the binary, so a
// `.yseq` must load like a `.yaml` here too — and a sequence_call naming a
// `.yseq` sibling with no slash must be read as a file, not as an inline name.
test("a .yseq sequence validates, and its .yseq subsequence is found as a file", async () => {
  const parent =
    "name: parent\nexecutors:\n  - { name: e, type: grpc, host: 127.0.0.1, port: 9101 }\n" +
    "main:\n  - name: c\n    type: sequence_call\n    sequence: child.yseq\n";
  const child = "name: child\nmain:\n  - name: m\n    module: m\n    type: pass_fail\n    executor: e\n";

  const { exitCode, stderr } = await runEngine({
    args: ["sequence.yseq", "--validate"],
    files: { "sequence.yseq": parent, "child.yseq": child },
  });

  assert.equal(exitCode, 0, `expected a clean exit, got ${exitCode}:\n${stderr}`);
  assert.match(stderr, /1 subsecuencia\(s\) externa\(s\)/);
});
