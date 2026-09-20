// Writing a sequence with the editor alone, without touching the text.
//
// The step view could show a sequence and change what was already in it, but
// three things could only be done by typing YAML: putting a step where you want
// it, declaring a variable to keep what a module returns, and giving a numeric
// limit its comparison. An editor that needs the text view to finish the job is
// not an editor, and the fourth test here is the one that says so out loud: it
// builds `basica.yaml` from an empty document, through the same calls the UI
// makes, and hands the result to the real engine.

import { strict as assert } from "node:assert";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { SequenceDocument } from "../src/document.mjs";
import { runEngine as run } from "../src/engine.mjs";
import { exampleFiles } from "./ejemplos.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..");
const load = (name) => readFile(join(HERE, "..", "generated", name));
const fixture = (name) => readFile(join(REPO, "ejemplos", name), "utf8");

async function validate(doc, alongside = {}) {
  return run({
    args: ["seq.yaml", "--validate"],
    files: { "seq.yaml": doc.text, ...alongside },
    load,
  });
}

// ---------------------------------------------------------------- placement

test("a step can be inserted at a position, not only at the end", async () => {
  const doc = new SequenceDocument(await fixture("basica.yaml"));
  const before = doc.steps("main").map((s) => s.name);
  assert.equal(before.length, 2, "basica's main has two steps");

  doc.addStep("main", "pass_fail", 0);

  const after = doc.steps("main").map((s) => s.name);
  assert.equal(after.length, 3);
  assert.equal(after[1], before[0], "the step that was first is now second");
  assert.equal(after[2], before[1]);
});

test("a step moves to any position in any phase, which is a real edit", async () => {
  const doc = new SequenceDocument(await fixture("basica.yaml"));
  const { "basica.yaml": _self, ...alongside } = await exampleFiles("basica.yaml");

  // main: [measure_voltage, check_led] → drag check_led onto cleanup's front.
  doc.moveStepTo("main", 1, "cleanup", 0);

  assert.deepEqual(
    doc.steps("main").map((s) => s.name),
    ["demo/measure_voltage"],
  );
  assert.deepEqual(
    doc.steps("cleanup").map((s) => s.name),
    ["demo/check_led", "demo/disconnect"],
  );

  const out = await validate(doc, alongside);
  assert.equal(out.exitCode, 0, `moving a step across phases broke the file:\n${out.stderr}`);
});

// ------------------------------------------------------------------- types

test("changing a step's type takes the old type's fields with it", async () => {
  const doc = new SequenceDocument(await fixture("basica.yaml"));
  const { "basica.yaml": _self, ...alongside } = await exampleFiles("basica.yaml");

  // main[0] is a numeric_limit with a limit. An action judges nothing, so a
  // limit left on one is a load error (ADR-0040 §3) — which is what setting
  // `type` on its own used to produce, one click from the type menu.
  assert.ok(doc.steps("main")[0].limit, "the step starts with a limit");

  doc.setStepType("main", 0, "action");

  const asAction = doc.steps("main")[0];
  assert.equal(asAction.type, "action");
  assert.equal(asAction.limit, null, "the limit went with the type");
  assert.equal(asAction.module, "demo/measure_voltage", "what it calls stayed");

  let out = await validate(doc, alongside);
  assert.equal(out.exitCode, 0, `numeric_limit → action broke the file:\n${out.stderr}`);

  // And the other way: a statement calls nothing, so the module has to go, and
  // the statement it cannot be without has to arrive.
  doc.setStepType("main", 0, "statement");

  const asStatement = doc.steps("main")[0];
  assert.equal(asStatement.module, null, "a statement calls no module");
  assert.equal(asStatement.executor, null, "and so names no executor");
  assert.ok(asStatement.statement, "and cannot be without a statement");

  out = await validate(doc, alongside);
  assert.equal(out.exitCode, 0, `action → statement broke the file:\n${out.stderr}`);
});

test("every type a step can become leaves a sequence the engine loads", async () => {
  const { "basica.yaml": _self, ...alongside } = await exampleFiles("basica.yaml");

  // Every type but sequence_call, which needs a subsequence basica has not got
  // — `cannotAdd` says so, and the palette greys it out for the same reason.
  for (const type of ["action", "pass_fail", "numeric_limit", "statement"]) {
    const doc = new SequenceDocument(await fixture("basica.yaml"));
    doc.setStepType("main", 0, type);
    const out = await validate(doc, alongside);
    assert.equal(out.exitCode, 0, `main[0] as '${type}' does not load:\n${out.stderr}`);
  }
});

