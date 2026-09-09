// What the engine is doing, read from the NDJSON of `--events`.
//
// This is a pure state machine on purpose: it is the piece that decides which
// row lights up, and a wrong answer here points an operator at the wrong step
// while a unit is on the bench. Keeping it out of the DOM is what lets it be
// asserted line by line, with no browser.
//
// The rules it exists to honour (ADR-0029, ADR-0033):
//
// - **Nothing here concludes a verdict.** The verdict is the report and the
//   exit code, exactly as in the CLI. This only says *where* the engine is.
// - **Unknown events are skipped, not fatal** (ADR-0029 §5) — that is what lets
//   the engine add events later without breaking this.
// - **A gap in `seq` is shown, not hidden.** A reader that cannot see it would
//   conclude confidently from an incomplete stream, which is exactly what
//   Rule 2 of ADR-0019 forbids.
// - **The `locator` is a hint, not a key.** Two segments — `[phase, index]` —
//   is a row this editor owns; anything longer is running inside a subsequence,
//   which has no row here, so the honest answer is to name the call it is under
//   rather than highlight nothing or, worse, the wrong row.

/** A fresh, empty run state. */
export function newRunState() {
  return {
    /** The row being executed: `{ phase, index }`, or null. */
    current: null,
    /** `"phase:index"` → the verdict that row already produced. */
    results: new Map(),
    /** The step running inside a subsequence, by name, or null. */
    nested: null,
    /** Every step the sequence declared, from `sequence_start`. */
    plan: [],
    /** The last `seq` seen; -1 before the first line. */
    seq: -1,
    /** How many lines the stream is known to have lost. */
    lost: 0,
    /** True once `sequence_end` has arrived. */
    done: false,
  };
}

/** The key a row is addressed by, which is also the editor's own handle. */
export const rowKey = (phase, index) => `${phase}:${index}`;

/**
 * Applies one raw stderr line to the run state, in place.
 *
 * Returns true if anything changed and the view should be repainted. Lines that
 * are not JSON are ignored without comment: fd 2 is shared with the engine's own
 * logs and with the executors, so foreign text is expected, not exceptional.
 */
export function applyEvent(state, line) {
  let e;
  try {
    e = JSON.parse(line);
  } catch {
    return false;
  }
  if (!e || typeof e.event !== "string") return false;

  if (typeof e.seq === "number") {
    if (state.seq >= 0 && e.seq > state.seq + 1) {
      state.lost += e.seq - state.seq - 1;
    }
    state.seq = e.seq;
  }

  const path = e.locator?.path;
  const mine = Array.isArray(path) && path.length === 2;
  const phase = mine ? path[0] : null;
  const index = mine ? path[1] : null;

  switch (e.event) {
    case "sequence_start":
      state.plan = Array.isArray(e.plan) ? e.plan : [];
      return true;

    case "step_start":
      if (mine) {
        state.current = { phase, index };
        state.nested = null;
      } else {
        // Leave the ancestor row lit and say what is running under it.
        state.nested = typeof e.name === "string" ? e.name : null;
      }
      return true;

    case "step_result":
      if (mine && typeof e.status === "string") {
        state.results.set(rowKey(phase, index), e.status);
        return true;
      }
      return false;

    case "step_end":
      if (mine) {
        state.current = null;
        state.nested = null;
        return true;
      }
      return false;

    case "sequence_end":
      state.current = null;
      state.nested = null;
      state.done = true;
      return true;

    default:
      // An event this build does not know. Skip the line and keep going.
      return false;
  }
}

/**
 * Whether Run may be offered, and what to say when it may not.
 *
 * A pure function because the bug it guards was exactly one missing term: the
 * button was computed from the bridge alone, so **any repaint during a run** —
 * inserting a step, saving, typing — re-armed it and offered a second run over
 * an engine that takes one sequence at a time.
 *
 * The title is part of the answer, not decoration: a control that is refused
 * without saying why reads as broken.
 */
export function runButton({ hasDoc, bridged, inFlight }) {
  if (inFlight) {
    return {
      disabled: true,
      title: "A run is in flight; the engine takes one sequence at a time",
    };
  }
  if (!bridged) {
    return {
      disabled: true,
      title:
        "Run needs a bridge: start `anvil <sequence.yaml> --bridge` and open the URL it prints",
    };
  }
  return { disabled: !hasDoc, title: "Run this sequence" };
}

/**
 * The steps the sequence declared that produced no lines at all.
 *
 * Only answerable while `lost` is 0: a step with no lines and no gap in the
 * stream **did not run**; with a gap, its lines may simply be missing. Saying
 * which of the two it is — rather than guessing — is the whole reason
 * `sequence_start` carries a plan (ADR-0033 §4b).
 */
export function neverRan(state) {
  if (state.lost > 0) return null;
  return state.plan.filter((p) => !state.results.has(rowKey(p.phase, p.index)));
}
