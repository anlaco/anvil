// The sequence document: one YAML tree, two views.
//
// The text is the source of truth and the step view is an editable projection
// of it — not a parallel model that gets serialised back. Editing a field
// mutates the corresponding node and the text is re-emitted, so comments, key
// order and formatting survive (AP-05). This matters because Anvil's sequences
// are hand-written files that live in git and carry comments worth as much as
// the content: `ejemplos/basica.yseq` opens with nine lines explaining where its
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
 * Which fields each step type may carry (ADR-0040). The loader refuses the
 * rest: a `limit` on an `action`, a `module` on a `statement`, a `condition`
 * on a `numeric_limit`. `setStepType` drops whatever is not on the new type's
 * list, so changing a type cannot leave the previous one's fields behind.
 *
 * `inputs` and `assign` go with `module`: they are what a step sends to its
 * executor and what it keeps from the answer, so a step that calls nothing has
 * neither.
 */
const TYPE_FIELDS = {
  action: ["module", "executor", "inputs", "assign"],
  pass_fail: ["module", "executor", "inputs", "assign", "condition"],
  numeric_limit: ["module", "executor", "inputs", "assign", "limit", "value"],
  statement: ["statement"],
  sequence_call: ["sequence", "args"],
};

/** Every field that belongs to some type and not to others. */
const TYPE_OWNED = [...new Set(Object.values(TYPE_FIELDS).flat())];

/**
 * Which fields each comparison code uses, and with what value when there is
 * nothing to carry over. Mirrored from the loader, which refuses a code that
 * carries a field it does not use and one that is missing a field it does
 * (`a_limite_teststand`, crates/cargador/src/lib.rs) — so this table is the
 * editor's half of ADR-0040 §7 and has no opinions of its own.
 *
 * The defaults exist because a limit is written the moment the code is chosen:
 * a `GELE` with no bounds does not load, and asking someone to fix an invalid
 * file the editor just wrote is the thing AP-04 forbids.
 */
export const COMPARISONS = {
  EQ: { low: 0 },
  NE: { low: 0 },
  GT: { low: 0 },
  LT: { low: 0 },
  GE: { low: 0 },
  LE: { low: 0 },
  GTLT: { low: 0, high: 1 },
  GELE: { low: 0, high: 1 },
  GELT: { low: 0, high: 1 },
  GTLE: { low: 0, high: 1 },
  LTGT: { low: 0, high: 1 },
  LEGE: { low: 0, high: 1 },
  LEGT: { low: 0, high: 1 },
  LTGE: { low: 0, high: 1 },
  EQT: { nominal: 0, lower: 1, upper: 1, threshold: "percent" },
  none: {},
};

