// The sequence document: editing through the step view must not disturb the
// file around the edit.
//
// This is the test that decides whether the editor is usable on real
// sequences. Anvil's YAML is hand-written, lives in git and is reviewed in
// diffs, so an editor that rewrites the whole file on save makes every change
// unreviewable and gets abandoned after two uses (AP-05). "One line changed" is
// not a nicety here; it is the requirement.

import { strict as assert } from "node:assert";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { SequenceDocument } from "../src/document.mjs";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const basica = () => readFile(join(REPO, "ejemplos", "basica.yaml"), "utf8");

/** The lines that differ between two texts, as `[lineNumber, before, after]`. */
function changedLines(before, after) {
  const a = before.split("\n");
  const b = after.split("\n");
  const out = [];
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (a[i] !== b[i]) out.push([i + 1, a[i], b[i]]);
  }
  return out;
}

test("changing a limit rewrites exactly one line", async () => {
  const original = await basica();
  const doc = new SequenceDocument(original);

  // `demo/measure_voltage` is the first step of `main`, and its range is 4.5–5.5.
  const steps = doc.steps("main");
  const index = steps.findIndex((s) => s.name === "demo/measure_voltage");
  assert.notEqual(index, -1, "expected demo/measure_voltage in main");
  assert.equal(steps[index].limit.max, 5.5, "expected the fixture's 5.5 upper bound");

  doc.setStepLimit("main", index, "max", 6.5);

  const changed = changedLines(original, doc.text);
  assert.equal(
    changed.length,
    1,
    `expected exactly one changed line, got ${changed.length}:\n` +
      changed.map(([n, a, b]) => `  line ${n}: ${JSON.stringify(a)} -> ${JSON.stringify(b)}`).join("\n"),
  );
  assert.match(changed[0][2], /max:\s*6\.5/, "the changed line must be the upper bound");
});

test("the header comments survive an edit", async () => {
  const original = await basica();
  const doc = new SequenceDocument(original);

  // The fixture opens with nine lines of comments explaining where the
  // threshold comes from and which ADR decided it. They are the reason this
  // whole design exists.
  const commentsBefore = original.split("\n").filter((l) => l.trimStart().startsWith("#"));
  assert.ok(commentsBefore.length >= 9, "fixture should carry its header comments");

  const index = doc.steps("main").findIndex((s) => s.name === "demo/measure_voltage");
  doc.setStepLimit("main", index, "max", 6.5);

  const commentsAfter = doc.text.split("\n").filter((l) => l.trimStart().startsWith("#"));
  assert.deepEqual(commentsAfter, commentsBefore, "no comment may be dropped or reworded");
});

// A sequence saved on Windows usually has CRLF line endings — git checks the
// repo's own files out that way there, and so does a Windows editor. The
// emitter writes `\n`, and re-emitting in LF turned a one-value change into a
// diff of every line: 26 of 26 in `basica.yaml`, caught on windows-latest.
// The CRLF copy is built here, so the test does not depend on how git happened
// to check the fixture out.
test("a CRLF file keeps its line endings, so an edit is still one line", async () => {
  const original = (await basica()).replace(/\r\n/g, "\n").replace(/\n/g, "\r\n");
  const doc = new SequenceDocument(original);

  const index = doc.steps("main").findIndex((s) => s.name === "demo/measure_voltage");
  doc.setStepLimit("main", index, "max", 6.5);

  const changed = changedLines(original, doc.text);
  assert.equal(changed.length, 1, `expected exactly one changed line, got ${changed.length}`);
  assert.equal(doc.text.replace(/\r\n/g, "").includes("\n"), false, "no bare LF may be introduced");
  assert.equal(
    /\r\n$/.test(doc.text),
    /\r\n$/.test(original),
    "the trailing line break stays as the file had it",
  );
});

test("the step view reads the loader's vocabulary", async () => {
  const doc = new SequenceDocument(await basica());

  assert.equal(doc.name, "basica");
  assert.deepEqual(
    doc.steps("setup").map((s) => s.name),
    ["demo/connect"],
  );
  assert.deepEqual(
    doc.steps("main").map((s) => s.name),
    ["demo/measure_voltage", "demo/check_led"],
  );
  assert.deepEqual(
    doc.steps("cleanup").map((s) => s.name),
    ["demo/disconnect"],
  );

  // A declared `retries` is shown as declared.
  const [setup] = doc.steps("setup");
  assert.equal(setup.retries, 3);
});

test("absent fields are shown as the engine will read them", async () => {
  // `retries` defaults to 1 when absent (crates/cargador/src/lib.rs:318) and
  // `type` to grpc (lib.rs:322). The panel must show the effective value, not a
  // blank: a step that will be retried once must not look like a step that will
  // not be retried at all.
  //
  // This needs its own fixture. Every step in `ejemplos/basica.yaml` declares
  // `retries`, so asserting the defaults against it passes no matter what the
  // defaults are — which is what mutating them proved.
  const doc = new SequenceDocument("name: bare\nmain:\n  - name: measure\n");

  const [step] = doc.steps("main");
  assert.equal(step.retries, 1, "an undeclared retries reads as 1");
  assert.equal(step.type, "grpc", "an undeclared type reads as grpc");
  assert.equal(step.disable, false, "an undeclared disable reads as false");
});

test("broken text keeps the last good tree and says so", async () => {
  const doc = new SequenceDocument(await basica());
  assert.equal(doc.stale, false);

  // Mid-keystroke state: a value opened and not closed.
  doc.setText("name: basica\nmain:\n  - name: [unclosed\n");

  assert.equal(doc.stale, true, "the view must be marked as not reflecting the text");
  assert.ok(doc.error, "there must be an error to point at");
  assert.deepEqual(
    doc.steps("main").map((s) => s.name),
    ["demo/measure_voltage", "demo/check_led"],
    "the last good structure stays visible rather than blanking",
  );
  assert.match(doc.text, /\[unclosed/, "the text stays exactly as typed");
});

test("recovering from broken text clears the mark", async () => {
  const original = await basica();
  const doc = new SequenceDocument(original);

  doc.setText("name: basica\nmain:\n  - name: [unclosed\n");
  assert.equal(doc.stale, true);

  doc.setText(original);
  assert.equal(doc.stale, false, "a parse that succeeds must clear staleness");
  assert.equal(doc.error, null);
});