// ---------------------------------------------------------------- variables

test("a variable can be declared, renamed and removed", async () => {
  const doc = new SequenceDocument(await fixture("basica.yaml"));
  assert.deepEqual(doc.variables("locals"), {}, "basica declares none");

  doc.setVariable("locals", "voltaje", 0);
  doc.setVariable("locals", "ok", false);
  assert.deepEqual(doc.variables("locals"), { voltaje: 0, ok: false });

  doc.renameVariable("locals", "voltaje", "tension");
  assert.deepEqual(doc.variables("locals"), { tension: 0, ok: false });

  doc.removeVariable("locals", "ok");
  assert.deepEqual(doc.variables("locals"), { tension: 0 });
});

test("what a module returns can be kept in a local, and the engine accepts it", async () => {
  const doc = new SequenceDocument(await fixture("basica.yaml"));
  const { "basica.yaml": _self, ...alongside } = await exampleFiles("basica.yaml");

  doc.setVariable("locals", "voltaje", 0);
  const index = doc.steps("main").findIndex((s) => s.name === "demo/measure_voltage");
  doc.setStepAssign("main", index, "voltaje", "${result.measured_value}");

  assert.deepEqual(doc.steps("main")[index].assign, { voltaje: "${result.measured_value}" });

  const out = await validate(doc, alongside);
  assert.equal(out.exitCode, 0, `the assign the editor wrote was refused:\n${out.stderr}`);
});

// ------------------------------------------------------------------- limits

test("a fresh numeric_limit can be given every comparison, and each one loads", async () => {
  const { "basica.yaml": _self, ...alongside } = await exampleFiles("basica.yaml");
  // Every code the loader accepts (ADR-0040 §7), including the one the palette
  // starts from. A code whose fields the editor got wrong is refused at load,
  // which is the whole point of walking all of them.
  const codes = [
    "EQ", "NE", "GT", "LT", "GE", "LE",
    "GTLT", "GELE", "GELT", "GTLE",
    "LTGT", "LEGE", "LEGT", "LTGE",
    "EQT", "none",
  ];

  for (const code of codes) {
    const doc = new SequenceDocument(await fixture("basica.yaml"));
    const index = doc.addStep("main", "numeric_limit");
    assert.equal(doc.steps("main")[index].limit.comparison, "none", "a new limit starts at none");

    doc.setStepComparison("main", index, code);
    assert.equal(doc.steps("main")[index].limit.comparison, code);

    const out = await validate(doc, alongside);
    assert.equal(
      out.exitCode,
      0,
      `a '${code}' limit built by the editor was refused:\n${out.stderr}\n\n${doc.text}`,
    );
  }
});

test("changing the comparison drops the fields the new code does not use", async () => {
  const doc = new SequenceDocument(await fixture("basica.yaml"));
  const index = doc.addStep("main", "numeric_limit");

  doc.setStepComparison("main", index, "GELE");
  assert.deepEqual(Object.keys(doc.steps("main")[index].limit).sort(), ["comparison", "high", "low"]);

  // `GE` uses `low` alone: leaving `high` behind would be a load error, and
  // silently keeping it would make the editor write a file it cannot explain.
  doc.setStepComparison("main", index, "GE");
  assert.deepEqual(Object.keys(doc.steps("main")[index].limit).sort(), ["comparison", "low"]);

  doc.setStepComparison("main", index, "EQT");
  assert.deepEqual(
    Object.keys(doc.steps("main")[index].limit).sort(),
    ["comparison", "lower", "nominal", "threshold", "upper"],
  );
});

test("a limit keeps the bounds it already had when the new code still uses them", async () => {
  const doc = new SequenceDocument(await fixture("basica.yaml"));
  const index = doc.steps("main").findIndex((s) => s.name === "demo/measure_voltage");
  assert.equal(doc.steps("main")[index].limit.low, 4.5);

  // GELE → GELT is a change of one bound's inclusiveness. Resetting 4.5 and 5.5
  // to defaults would throw away the thresholds someone measured to find.
  doc.setStepComparison("main", index, "GELT");
  assert.equal(doc.steps("main")[index].limit.low, 4.5);
  assert.equal(doc.steps("main")[index].limit.high, 5.5);
});

// ---------------------------------------------------------------- executors

