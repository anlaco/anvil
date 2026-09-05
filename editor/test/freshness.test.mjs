// The guard of ADR-0031: the editor and the command line must not end up
// carrying different compilations of the engine.
//
// The failure this prevents is silent and expensive — the editor says a
// sequence is fine and the CLI refuses it, or worse, runs it differently, with
// nothing anywhere saying the two are not the same build.

import { strict as assert } from "node:assert";
import { test } from "node:test";

import { checkFreshness } from "../scripts/freshness.mjs";

const HOUR = 3600_000;
const now = Date.now();

test("a guest built after the last source change is fine", () => {
  assert.equal(checkFreshness({ guestMtime: now, sourceMtime: now - HOUR }), null);
});

test("an unknown source timestamp is refused, not waved through", () => {
  // "I could not check" must not read as "there is nothing to report"
  // (ADR-0019, Rule 2). The first version of this guard returned null here,
  // and a bug that made every check throw then passed silently for that exact
  // reason.
  const problem = checkFreshness({ guestMtime: now, sourceMtime: null });

  assert.ok(problem, "an unknown source time must be reported");
  assert.equal(problem.fatal, true);
  assert.match(problem.message, /could not|unknown/i);
});

test("a guest older than the engine's source is refused", () => {
  // The real case: someone edits the engine and the editor would transpile the
  // stale guest without a word.
  const problem = checkFreshness({ guestMtime: now - 2 * HOUR, sourceMtime: now });

  assert.ok(problem, "drift must be reported");
  assert.equal(problem.fatal, true, "it must stop the transpile, not just warn");
  assert.match(problem.message, /make release/, "it must say how to fix it");
  assert.match(problem.message, /--allow-stale/, "it must say how to override");
  assert.match(problem.message, /2h/, "it must say how far behind");
});

test("a missing release guest is refused with the command to build it", () => {
  const problem = checkFreshness({ guestMtime: null, sourceMtime: now });

  assert.ok(problem);
  assert.equal(problem.fatal, true);
  assert.match(problem.message, /make release/);
});

test("the gap is reported in units a person reads", () => {
  const at = (ms) =>
    checkFreshness({ guestMtime: now - ms, sourceMtime: now }).message;

  assert.match(at(30_000), /30s/);
  assert.match(at(10 * 60_000), /10min/);
  assert.match(at(5 * HOUR), /5h/);
  assert.match(at(72 * HOUR), /3 days/);
});
