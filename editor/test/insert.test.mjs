// The rule that holds the editor together, as a test: the editor cannot build
// a sequence the loader refuses (AP-04).
//
// Every step type the palette offers is inserted into a real fixture and the
// result is handed to the real engine. If it comes back rejected, the palette
// is offering something that produces a broken file — which is exactly what it
// did on the first attempt: a `statement` step assigning to `locals.ok` in a
// sequence that declares no `locals`.

import { strict as assert } from "node:assert";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { SequenceDocument, STEP_TYPES } from "../src/document.mjs";
import { runEngine as run } from "../src/engine.mjs";
import { exampleFiles } from "./ejemplos.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..");
const load = (name) => readFile(join(HERE, "..", "generated", name));
const fixture = (name) => readFile(join(REPO, "ejemplos", name), "utf8");

/**
 * Runs the loader over a document's current text and returns its verdict.
 *
 * `alongside` mounts the other files the sequence needs: the demo bench's
 * binary, and for `subsecuencia.yaml` the `./medir_fuentes.yaml` it calls — a
 * file the loader cannot read is a load error.
 */
async function validate(doc, alongside = {}) {
  return run({
    args: ["seq.yaml", "--validate"],
    files: { "seq.yaml": doc.text, ...alongside },
    load,
  });
}

for (const type of STEP_TYPES) {
  test(`inserting a '${type}' step leaves the sequence loadable`, async () => {
    // `sequence_call` needs a subsequence to call, so it gets the fixture that
    // has one; the others use the plainest sequence in the repo, which is also
    // the least forgiving — `basica.yaml` declares no variables at all.
    const file = type === "sequence_call" ? "subsecuencia.yaml" : "basica.yaml";
    const doc = new SequenceDocument(await fixture(file));
    const { [file]: _self, ...alongside } = await exampleFiles(file);

    const before = await validate(doc, alongside);
    assert.equal(before.exitCode, 0, `fixture ${file} should start valid:\n${before.stderr}`);

    assert.equal(doc.cannotAdd(type), null, `${file} should accept a ${type} step`);
    doc.addStep("main", type);

    const after = await validate(doc, alongside);
    assert.equal(
      after.exitCode,
      0,
      `inserting a '${type}' step made the sequence invalid:\n${after.stderr}\n\n${doc.text}`,
    );
  });
}

test("a sequence_call is refused when there is no subsequence to call", async () => {
  // `sequence` is mandatory on that type (crates/cargador/src/lib.rs:2323-2328)
  // and naming one that does not exist fails to load, so there is no valid step
  // to insert. Refusing with a reason beats inserting something broken.
  const doc = new SequenceDocument(await fixture("basica.yaml"));

  const why = doc.cannotAdd("sequence_call");
  assert.ok(why, "a file with no subsequences must refuse a sequence_call");
  assert.match(why, /subsequence/i, "the refusal must say what is missing");
  assert.throws(() => doc.addStep("main", "sequence_call"), /subsequence/i);
});

test("an inserted step is readable by the step view straight away", async () => {
  // `seq.add(plainObject)` emits fine but leaves an item with no `get`, so the
  // step view read it back as unnamed until the document was re-parsed.
  const doc = new SequenceDocument(await fixture("basica.yaml"));
  const index = doc.addStep("main", "statement");

  const step = doc.steps("main")[index];
  assert.equal(step.name, "new_statement", "the new step must show its name");
  assert.equal(step.type, "statement", "the new step must show its type");
});

test("inserted names do not collide", async () => {
  const doc = new SequenceDocument(await fixture("basica.yaml"));
  doc.addStep("main", "grpc");
  doc.addStep("main", "grpc");

  const names = doc.steps("main").map((s) => s.name);
  assert.equal(new Set(names).size, names.length, `names collided: ${names.join(", ")}`);
});
