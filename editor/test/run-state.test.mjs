// Which row lights up, decided line by line.
//
// This is the piece where a wrong answer points an operator at the wrong step
// while a unit is on the bench, so it is a pure state machine and it is
// asserted here with no browser. The lines below are the real shape the engine
// emits — taken from `anvil ejemplos/basica.yseq --events`, trimmed of the
// fields the view does not read.
//
// Run with: npm test

import { strict as assert } from "node:assert";
import { test } from "node:test";

import {
  applyEvent,
  callStack,
  neverRan,
  nestedRows,
  newRunState,
  rowKey,
  runButton,
} from "../src/run-state.mjs";

const ev = (o) => JSON.stringify(o);

const inicio = (plan) =>
  ev({ event: "sequence_start", sequence: "basica", events_version: 1, plan, seq: 0 });

const paso = (event, phase, index, seq, extra = {}) =>
  ev({
    event,
    name: `p${index}`,
    step_run_id: `id-${phase}-${index}`,
    parent_run_id: null,
    depth: 0,
    phase,
    locator: { sequence: "basica", path: [phase, index] },
    seq,
    ...extra,
  });

const PLAN = [
  { name: "conectar", phase: "setup", index: 0 },
  { name: "medir", phase: "main", index: 0 },
  { name: "led", phase: "main", index: 1 },
];

test("the running row is the one the engine says, and it clears when it ends", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  assert.equal(s.current, null);

  applyEvent(s, paso("step_start", "main", 0, 1));
  assert.deepEqual(s.current, { phase: "main", index: 0 });

  applyEvent(s, paso("step_result", "main", 0, 2, { status: "fail" }));
  assert.equal(s.results.get(rowKey("main", 0)), "fail");
  // The verdict lands but the row is still the current one until it closes.
  assert.deepEqual(s.current, { phase: "main", index: 0 });

  applyEvent(s, paso("step_end", "main", 0, 3));
  assert.equal(s.current, null);
  // The verdict outlives the run: it is what the row shows afterwards.
  assert.equal(s.results.get(rowKey("main", 0)), "fail");
});

test("a step inside a subsequence names its call instead of moving the highlight", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  applyEvent(s, paso("step_start", "main", 0, 1));

  // Three segments: this runs inside `main[0]`, and the editor has no row for
  // it. Moving the highlight would point at a row that is not what is running.
  applyEvent(
    s,
    ev({
      event: "step_start",
      name: "medir_interno",
      step_run_id: "hijo",
      parent_run_id: "id-main-0",
      depth: 1,
      phase: "main",
      locator: { sequence: "sub", path: ["main", 0, "main", 0] },
      seq: 2,
    }),
  );

  assert.deepEqual(s.current, { phase: "main", index: 0 }, "the call stays lit");
  assert.equal(s.nested, "medir_interno");
});

test("a gap in seq is counted, not swallowed", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  applyEvent(s, paso("step_start", "main", 0, 1));
  // seq 2 and 3 never arrive.
  applyEvent(s, paso("step_start", "main", 1, 4));

  assert.equal(s.lost, 2);
  // And the state still moved: a lost line costs its own information, not the
  // rest of the run.
  assert.deepEqual(s.current, { phase: "main", index: 1 });
});

test("a step that never ran is distinguishable from one whose lines were lost", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  for (const [ph, ix, seq] of [
    ["setup", 0, 1],
    ["main", 0, 4],
  ]) {
    applyEvent(s, paso("step_start", ph, ix, seq - 1));
    applyEvent(s, paso("step_result", ph, ix, seq, { status: "pass" }));
    applyEvent(s, paso("step_end", ph, ix, seq + 1));
  }
  applyEvent(s, ev({ event: "sequence_end", status: "pass", seq_total: 7, seq: 6 }));

  // No gap, so silence means it did not run — that is the answer you want
  // after an abort, and the plan is what makes it answerable.
  assert.equal(s.lost, 0);
  assert.deepEqual(
    neverRan(s).map((p) => p.name),
    ["led"],
  );
});

