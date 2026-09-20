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
//   so the highlight stays on the call and the step is listed underneath it.
// - **Paternity is read off each line, never reconstructed with a push/pop
//   stack** (ADR-0033 §2). Frames are stored by `step_run_id` and the call
//   stack is walked from the innermost one through `parent_run_id`, so a lost
//   line costs one node rather than the whole tree — and the walk says when it
//   ran out of parents instead of pretending it reached the root.

/** A fresh, empty run state. */
export function newRunState() {
  return {
    /** The row being executed: `{ phase, index }`, or null. */
    current: null,
    /** `"phase:index"` → the verdict that row already produced. */
    results: new Map(),
    /** The step running inside a subsequence, by name, or null. */
    nested: null,
    /**
     * Every step this run has started, by `step_run_id`.
     *
     * `{ id, parent, depth, name, sequence, phase, row, status }` — `row` is
     * the `phase:index` of the editor's own row this step happened under, or
     * null when the chain to it was broken by a lost line.
     */
    frames: new Map(),
    /** The innermost step that has started and not ended, or null. */
    innermost: null,
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
      remember(state, e, mine ? rowKey(phase, index) : null);
      if (mine) {
        state.current = { phase, index };
        state.nested = null;
      } else {
        // Leave the ancestor row lit and say what is running under it. The
        // step also gets a row of its own now, under that call.
        state.nested = typeof e.name === "string" ? e.name : null;
      }
      return true;

    case "step_result": {
      const frame = state.frames.get(e.step_run_id);
      if (frame && typeof e.status === "string") frame.status = e.status;
      if (mine && typeof e.status === "string") {
        state.results.set(rowKey(phase, index), e.status);
        return true;
      }
      // A verdict inside a subsequence moves no row of its own, but it is the
      // only thing the nested row under the call has to show.
      return Boolean(frame);
    }

    case "step_end":
      if (state.innermost === e.step_run_id) {
        state.innermost = state.frames.get(e.step_run_id)?.parent ?? null;
      }
      if (mine) {
        state.current = null;
        state.nested = null;
        return true;
      }
      return Boolean(state.frames.get(e.step_run_id));

    case "sequence_end":
      state.current = null;
      state.nested = null;
      state.innermost = null;
      state.done = true;
      return true;

    default:
      // An event this build does not know. Skip the line and keep going.
      return false;
  }
}

/**
 * Files a `step_start` as a frame, and makes it the innermost one.
 *
 * `row` is the editor's own row when this step *is* one; otherwise the step
 * runs inside a subsequence and inherits the row of the nearest ancestor that
 * has one. Inheriting at write time rather than walking at read time is what
 * keeps a lost line cheap: the chain is only as long as it was when the line
 * arrived, and a frame whose parent never came through says `row: null`
 * instead of silently attaching itself to the wrong call.
 */
function remember(state, e, row) {
  const id = e.step_run_id;
  if (typeof id !== "string") return;
  const parent = typeof e.parent_run_id === "string" ? e.parent_run_id : null;
  const inherited = parent ? (state.frames.get(parent)?.row ?? null) : null;
  state.frames.set(id, {
    id,
    parent,
    depth: typeof e.depth === "number" ? e.depth : 0,
    name: typeof e.name === "string" ? e.name : "(unnamed)",
    sequence: e.locator?.sequence ?? null,
    phase: typeof e.phase === "string" ? e.phase : null,
    row: row ?? inherited,
    status: null,
  });
  state.innermost = id;
}

/**
 * The call stack, outermost first: what is running, and what it is running
 * under. TestStand's Call Stack pane.
 *
 * Walked from the innermost frame through `parent_run_id`, never from a
 * push/pop stack (ADR-0033 §2). `truncated` is true when the walk ran out of
 * parents before reaching a root — a line was lost — and saying so is the
 * point: a stack that quietly starts halfway up reads as the whole truth.
 */
export function callStack(state) {
  const frames = [];
  let seen = 0;
  let id = state.innermost;
  let truncated = false;
  while (id) {
    const frame = state.frames.get(id);
    if (!frame) {
      truncated = true;
      break;
    }
    frames.unshift(frame);
    id = frame.parent;
    // A parent chain that points at itself would hang the interface, and the
    // ids come off a wire this code does not control.
    if (++seen > 128) {
      truncated = true;
      break;
    }
  }
  return { frames, truncated };
}

/**
 * The steps that ran inside a subsequence, grouped by the row of the call they
 * happened under.
 *
 * This is what stops the step list being flat. A `sequence_call` used to stay
 * lit with the status bar naming whatever was running beneath it, and nothing
 * on screen said what had already passed or failed down there.
 */
export function nestedRows(state) {
  const under = new Map();
  for (const frame of state.frames.values()) {
    // depth 0 is a row of the sequence being edited; it is not nested.
    if (frame.depth === 0 || !frame.row) continue;
    if (!under.has(frame.row)) under.set(frame.row, []);
    under.get(frame.row).push(frame);
  }
  return under;
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
export function runButton({ hasDoc, bridged, inFlight, unavailable = null }) {
  if (inFlight) {
    return {
      disabled: true,
      title: "A run is in flight; the engine takes one sequence at a time",
    };
  }
  // The desktop app starts the bridge itself, so "start `anvil --bridge`" is
  // the wrong advice there; when it tried and could not, the reason it gives
  // (no engine found, not the engine) is what the person can act on.
  if (!bridged && unavailable) {
    return { disabled: true, title: `Run is unavailable: ${unavailable}` };
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