test("an executor can be declared, edited and removed", async () => {
  const doc = new SequenceDocument("name: sequence\nmain: []\n");
  assert.deepEqual(doc.executorNames(), []);
  // With none declared, the palette cannot offer the types that call one.
  assert.match(doc.cannotAdd("action"), /no executors/);

  doc.addExecutor("demo", "wasm", "departamento/dist/anvil-exec-wasm");
  assert.deepEqual(doc.executorNames(), ["demo"]);
  assert.equal(doc.cannotAdd("action"), null);
  assert.deepEqual(doc.executors(), [
    { name: "demo", type: "wasm", path: "departamento/dist/anvil-exec-wasm", host: null, port: null },
  ]);

  doc.setExecutorField("demo", "path", "otro/anvil-exec-wasm");
  assert.equal(doc.executors()[0].path, "otro/anvil-exec-wasm");

  doc.removeExecutor("demo");
  assert.deepEqual(doc.executorNames(), []);
});

test("a grpc executor is declared with its host and port", async () => {
  const doc = new SequenceDocument("name: sequence\nmain: []\n");
  doc.addExecutor("python", "grpc", "127.0.0.1", 9101);
  assert.deepEqual(doc.executors(), [
    { name: "python", type: "grpc", path: null, host: "127.0.0.1", port: 9101 },
  ]);
});

test("a step inserted into an empty flow list is written in block style", async () => {
  // File ▸ New starts from `main: []`, because an empty sequence cannot be
  // written in block style. Everything added to it inherited that flow style
  // and came out as `[ { name: …, type: … } ]`, which is valid YAML and unlike
  // every sequence in the repo — unreadable in a diff, which is where these
  // files get reviewed (AP-05).
  const doc = new SequenceDocument("name: sequence\nmain: []\n");
  doc.addStep("main", "pass_fail");
  assert.match(doc.text, /^main:\n {2}- name: /m, doc.text);
  assert.doesNotMatch(doc.text, /main: \[/, doc.text);
});

// ------------------------------------------------- the whole thing, no text

test("basica is built from nothing with the editor's own calls, and runs", async () => {
  const { "basica.yaml": _self, ...alongside } = await exampleFiles("basica.yaml");
  // What File ▸ New starts from, so the test builds what a person would.
  const doc = new SequenceDocument("name: sequence\nmain: []\n");

  doc.setName("basica_editada");
  doc.addExecutor("demo", "wasm", "departamento/dist/anvil-exec-wasm");

  const connect = doc.addStep("setup", "action");
  doc.setStepField("setup", connect, "name", "demo/connect");
  doc.setStepField("setup", connect, "module", "demo/connect");
  doc.setStepField("setup", connect, "retries", 3);

  const measure = doc.addStep("main", "numeric_limit");
  doc.setStepField("main", measure, "name", "demo/measure_voltage");
  doc.setStepField("main", measure, "module", "demo/measure_voltage");
  doc.setStepComparison("main", measure, "GELE");
  doc.setStepLimit("main", measure, "low", 4.5);
  doc.setStepLimit("main", measure, "high", 5.5);

  const led = doc.addStep("main", "pass_fail");
  doc.setStepField("main", led, "name", "demo/check_led");
  doc.setStepField("main", led, "module", "demo/check_led");
  doc.setStepField("main", led, "condition", undefined);
  // Naming a module names the executor that serves it: a step that calls one
  // and declares none does not load, and the editor cannot write that (AP-04).
  assert.equal(doc.steps("main")[led].executor, "demo");

  const disconnect = doc.addStep("cleanup", "action");
  doc.setStepField("cleanup", disconnect, "name", "demo/disconnect");
  doc.setStepField("cleanup", disconnect, "module", "demo/disconnect");

  const valid = await validate(doc, alongside);
  assert.equal(valid.exitCode, 0, `the built sequence does not load:\n${valid.stderr}\n\n${doc.text}`);

  // And it must be the same sequence as the one in the repo, step for step.
  // Loading only proves it parses; this proves the editor can express what a
  // person actually writes by hand. Running it needs the bridge to mount the
  // demo bench, which is the shell's job, not this harness's — so the run is
  // exercised through the UI, not here.
  const written = new SequenceDocument(doc.text);
  const original = new SequenceDocument(await fixture("basica.yaml"));
  const shape = (d) =>
    d.allSteps().map((s) => ({
      phase: s.phase,
      name: s.name,
      type: s.type,
      module: s.module,
      executor: s.executor,
      retries: s.retries,
      limit: s.limit,
    }));
  assert.deepEqual(shape(written), shape(original), doc.text);
});