test("with lines lost, it refuses to say which step never ran", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  applyEvent(s, paso("step_result", "setup", 0, 5, { status: "pass" }));

  assert.ok(s.lost > 0);
  // Answering here would be a guess presented as a fact about a unit, which is
  // exactly what Rule 2 of ADR-0019 forbids.
  assert.equal(neverRan(s), null);
});

test("an unknown event is skipped and the run carries on", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  applyEvent(s, paso("step_start", "main", 0, 1));

  // Skipping means leaving everything as it was. An unknown event that moved
  // the highlight would be worse than one that broke the view: the row would
  // go dark, or land elsewhere, with nothing saying why.
  applyEvent(s, ev({ event: "step_attempt", seq: 2 }));
  assert.deepEqual(s.current, { phase: "main", index: 0 }, "the row did not move");
  assert.equal(s.lost, 0, "an event it does not know is not a lost line");

  // ...and the run carries on normally afterwards.
  applyEvent(s, paso("step_end", "main", 0, 3));
  assert.equal(s.current, null);
});

test("foreign text on the shared stderr is ignored without comment", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  const antes = { ...s, results: new Map(s.results) };

  assert.equal(applyEvent(s, "paso pedido: medir_voltaje intento=1"), false);
  assert.equal(applyEvent(s, ""), false);
  assert.equal(applyEvent(s, "{not json"), false);

  assert.equal(s.seq, antes.seq, "foreign text does not move the counter");
  assert.equal(s.lost, 0);
});

test("sequence_end closes the run without inventing a verdict", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  applyEvent(s, paso("step_start", "main", 0, 1));
  applyEvent(s, ev({ event: "sequence_end", status: "fail", seq_total: 3, seq: 2 }));

  assert.equal(s.current, null);
  assert.equal(s.done, true);
  // The aggregate status is on the line and deliberately not stored: the
  // verdict is the report and the exit code, not something the view derives.
  assert.equal(s.results.size, 0);
});

test("Run is not offered while a run is in flight", () => {
  // The bug this guards: the button was computed from the bridge alone, so any
  // repaint during a run — inserting a step was how it was found — re-armed it
  // and offered a second run over an engine that takes one sequence at a time.
  const enVuelo = runButton({ hasDoc: true, bridged: true, inFlight: true });
  assert.equal(enVuelo.disabled, true);
  assert.match(enVuelo.title, /in flight/);

  const listo = runButton({ hasDoc: true, bridged: true, inFlight: false });
  assert.equal(listo.disabled, false);
});

test("a refused Run says which of the two things is missing", () => {
  // A control that is refused without saying why reads as broken, and the two
  // reasons need different actions from whoever is at the bench.
  const sinPuente = runButton({ hasDoc: true, bridged: false, inFlight: false });
  assert.equal(sinPuente.disabled, true);
  assert.match(sinPuente.title, /--bridge/);

  const sinFichero = runButton({ hasDoc: false, bridged: true, inFlight: false });
  assert.equal(sinFichero.disabled, true);

  // In flight wins over no bridge: it is the more recent truth, and it is the
  // one that says "wait" rather than "go and start something".
  const ambos = runButton({ hasDoc: true, bridged: false, inFlight: true });
  assert.match(ambos.title, /in flight/);
});

test("a Run the desktop app could not arm says why, not how to start a bridge", () => {
  // #80: the packaged editor starts the bridge itself. When it cannot find the
  // engine, telling the person to run `anvil --bridge` by hand sends them the
  // wrong way; the shell's own reason is the instruction.
  const reason = "anvil was not found on PATH — use File ▸ Locate Anvil Engine…";
  const noEngine = runButton({ hasDoc: true, bridged: false, inFlight: false, unavailable: reason });
  assert.equal(noEngine.disabled, true);
  assert.match(noEngine.title, /Locate Anvil Engine/);
  assert.doesNotMatch(noEngine.title, /--bridge/);

  // A run in flight still wins, as above.
  const running = runButton({ hasDoc: true, bridged: false, inFlight: true, unavailable: reason });
  assert.match(running.title, /in flight/);
});