/** The comparison codes, in the order the loader lists them in its error. */
export const COMPARISON_CODES = Object.keys(COMPARISONS);

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
      assign: readMap(item, "assign"),
      condition: item.get?.("condition") ?? null,
      statement: item.get?.("statement") ?? null,
      value: item.get?.("value") ?? null,
      precondition: item.get?.("precondition") ?? null,
      pause_on_fail: item.get?.("pause_on_fail") ?? false,
      comment: item.get?.("comment") ?? null,
      sequence: item.get?.("sequence") ?? null,
      // Read-only for now: the Module tab shows the parameter table TestStand
      // puts there, and cannot yet write it. Reading it means a step that has
      // inputs shows them rather than looking like a step that has none.
      inputs: readMap(item, "inputs"),
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
    const map = this.#doc.get(scope, true);
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
    // A step that names a module must name the executor that serves it, and
    // there is none built into anvil to fall back on (ADR-0041). Giving a
    // `pass_fail` a module and stopping there writes a file the loader refuses,
    // which the editor must not be able to do (AP-04) — so the first declared
    // executor is filled in, and the person changes it in the field right
    // below.
    if (key === "module" && value !== undefined && !step.get("executor")) {
      const first = this.executorNames()[0];
      if (first) step.set("executor", first);
    }
    // And the other way round: a module removed leaves an `executor` on a step
    // that no longer calls anything, which is a load error of its own.
    if (key === "module" && value === undefined) step.delete("executor");
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
  /**
   * Changes a step's type, and makes the rest of the step agree with it.
   *
   * A type decides which other fields a step may carry (ADR-0040): an `action`
   * judges nothing, so a `limit` on one is a load error; a `statement` calls
   * nothing, so a `module` on one is too. Setting `type` on its own therefore
   * left the previous type's fields behind and the sequence stopped loading —
   * one click from the type menu, which is exactly what the editor must not be
   * able to do (AP-04).
   *
   * So the fields the new type refuses go, and the ones it requires arrive with
   * the same placeholders `addStep` gives a fresh step of that type.
   */
  setStepType(phase, index, type) {
    if (!STEP_TYPES.includes(type)) {
      throw new Error(`unknown step type '${type}'`);
    }
    const step = this.#stepNode(phase, index);
    if (step.get("type") === type) return;
    step.set("type", type);

    for (const key of TYPE_OWNED) {
      if (!TYPE_FIELDS[type].includes(key)) step.delete(key);
    }

    const name = step.get("name") ?? uniqueName(this, type);
    for (const [key, value] of Object.entries(this.#requiredFor(type, name))) {
      if (step.get(key) === undefined) step.set(key, value);
    }

    this.#reemit();
  }

  /**
   * The fields a step of this type cannot be without, with the placeholders a
   * new one gets. Shared by `addStep` and `setStepType` so that a step reached
   * either way is the same step.
   */
  #requiredFor(type, name) {
    const step = {};
    if (CALLS_AN_EXECUTOR.has(type)) {
      step.module = name;
      step.executor = this.executorNames()[0];
    }
    // `none` records the value and judges nothing (ADR-0040 §8): a new limit
    // that reads `done` until its comparison is set, never a pass.
    if (type === "numeric_limit") step.limit = { comparison: "none" };
    if (type === "pass_fail") step.condition = "true";
    if (type === "statement") step.statement = `locals.${this.#aLocal()} = true`;
    if (type === "sequence_call") {
      const target = this.subsequenceNames()[0];
      step.sequence = target;
      const args = this.#argsFor(target);
      if (args) step.args = args;
    }
    return step;
  }

  /**
   * A local to write to, declaring one when the sequence has none: a statement
   * must assign to a declared variable or the loader rejects the sequence
   * (`validar_lvalues`, crates/cargador/src/lib.rs).
   */
  #aLocal() {
    const existing = Object.keys(this.variables("locals"))[0];
    if (existing) return existing;
    const locals = this.#doc.get("locals");
    if (isMap(locals)) locals.set("ok", false);
    else this.#doc.set("locals", { ok: false });
    return "ok";
  }

  /**
   * Every parameter of `target` wired to a local, declaring the ones that do
   * not exist. A call must match the subsequence's signature parameter for
   * parameter, and `args` may only name a local.
   */
  #argsFor(target) {
    const params = this.subsequenceParameters(target);
    if (Object.keys(params).length === 0) return null;
    const args = {};
    for (const [param, initial] of Object.entries(params)) {
      const local = `${target}_${param}`;
      if (!(local in this.variables("locals"))) {
        const locals = this.#doc.get("locals");
        if (isMap(locals)) locals.set(local, initial);
        else this.#doc.set("locals", { [local]: initial });
      }
      args[param] = `locals.${local}`;
    }
    return args;
  }

  addStep(phase, type = "action", at = null) {
    assertPhase(phase);
    if (!STEP_TYPES.includes(type)) {
      throw new Error(`unknown step type '${type}'`);
    }
    const why = this.cannotAdd(type);
    if (why) throw new Error(why);

    let seq = this.#doc.get(phase, true);
    if (!isSeq(seq)) {
      this.#doc.set(phase, this.#doc.createNode([]));
      seq = this.#doc.get(phase, true);
    }

    // The fields its type requires, never fewer: the loader's coherence rules
    // make `statement` mandatory on a statement step, `condition` on a
    // pass_fail and `sequence` on a sequence_call, and a step missing them is
    // rejected at load. The placeholders are part of the insert, not something
    // to fill in later (AP-04). `setStepType` fills the same ones, so a step
    // reached either way is the same step.
    const name = uniqueName(this, type);
    const step = { name, type, ...this.#requiredFor(type, name) };

    // `createNode` makes a real YAML node. Adding the plain object works for
    // emitting but leaves an item with no `get`, so the step view reads it back
    // as unnamed until the document is re-parsed.
    // Where it lands is part of the edit: a phase runs in order, so "at the
    // end" is a decision about execution, not about the view. `at` is where the
    // person dropped it; clamped, because a drop past the last row means last.
    const node = this.#doc.createNode(step);
    // `main: []` — what File ▸ New starts from, because an empty sequence
    // cannot be written in block style — is a flow list, and everything added
    // to it inherits that: `[ { name: …, type: … } ]`. Valid YAML, and unlike
    // every sequence in the repo. These files are reviewed as diffs (AP-05), so
    // the first step added is where the list becomes a normal block one.
    seq.flow = false;
    node.flow = false;
    const index =
      at === null ? seq.items.length : Math.max(0, Math.min(seq.items.length, at));
    seq.items.splice(index, 0, node);
    this.#reemit();
    return index;
  }

  /**
   * Sets the sequence's `name`. A file with no name does not load.
   */
  setName(name) {
    this.#doc.set("name", name);
    this.#reemit();
  }

  /**
   * Declares an executor. Without one, no step can call anything (ADR-0041),
   * so this is the first thing a sequence built from nothing needs.
   *
   * One kind since ADR-0046, and it is an address: how the thing at that
   * address was started is not the sequence's business. What it takes to
   * bring one up on this machine goes in `dev:`, which the engine ignores.
   */
  addExecutor(name, host, port) {
    const entry = { name, type: "grpc", host, port };
    let list = this.#doc.get("executors", true);
    if (!isSeq(list)) {
      this.#doc.set("executors", this.#doc.createNode([]));
      list = this.#doc.get("executors", true);
    }
    list.add(this.#doc.createNode(entry));
    this.#reemit();
  }

  /**
   * Moves a step to a position, in this phase or another one.
   *
   * Crossing phases is a real change of meaning — a step that moves from `main`
   * to `cleanup` now runs even when the sequence failed — so it is an edit of
   * the file like any other, not a view arrangement.
   */
  moveStepTo(fromPhase, fromIndex, toPhase, toIndex) {
    assertPhase(fromPhase);
    assertPhase(toPhase);
    const from = this.#doc.get(fromPhase);
    if (!isSeq(from) || !from.items[fromIndex]) {
      throw new Error(`no step at index ${fromIndex} of '${fromPhase}'`);
    }
    const [node] = from.items.splice(fromIndex, 1);

    let to = this.#doc.get(toPhase, true);
    if (!isSeq(to)) {
      this.#doc.set(toPhase, this.#doc.createNode([]));
      to = this.#doc.get(toPhase, true);
    }
    const at = Math.max(0, Math.min(to.items.length, toIndex));
    to.items.splice(at, 0, node);
    this.#reemit();
    return at;
  }

  /**
   * Gives a step's limit a comparison code, rewriting the limit to exactly the
   * fields that code uses.
   *
   * Bounds that both codes use are carried over: going from `GELE` to `GELT`
   * changes whether the upper bound is included, and throwing away thresholds
   * someone measured to find would be its own kind of damage. Fields the new
   * code does not use are dropped, because the loader refuses them — the editor
   * must not be able to write a file the engine rejects (AP-04).
   */
  setStepComparison(phase, index, code) {
    if (!(code in COMPARISONS)) {
      throw new Error(`unknown comparison '${code}'`);
    }
    const step = this.#stepNode(phase, index);
    const previous = readLimit(step) ?? {};
    const next = { comparison: code };
    for (const [field, fallback] of Object.entries(COMPARISONS[code])) {
      next[field] = field in previous ? previous[field] : fallback;
    }
    // `units` is report-only and every code accepts it, so it survives.
    if ("units" in previous) next.units = previous.units;
    step.set("limit", this.#doc.createNode(next));
    this.#reemit();
  }

  /**
   * Declares a variable, or changes what it starts as.
   *
   * The scalar decides the type (`true` → bool, `4.5` → number, the rest text),
   * which is the loader's rule, not one invented here.
   */
  setVariable(scope, name, value) {
    assertScope(scope);
    let map = this.#doc.get(scope, true);
    if (!isMap(map)) {
      // A plain `{}` is stored as it is and has no `set`, the same trap the
      // insert path documents for sequences: it must be a node.
      this.#doc.set(scope, this.#doc.createNode({}));
      map = this.#doc.get(scope, true);
    }
    map.set(name, value);
    this.#reemit();
  }

  /** Renames a variable, keeping its position and its value. */
  renameVariable(scope, from, to) {
    assertScope(scope);
    const map = this.#doc.get(scope, true);
    if (!isMap(map)) throw new Error(`this sequence declares no '${scope}'`);
    const item = map.items.find((i) => String(i.key) === from);
    if (!item) throw new Error(`no variable '${from}' in '${scope}'`);
    // Renaming the key in place keeps the declaration where it was in the file,
    // so the diff is the one line the person changed.
    item.key = this.#doc.createNode(to);
    this.#reemit();
  }

  /** Removes a variable. */
  removeVariable(scope, name) {
    assertScope(scope);
    const map = this.#doc.get(scope, true);
    if (!isMap(map)) throw new Error(`this sequence declares no '${scope}'`);
    map.delete(name);
    this.#reemit();
  }

  /**
   * Keeps one field of what a step returned in a variable (`assign`).
   *
   * This is how a measurement outlives its step: `result.*` is only readable
   * where the step answered, so anything a later step needs has to be dumped
   * here first. Passing `undefined` removes the entry, and the last one removes
   * `assign` itself rather than leaving an empty map the loader has to accept.
   */
  setStepAssign(phase, index, target, expression) {
    const step = this.#stepNode(phase, index);
    let assign = step.get("assign", true);
    if (!isMap(assign)) {
      if (expression === undefined) return;
      step.set("assign", this.#doc.createNode({}));
      assign = step.get("assign", true);
    }
    if (expression === undefined) {
      assign.delete(target);
      if (assign.items.length === 0) step.delete("assign");
    } else {
      assign.set(target, expression);
    }
    this.#reemit();
  }

  /**
   * One of a step's `inputs`: what it sends to its module (ADR-0020).
   *
   * Same shape as `setStepAssign`, and for the same reasons: `undefined`
   * removes the entry, and the last one removes `inputs` itself rather than
   * leaving an empty map behind in a file that gets read in diffs (AP-05).
   *
   * The value is written **with its type**, because the loader reads the
   * scalar: `4.5` is a number, `true` a boolean, the rest text (RF-31). An
   * expression is text that happens to say `${…}`, and the engine evaluates
   * it — the editor does not have to know which it is.
   */
  setStepInput(phase, index, name, value) {
    const step = this.#stepNode(phase, index);
    let inputs = step.get("inputs", true);
    if (!isMap(inputs)) {
      if (value === undefined) return;
      step.set("inputs", this.#doc.createNode({}));
      inputs = step.get("inputs", true);
    }
    if (value === undefined) {
      inputs.delete(name);
      if (inputs.items.length === 0) step.delete("inputs");
    } else {
      inputs.set(name, value);
    }
    this.#reemit();
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

  /**
   * The executors this file declares: a name and where it is listening.
   *
   * One kind, and it is an address (ADR-0046). There is no default: a step
   * that calls one names it (ADR-0041).
   */
  executors() {
    const list = this.#doc.get("executors", true);
    if (!isSeq(list)) return [];
    return list.items.map((e) => ({
      name: e?.get?.("name") ?? null,
      type: e?.get?.("type") ?? null,
      host: e?.get?.("host") ?? null,
      port: e?.get?.("port") ?? null,
    }));
  }

  /**
   * The `dev:` entry for an executor, or null — how a front end may bring it
   * up here (ADR-0046). The engine never reads this, and a production
   * sequence does not carry it.
   */
  devFor(name) {
    const dev = this.#doc.get("dev", true);
    const entry = isMap(dev) ? dev.get(name, true) : null;
    if (!isMap(entry)) return null;
    return {
      runtime: entry.get("runtime") ?? null,
      code: entry.get("code") ?? null,
    };
  }

  /** Sets one field of a declared executor. */
  setExecutorField(name, key, value) {
    const node = this.#executorNode(name);
    if (value === undefined) node.delete(key);
    else node.set(key, value);
    this.#reemit();
  }

  /**
   * Removes an executor. The steps that named it are left alone: they become a
   * load error that names them, which is the truth — deleting someone's steps
   * because their executor went away would be a far worse surprise.
   */
  removeExecutor(name) {
    const list = this.#doc.get("executors", true);
    if (!isSeq(list)) throw new Error("this sequence declares no executors");
    const index = list.items.findIndex((e) => e?.get?.("name") === name);
    if (index === -1) throw new Error(`no executor '${name}'`);
    list.delete(index);
    if (list.items.length === 0) this.#doc.delete("executors");
    this.#reemit();
  }

  #executorNode(name) {
    const list = this.#doc.get("executors", true);
    const node = isSeq(list)
      ? list.items.find((e) => e?.get?.("name") === name)
      : null;
    if (!node) throw new Error(`no executor '${name}'`);
    return node;
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
  // always ends with one, and `ejemplos/basica.yseq` ends without — so saving
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

function assertScope(scope) {
  if (!SCOPES.includes(scope)) {
    throw new Error(`unknown scope '${scope}': expected one of ${SCOPES.join(", ")}`);
  }
}

function assertPhase(phase) {
  if (!PHASES.includes(phase)) {
    throw new Error(`unknown phase '${phase}': expected one of ${PHASES.join(", ")}`);
  }
}

// A step's nested map, as a plain object, or null when it has none.
function readMap(item, key) {
  const node = item.get?.(key);
  if (!node) return null;
  const plain = typeof node.toJSON === "function" ? node.toJSON() : node;
  return plain && typeof plain === "object" ? plain : null;
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
