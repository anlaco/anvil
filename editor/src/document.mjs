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
 * The four step types the engine understands
 * (`crates/cargador/src/lib.rs:2191-2203`). `grpc` is the default when a step
 * says nothing (`lib.rs:322`).
 */
export const STEP_TYPES = ["grpc", "statement", "sequence_call", "pass_fail"];

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
      type: item.get?.("type") ?? "grpc",
      retries: item.get?.("retries") ?? 1,
      disable: item.get?.("disable") ?? false,
      executor: item.get?.("executor") ?? null,
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
   * coherence rules — `range` needs `min` and `max` and forbids `op`/`expected`
   * (`crates/cargador/src/lib.rs:352-405`) — and because changing a threshold is
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
  #reemit() {
    const emitted = String(this.#doc);
    const hadNewline = /\n$/.test(this.#text);
    this.#text = hadNewline ? emitted : emitted.replace(/\n$/, "");
    this.#stale = false;
    this.#error = null;
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