// ---------------------------------------------------------------------------
// The call stack, and the rows a subsequence's steps now have.
//
// Until these existed the step list was flat: a `sequence_call` stayed lit and
// the status bar named whatever was running under it, so nothing on screen
// said what had already passed or failed down there. `editor/README.md` listed
// that as one of two honest limits. The engine had been emitting what it takes
// — `parent_run_id` and `depth` — since ADR-0033.
// ---------------------------------------------------------------------------

/** A step one level down, inside the subsequence `main[0]` calls. */
const dentro = (event, name, seq, extra = {}) =>
  ev({
    event,
    name,
    step_run_id: `sub-${name}`,
    parent_run_id: "id-main-0",
    depth: 1,
    phase: "main",
    locator: { sequence: "sub", path: ["main", 0, "main", 0] },
    seq,
    ...extra,
  });

test("the call stack is what is running and what it runs under", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  applyEvent(s, paso("step_start", "main", 0, 1));
  applyEvent(s, dentro("step_start", "medir_interno", 2));

  const { frames, truncated } = callStack(s);
  assert.equal(truncated, false);
  assert.deepEqual(
    frames.map((f) => f.name),
    ["p0", "medir_interno"],
    "outermost first, as TestStand shows it",
  );
  assert.deepEqual(frames.map((f) => f.depth), [0, 1]);
});

test("the stack unwinds to the caller when the inner step ends", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  applyEvent(s, paso("step_start", "main", 0, 1));
  applyEvent(s, dentro("step_start", "medir_interno", 2));
  applyEvent(s, dentro("step_end", "medir_interno", 3));

  assert.deepEqual(callStack(s).frames.map((f) => f.name), ["p0"]);
  // And the call is still the lit row: it has not finished.
  assert.deepEqual(s.current, { phase: "main", index: 0 });
});

test("a stack whose parent line was lost says so instead of starting halfway", () => {
  // ADR-0033 §2: paternity is asserted on every line, so a lost line costs one
  // node. What must not happen is the walk stopping quietly — a stack that
  // begins in the middle reads as the whole truth, which is Rule 2 of
  // ADR-0019 all over again.
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  // `step_start` for main[0] never arrives; its child does.
  applyEvent(s, dentro("step_start", "medir_interno", 2));

  const { frames, truncated } = callStack(s);
  assert.deepEqual(frames.map((f) => f.name), ["medir_interno"]);
  assert.equal(truncated, true, "the walk ran out of parents and admits it");
});

test("a step inside a subsequence gets a row under the call it ran in", () => {
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  applyEvent(s, paso("step_start", "main", 0, 1));
  applyEvent(s, dentro("step_start", "medir_interno", 2));
  applyEvent(s, dentro("step_result", "medir_interno", 3, { status: "fail" }));

  const under = nestedRows(s);
  const rows = under.get(rowKey("main", 0));
  assert.equal(rows.length, 1);
  assert.equal(rows[0].name, "medir_interno");
  assert.equal(rows[0].status, "fail", "the nested row carries its own verdict");

  // And it did not leak into the parent row's verdict: that is the call's own,
  // and it has not produced one yet.
  assert.equal(s.results.has(rowKey("main", 0)), false);
});

test("a nested step whose ancestry was lost is not filed under the wrong call", () => {
  // Guessing a parent here attaches a step to a call it never ran in, which is
  // worse than not showing it: it is a wrong answer stated confidently.
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  applyEvent(s, paso("step_start", "main", 1, 1));
  // Its `parent_run_id` names a step that never arrived.
  applyEvent(s, dentro("step_start", "huerfano", 2));

  assert.equal(nestedRows(s).size, 0, "no row claims it");
});

test("a parent chain that loops does not hang the interface", () => {
  // The ids come off a wire this code does not control.
  const s = newRunState();
  applyEvent(s, inicio(PLAN));
  applyEvent(
    s,
    ev({
      event: "step_start",
      name: "bucle",
      step_run_id: "a",
      parent_run_id: "a",
      depth: 1,
      phase: "main",
      locator: { sequence: "sub", path: ["main", 0, "main", 0] },
      seq: 1,
    }),
  );
  const { truncated } = callStack(s);
  assert.equal(truncated, true);
});

