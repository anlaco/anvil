// The sequence document: one YAML tree, two views.
//
// The text is the source of truth and the step view is an editable projection
// of it — not a parallel model that gets serialised back. Editing a field
// mutates the corresponding node and the text is re-emitted, so comments, key
// order and formatting survive (AP-05). This matters because Anvil's sequences
// are hand-written files that live in git and carry comments worth as much as
// the content: `ejemplos/basica.yaml` opens with nine lines explaining where its
// threshold comes from and which ADR decided it. An editor that reformats that
// on save is an editor people stop using.
//
// It also answers what the step view shows while the text is mid-edit and does
// not parse (AP-12): the last good tree, marked stale. Freezing silently would
// be a lie and blanking would be unusable.
//
// The vocabulary is the loader's, never invented here (AP-04): every key below
// is one `crates/cargador/src/lib.rs` accepts, and the loader rejects anything
// else with `deny_unknown_fields`.

import { parseDocument, isMap, isSeq } from "yaml";

/** The three phases, in the order the engine runs them. */
export const PHASES = ["setup", "main", "cleanup"];

/** The variable scopes a sequence can declare, as the loader names them. */
export const SCOPES = ["locals", "parameters", "file_globals"];

/**
 * The step types the engine understands, TestStand's (ADR-0040). A step must
 * say which: there is no default.
 */
export const STEP_TYPES = ["action", "pass_fail", "numeric_limit", "statement", "sequence_call"];

/** The types that call an executor when they are inserted. */
const CALLS_AN_EXECUTOR = new Set(["action", "numeric_limit"]);

/**
 * A sequence document.
 *
 * Holds the parsed tree and the text it came from. When the text stops parsing
 * the tree is kept as it was and `stale` goes true: callers show the old
 * structure marked as not reflecting the text, rather than showing nothing.
 */
export class SequenceDocument {
  #doc;
  #text;
  #stale = false;
  #error = null;

  constructor(text) {
    this.#text = text;
    this.#doc = parseDocument(text);
    this.#error = firstError(this.#doc);
  }

  /** The current text. This is what gets written to disk. */
  get text() {
    return this.#text;
  }

  /** True when the text no longer parses and the tree is the last good one. */
  get stale() {
    return this.#stale;
  }

  /**
   * The parse error, or null. Carries `message` and, when YAML reports one,
   * `line`/`col` so the text view can point at it.
   */
  get error() {
    return this.#error;
  }

  /**
   * Replaces the text — what the text view calls on every keystroke.
   *
   * If it parses, the tree is replaced and staleness clears. If it does not,
   * the text is kept as typed (people must see what they wrote) while the tree
   * stays at its last good state and is marked stale.
   */
  setText(text) {
    this.#text = text;
    const next = parseDocument(text);
    const error = firstError(next);
    if (error) {
      this.#stale = true;
      this.#error = error;
      return;
    }
    this.#doc = next;
    this.#stale = false;
    this.#error = null;
  }

  /** The sequence's `name`, or null if it has none. */
  get name() {
    return this.#doc.get("name") ?? null;
  }

  /**
   * The steps of one phase, as plain objects for rendering.
   *
   * Flat, with no nesting: the engine has no nested control flow today — only
   * `precondition`, `condition`, `statement` and a subsequence call — so the
   * editor offers none either (AP-03, AP-04).
   */
  steps(phase) {
    assertPhase(phase);
    const seq = this.#doc.get(phase);
    if (!isSeq(seq)) return [];
    return seq.items.map((item, index) => ({
      index,
      name: item.get?.("name") ?? null,
      type: item.get?.("type") ?? null,
      retries: item.get?.("retries") ?? 1,
      disable: item.get?.("disable") ?? false,
      executor: item.get?.("executor") ?? null,
      module: item.get?.("module") ?? null,
      limit: readLimit(item),
    }));
  }

  /** Every step in the document, phase by phase, in execution order. */
  allSteps() {
    return PHASES.flatMap((phase) =>
      this.steps(phase).map((step) => ({ ...step, phase })),
    );
  }

  /** The variables declared in one scope, as a plain object. */
  variables(scope) {
    if (!SCOPES.includes(scope)) {
      throw new Error(`unknown scope '${scope}': expected one of ${SCOPES.join(", ")}`);
    }
    const map = this.#doc.get(scope);
    return isMap(map) ? map.toJSON() : {};
  }

  /**
   * Sets one field on one step, in place.
   *
   * `set` on a node the parser produced keeps the surrounding formatting and
   * comments; the text is re-emitted from the same tree. Passing `undefined`
   * removes the key, which is how an optional field goes back to its default
   * rather than being written out as null.
   */
  setStepField(phase, index, key, value) {
    const step = this.#stepNode(phase, index);
    if (value === undefined) step.delete(key);
    else step.set(key, value);
    this.#reemit();
  }

