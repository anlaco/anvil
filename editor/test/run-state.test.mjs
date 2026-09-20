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

import { applyEvent, neverRan, newRunState, rowKey, runButton } from "../src/run-state.mjs";

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