// ---------------------------------------------------------------------------
// The whole of a real run, replayed.
//
// The lines below are **the engine's own output**, captured from
//
//     anvil ejemplos/subsecuencia.yseq --events
//
// trimmed to the fields this view reads, with the 128-bit run ids replaced by
// short ones so the parentage is legible. Nothing else is edited.
//
// The tests above each pin one rule against a line written to exercise it.
// This one checks the rules compose over a stream nobody wrote for them: two
// sequence calls, six nested steps, one of them in the subsequence's own
// cleanup phase, and nineteen lines in the order the engine really emits them.
// ---------------------------------------------------------------------------

const STREAM = [
  {"event": "sequence_start", "seq": 0, "plan": [{"name": "preparar", "phase": "main", "index": 0}, {"name": "test_fuentes", "phase": "main", "index": 1}]},
  {"event": "step_start", "name": "preparar", "step_run_id": "r0", "parent_run_id": null, "depth": 0, "phase": "main", "locator": {"sequence": "basica", "path": ["main", 0]}, "seq": 1},
  {"event": "step_start", "name": "preparar_canal", "step_run_id": "r1", "parent_run_id": "r0", "depth": 1, "phase": "main", "locator": {"sequence": "init_comun", "path": ["main", 0, "main", 0]}, "seq": 2},
  {"event": "step_result", "name": "preparar_canal", "step_run_id": "r1", "parent_run_id": "r0", "depth": 1, "phase": "main", "locator": {"sequence": "init_comun", "path": ["main", 0, "main", 0]}, "status": "done", "seq": 3},
  {"event": "step_end", "name": "preparar_canal", "step_run_id": "r1", "parent_run_id": "r0", "depth": 1, "phase": "main", "locator": {"sequence": "init_comun", "path": ["main", 0, "main", 0]}, "seq": 4},
  {"event": "step_result", "name": "preparar", "step_run_id": "r0", "parent_run_id": null, "depth": 0, "phase": "main", "locator": {"sequence": "basica", "path": ["main", 0]}, "status": "pass", "seq": 5},
  {"event": "step_end", "name": "preparar", "step_run_id": "r0", "parent_run_id": null, "depth": 0, "phase": "main", "locator": {"sequence": "basica", "path": ["main", 0]}, "seq": 6},
  {"event": "step_start", "name": "test_fuentes", "step_run_id": "r2", "parent_run_id": null, "depth": 0, "phase": "main", "locator": {"sequence": "basica", "path": ["main", 1]}, "seq": 7},
  {"event": "step_start", "name": "ajustar_canal", "step_run_id": "r3", "parent_run_id": "r2", "depth": 1, "phase": "main", "locator": {"sequence": "medir_fuentes", "path": ["main", 1, "main", 0]}, "seq": 8},
  {"event": "step_result", "name": "ajustar_canal", "step_run_id": "r3", "parent_run_id": "r2", "depth": 1, "phase": "main", "locator": {"sequence": "medir_fuentes", "path": ["main", 1, "main", 0]}, "status": "done", "seq": 9},
  {"event": "step_end", "name": "ajustar_canal", "step_run_id": "r3", "parent_run_id": "r2", "depth": 1, "phase": "main", "locator": {"sequence": "medir_fuentes", "path": ["main", 1, "main", 0]}, "seq": 10},
  {"event": "step_start", "name": "demo/measure_voltage", "step_run_id": "r4", "parent_run_id": "r2", "depth": 1, "phase": "main", "locator": {"sequence": "medir_fuentes", "path": ["main", 1, "main", 1]}, "seq": 11},
  {"event": "step_result", "name": "demo/measure_voltage", "step_run_id": "r4", "parent_run_id": "r2", "depth": 1, "phase": "main", "locator": {"sequence": "medir_fuentes", "path": ["main", 1, "main", 1]}, "status": "pass", "seq": 12},
  {"event": "step_end", "name": "demo/measure_voltage", "step_run_id": "r4", "parent_run_id": "r2", "depth": 1, "phase": "main", "locator": {"sequence": "medir_fuentes", "path": ["main", 1, "main", 1]}, "seq": 13},
  {"event": "step_start", "name": "demo/disconnect", "step_run_id": "r5", "parent_run_id": "r2", "depth": 1, "phase": "cleanup", "locator": {"sequence": "medir_fuentes", "path": ["main", 1, "cleanup", 0]}, "seq": 14},
  {"event": "step_result", "name": "demo/disconnect", "step_run_id": "r5", "parent_run_id": "r2", "depth": 1, "phase": "cleanup", "locator": {"sequence": "medir_fuentes", "path": ["main", 1, "cleanup", 0]}, "status": "done", "seq": 15},
  {"event": "step_end", "name": "demo/disconnect", "step_run_id": "r5", "parent_run_id": "r2", "depth": 1, "phase": "cleanup", "locator": {"sequence": "medir_fuentes", "path": ["main", 1, "cleanup", 0]}, "seq": 16},
  {"event": "step_result", "name": "test_fuentes", "step_run_id": "r2", "parent_run_id": null, "depth": 0, "phase": "main", "locator": {"sequence": "basica", "path": ["main", 1]}, "status": "pass", "seq": 17},
  {"event": "step_end", "name": "test_fuentes", "step_run_id": "r2", "parent_run_id": null, "depth": 0, "phase": "main", "locator": {"sequence": "basica", "path": ["main", 1]}, "seq": 18},
  {"event": "sequence_end", "status": "pass", "seq": 19},
];