  /**
   * Sets one bound of a step's `limit`.
   *
   * Split out from `setStepField` because `limit` is a nested map with its own
   * coherence rules — each comparison code uses its own fields and refuses the
   * rest (ADR-0040 §7) — and because changing a threshold is
   * the single most common edit anyone makes to a sequence.
   */
  setStepLimit(phase, index, key, value) {
    const step = this.#stepNode(phase, index);
    const limit = step.get("limit", true);
    if (!isMap(limit)) {
      throw new Error(
        `step ${index} of '${phase}' has no 'limit' to set '${key}' on`,
      );
    }
    limit.set(key, value);
    this.#reemit();
  }

  /**
   * Appends a step of the given type to a phase, and returns its index.
   *
   * The step is created with the fields its type *requires*, never fewer: the
   * loader's coherence rules make `statement` mandatory on a statement step,
   * `condition` on a pass_fail and `sequence` on a sequence_call
   * (`crates/cargador/src/lib.rs:2297-2328`), and a step missing them is
   * rejected at load. Inserting a step that makes the file invalid would break
   * the rule that the editor cannot build what the loader refuses (AP-04), so
   * the placeholders are part of the insert, not something to fill in later.
   */
  addStep(phase, type = "action") {
    assertPhase(phase);
    if (!STEP_TYPES.includes(type)) {
      throw new Error(`unknown step type '${type}'`);
    }
    const why = this.cannotAdd(type);
    if (why) throw new Error(why);

    let seq = this.#doc.get(phase);
    if (!isSeq(seq)) {
      this.#doc.set(phase, []);
      seq = this.#doc.get(phase);
    }

    const step = { name: uniqueName(this, type), type };
    // A step that calls something names a module and its executor: the first
    // declared one, which the person changes in the step's settings. The
    // module is a placeholder the catalog check will name as unknown until it
    // is set — not something that runs quietly.
    if (CALLS_AN_EXECUTOR.has(type)) {
      step.module = step.name;
      step.executor = this.executorNames()[0];
    }
    // `none` records the value and judges nothing (ADR-0040 §8): a new limit
    // that reads `done` until its comparison is set, never a pass.
    if (type === "numeric_limit") step.limit = { comparison: "none" };

    if (type === "statement") {
      // A statement must assign to a declared variable, or the loader rejects
      // the sequence (`validar_lvalues`, crates/cargador/src/lib.rs:1046). So
      // inserting one declares its target when there is none to write to —
      // found by inserting a statement into `basica.yaml`, which declares no
      // `locals` at all, and watching the engine refuse the result.
      const target = Object.keys(this.variables("locals"))[0] ?? "ok";
      if (!(target in this.variables("locals"))) {
        const locals = this.#doc.get("locals");
        if (isMap(locals)) locals.set(target, false);
        else this.#doc.set("locals", { [target]: false });
      }
      step.statement = `locals.${target} = true`;
    }

    if (type === "pass_fail") step.condition = "true";

    if (type === "sequence_call") {
      // A call must match the subsequence's signature, parameter for parameter
      // — the loader checks it and rejects a call that is missing any
      // (`el sequence call ... no encaja con la firma de ...`). And `args` may
      // only name a local (crates/cargador/src/lib.rs:2259-2288). So inserting
      // one wires every parameter to a local, declaring the ones that do not
      // exist with the subsequence's own default as their initial value.
      const name = this.subsequenceNames()[0];
      step.sequence = name;
      const params = this.subsequenceParameters(name);
      if (Object.keys(params).length > 0) {
        step.args = {};
        for (const [param, initial] of Object.entries(params)) {
          const local = `${name}_${param}`;
          if (!(local in this.variables("locals"))) {
            const locals = this.#doc.get("locals");
            if (isMap(locals)) locals.set(local, initial);
            else this.#doc.set("locals", { [local]: initial });
          }
          step.args[param] = `locals.${local}`;
        }
      }
    }

    // `createNode` makes a real YAML node. Adding the plain object works for
    // emitting but leaves an item with no `get`, so the step view reads it back
    // as unnamed until the document is re-parsed.
    seq.add(this.#doc.createNode(step));
    this.#reemit();
    return seq.items.length - 1;
  }

  /** The subsequences this file declares, by name. */
  subsequenceNames() {
    const subs = this.#doc.get("subsequences");
    return isMap(subs) ? subs.items.map((i) => String(i.key)) : [];
  }

  /**
   * The `parameters` of one inline subsequence, with their declared initial
   * values — which is the signature a call has to match.
   */
  subsequenceParameters(name) {
    const subs = this.#doc.get("subsequences");
    if (!isMap(subs)) return {};
    const sub = subs.get(name);
    const params = sub?.get?.("parameters");
    return isMap(params) ? params.toJSON() : {};
  }

  /**
   * Why a step of this type cannot be inserted right now, or null if it can.
   *
   * The editor must not be able to build a sequence the loader refuses (AP-04),
   * and a `sequence_call` needs a subsequence to call: `sequence` is mandatory
   * for that type (crates/cargador/src/lib.rs:2323-2328) and naming one that
   * does not exist fails to load. So the palette refuses, and says why, rather
   * than inserting something broken.
   */
  cannotAdd(type) {
    if (type === "sequence_call" && this.subsequenceNames().length === 0) {
      return "this sequence declares no subsequences to call";
    }
    // A step that calls an executor must name it, and there is none built into
    // anvil to fall back on (ADR-0041): with no `executors:` there is nothing to
    // name.
    if (CALLS_AN_EXECUTOR.has(type) && this.executorNames().length === 0) {
      return "this sequence declares no executors to call";
    }
    return null;
  }

  /** The executors this file declares under `executors:`, by name. */
  executorNames() {
    const list = this.#doc.get("executors");
    if (!isSeq(list)) return [];
    return list.items.map((e) => e?.get?.("name")).filter((n) => typeof n === "string");
  }

  /** Removes a step from a phase. */
  removeStep(phase, index) {
    assertPhase(phase);
    const seq = this.#doc.get(phase);
    if (!isSeq(seq) || !seq.items[index]) {
      throw new Error(`no step at index ${index} of '${phase}'`);
    }
    seq.delete(index);
    this.#reemit();
  }

  /**
   * Moves a step within its phase by `delta` positions, and returns where it
   * ended up. Order is execution order, so this is a real edit, not a view
   * preference.
   */
  moveStep(phase, index, delta) {
    assertPhase(phase);
    const seq = this.#doc.get(phase);
    if (!isSeq(seq) || !seq.items[index]) {
      throw new Error(`no step at index ${index} of '${phase}'`);
    }
    const to = Math.max(0, Math.min(seq.items.length - 1, index + delta));
    if (to === index) return index;
    const [node] = seq.items.splice(index, 1);
    seq.items.splice(to, 0, node);
    this.#reemit();
    return to;
  }

  #stepNode(phase, index) {
    assertPhase(phase);
    const seq = this.#doc.get(phase);
    if (!isSeq(seq) || !seq.items[index]) {
      throw new Error(`no step at index ${index} of '${phase}'`);
    }
    return seq.items[index];
  }

