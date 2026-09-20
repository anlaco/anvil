// The files a sequence needs beside it, and the engine accepting them.

import { strict as assert } from "node:assert";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { runEngine as run } from "../src/engine.mjs";
import {
  gatherFiles,
  isPath,
  joinRelative,
  referencesOf,
  unsavedPathHint,
} from "../src/neighbours.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..");
const EJEMPLOS = join(REPO, "ejemplos");
const load = (name) => readFile(join(HERE, "..", "generated", name));

/** A reader over a real directory, the shape the desktop shell provides. */
const diskReader = (root) => ({
  exists: async (path) => existsSync(join(root, path)),
  readText: async (path) => (existsSync(join(root, path)) ? readFile(join(root, path), "utf8") : null),
});

/** A reader over a fixed map, for cases the repo has no file for. */
const mapReader = (map) => ({
  exists: async (path) => path in map,
  readText: async (path) => map[path] ?? null,
});

test("the loader's path rule, not a looser one", () => {
  assert.equal(isPath("./medir_fuentes.yseq"), true);
  assert.equal(isPath("sub.yseq"), true);
  assert.equal(isPath("init_comun"), false);
});

test("a relative path folds into the mounted root, and cannot climb out of it", () => {
  assert.equal(joinRelative("", "./departamento/dist/anvil-exec-wasm"), "departamento/dist/anvil-exec-wasm");
  assert.equal(joinRelative("sub", "../x.yaml"), "x.yaml");
  assert.equal(joinRelative("", "../x.yaml"), null);
  assert.equal(joinRelative("", "/abs/x.yaml"), null);
});

test("references are the wasm executors and the calls by path, inline subsequences included", () => {
  const refs = referencesOf(`
name: s
executors:
  - { name: dmm, type: wasm, path: dept/anvil-exec-wasm }
  - { name: py, type: grpc, host: 127.0.0.1, port: 9101 }
subsequences:
  inner:
    main:
      - { name: c, type: sequence_call, sequence: ./deep.yaml }
main:
  - { name: a, type: sequence_call, sequence: inner }
  - { name: b, type: sequence_call, sequence: ./other.yseq }
`);
  assert.deepEqual(refs.executors, ["dept/anvil-exec-wasm"]);
  assert.deepEqual(refs.sequences.sort(), ["./deep.yaml", "./other.yseq"]);
});

test("with no reader, only the open document is mounted", async () => {
  const text = await readFile(join(EJEMPLOS, "basica.yseq"), "utf8");
  assert.deepEqual(Object.keys(await gatherFiles("basica.yseq", text)), ["basica.yseq"]);
});

test("the demo bench's binary is mounted, and basica validates as the binary says", async () => {
  const text = await readFile(join(EJEMPLOS, "basica.yseq"), "utf8");
  const files = await gatherFiles("basica.yseq", text, diskReader(EJEMPLOS));
  assert.ok("departamento/dist/anvil-exec-wasm" in files, Object.keys(files).join(", "));

  const { exitCode, stderr } = await run({ args: ["basica.yseq", "--validate"], files, load });
  assert.equal(exitCode, 0, stderr);
});

test("an external subsequence is mounted, and so is what it references", async () => {
  const text = await readFile(join(EJEMPLOS, "subsecuencia.yseq"), "utf8");
  const files = await gatherFiles("subsecuencia.yseq", text, diskReader(EJEMPLOS));
  assert.ok("medir_fuentes.yseq" in files, Object.keys(files).join(", "));

  const { exitCode, stderr } = await run({ args: ["subsecuencia.yseq", "--validate"], files, load });
  assert.equal(exitCode, 0, stderr);
});

test("a Windows binary is mounted under its .exe name", async () => {
  const text = "name: s\nexecutors:\n  - { name: d, type: wasm, path: dist/anvil-exec-wasm }\nmain:\n  - { name: d/x, type: pass_fail, module: d/x, executor: d }\n";
  const files = await gatherFiles("s.yaml", text, mapReader({ "dist/anvil-exec-wasm.exe": "" }));
  assert.deepEqual(Object.keys(files).sort(), ["dist/anvil-exec-wasm.exe", "s.yaml"]);
});

test("a missing file is not invented, and a cycle ends", async () => {
  const a = "name: a\nmain:\n  - { name: c, type: sequence_call, sequence: ./b.yaml }\n  - { name: m, type: sequence_call, sequence: ./missing.yaml }\n";
  const b = "name: b\nmain:\n  - { name: c, type: sequence_call, sequence: ./a.yaml }\n";
  const files = await gatherFiles("a.yaml", a, mapReader({ "a.yaml": a, "b.yaml": b }));
  assert.deepEqual(Object.keys(files).sort(), ["a.yaml", "b.yaml"]);
});

test("a sequence that was never saved is told why its paths do not resolve", () => {
  // Found by building a sequence with File ▸ New and watching it be rejected:
  // an executor's `path` is relative to the sequence file, and a document that
  // has never been saved has no file to be relative to. The loader's message is
  // true — the path does not exist — but it reads as a typo, and the person
  // then goes looking for a mistake that is not in the file.
  const refused =
    "secuencia inválida: el ejecutor 'demo' es 'wasm' y su 'path' " +
    "'departamento/dist/anvil-exec-wasm' no existe";

  assert.match(unsavedPathHint(false, refused), /save/i);
  // Saved: the message is about the path, and the loader's own wording is the
  // precise one. Adding a hint there would send someone to save a file that is
  // already on disk.
  assert.equal(unsavedPathHint(true, refused), null);
  // Unsaved, but the complaint is not about a path: nothing to add.
  assert.equal(
    unsavedPathHint(false, "secuencia inválida: la sección 'main' es obligatoria"),
    null,
  );
});