test("a real run of subsecuencia.yseq fills both rows and both stacks", () => {
  const s = newRunState();
  // The stack after every line, with repeats collapsed: what the Call Stack
  // pane would have shown, in order.
  const seen = [];
  for (const line of STREAM) {
    applyEvent(s, JSON.stringify(line));
    const stack = callStack(s).frames.map((f) => f.name).join(" > ");
    if (seen.at(-1) !== stack) seen.push(stack);
  }

  // Both calls are verdicts of the editor's own rows, and both passed.
  assert.equal(s.results.get(rowKey("main", 0)), "pass");
  assert.equal(s.results.get(rowKey("main", 1)), "pass");
  assert.equal(s.done, true);
  assert.equal(s.lost, 0, "nothing was dropped replaying the engine's own lines");

  // Every nested step found the call it ran under: none went missing, and none
  // landed on the wrong row.
  const under = nestedRows(s);
  assert.deepEqual(
    (under.get(rowKey("main", 0)) ?? []).map((f) => f.name + ":" + f.status),
    ["preparar_canal:done"],
  );
  assert.deepEqual(
    (under.get(rowKey("main", 1)) ?? []).map((f) => f.name + ":" + f.status),
    ["ajustar_canal:done", "demo/measure_voltage:pass", "demo/disconnect:done"],
  );

  // A step in the subsequence's **cleanup** is still filed under the call that
  // ran it. The phase on the line is the sub-step's own, not the caller's
  // (DIAG-3), so anything that keyed off the phase would lose this row.
  const cleanup = under.get(rowKey("main", 1)).find((f) => f.phase === "cleanup");
  assert.equal(cleanup.name, "demo/disconnect");

  // The stack, in full. Asserting the whole sequence rather than spot-checking
  // it is what pins the **unwinding**: it has to come back to the caller
  // between two nested steps, not only at the end. Checking the last line
  // alone proves nothing, because `sequence_end` clears the stack whether it
  // was unwinding correctly or not — which is how the first version of this
  // test passed against a state machine that never popped at all.
  assert.deepEqual(seen, [
    "",
    "preparar",
    "preparar > preparar_canal",
    "preparar",
    "",
    "test_fuentes",
    "test_fuentes > ajustar_canal",
    "test_fuentes",
    "test_fuentes > demo/measure_voltage",
    "test_fuentes",
    "test_fuentes > demo/disconnect",
    "test_fuentes",
    "",
  ]);
});