  // Re-emitting the whole document is what keeps comments and key order: the
  // nodes the parser built carry their own formatting, so only the value that
  // changed is written differently.
  //
  // The trailing newline is restored to whatever the file had. The emitter
  // always ends with one, and `ejemplos/basica.yaml` ends without — so saving
  // would append a line nobody asked to change, in a file reviewed as a diff.
  // Whether a file should end in a newline is not this editor's opinion to
  // impose on someone else's file.
  //
  // Nor which line break it uses. The emitter writes `\n`; a file saved on
  // Windows is CRLF, and re-emitting it in LF made a one-value edit a diff of
  // every line. The file's first line break decides, and the whole text is
  // written back with it.
  #reemit() {
    const crlf = /^[^\n]*\r\n/.test(this.#text);
    const hadNewline = /\n$/.test(this.#text);
    let emitted = String(this.#doc).replace(/\r\n/g, "\n");
    if (!hadNewline) emitted = emitted.replace(/\n$/, "");
    this.#text = crlf ? emitted.replace(/\n/g, "\r\n") : emitted;
    this.#stale = false;
    this.#error = null;
  }
}

// Step names are how a sequence refers to a step and how the report names it,
// so a new one must not silently collide with an existing one.
function uniqueName(doc, type) {
  const taken = new Set(doc.allSteps().map((s) => s.name));
  const base = `new_${type}`;
  if (!taken.has(base)) return base;
  for (let n = 2; ; n++) {
    if (!taken.has(`${base}_${n}`)) return `${base}_${n}`;
  }
}

function assertPhase(phase) {
  if (!PHASES.includes(phase)) {
    throw new Error(`unknown phase '${phase}': expected one of ${PHASES.join(", ")}`);
  }
}

function readLimit(item) {
  const limit = item.get?.("limit");
  if (!limit) return null;
  const plain = typeof limit.toJSON === "function" ? limit.toJSON() : limit;
  return plain && typeof plain === "object" ? plain : null;
}

// A document with errors still has a tree, but a partial one; the first error is
// what the text view points at, and the rest usually cascade from it.
function firstError(doc) {
  const e = doc.errors?.[0];
  if (!e) return null;
  return {
    message: e.message,
    line: e.linePos?.[0]?.line ?? null,
    col: e.linePos?.[0]?.col ?? null,
  };
}
