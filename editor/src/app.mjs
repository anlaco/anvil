// The editor itself: wiring the document model and the engine to the panes.
//
// The layout is TestStand's, cut to what the engine can do today (AP-02): a
// sequence list, a step editor whose fields depend on the step's type, a
// variables pane, and a status bar carrying what the engine says about the file
// as it is typed. No nested control flow, because the engine has none (AP-03),
// and no field the loader would reject, because the editor must not be able to
// build a sequence the loader refuses (AP-04).

import { EditorView, basicSetup } from "codemirror";
import { EditorState } from "@codemirror/state";
import { yaml as yamlLang } from "@codemirror/lang-yaml";

import {
  SequenceDocument,
  PHASES,
  SCOPES,
  STEP_TYPES,
  COMPARISON_CODES,
} from "./document.mjs";
import {
  EXECUTION,
  INSERT_MENU,
  MENU_BAR,
  PAGE_DETAILS,
  PROPERTY_PAGES as PARITY_PAGES,
  SEQUENCE_WINDOW,
  STATUS_BAR,
  TYPE_LABELS,
  VARIABLES_PANE,
  gapTooltip,
  parityById,
} from "./paridad.mjs";
import { browserPool, connectBridge, EngineHostError } from "./engine-pool.mjs";
import { gatherFiles, isPath, unsavedPathHint } from "./neighbours.mjs";
import {
  applyEvent,
  callStack,
  nestedRows,
  newRunState,
  rowKey,
  runButton,
} from "./run-state.mjs";

// The shell forwards this window's console to its own stdout by itself
// (ADR-0037 §e, `editor/electron/main.mjs`), so the page carries no
// shell-specific logging code. What it does say, once, is what it woke up
// with: the engine cannot run without `SharedArrayBuffer`, and a shell that
// silently fails to provide it is the single most expensive way for this to
// break (ADR-0030, ADR-0031).
if (window.anvil) {
  console.log(
    "shell capabilities:",
    JSON.stringify({
      url: location.href,
      crossOriginIsolated: window.crossOriginIsolated,
      sharedArrayBuffer: typeof SharedArrayBuffer,
      secureContext: window.isSecureContext,
    }),
  );
}

// The engine runs on a worker thread, never on this one: it is a synchronous
// WASM component, and on the main thread it would freeze the interface for as
// long as a sequence takes. One worker for now; multi-UUT is more of them.
const engine = browserPool({ max: 1 });

const el = (id) => document.getElementById(id);

const ui = {
  panes: document.querySelector(".panes"),
  palette: el("palette"),
  list: el("sequence-list"),
  sequenceTitle: el("sequence-title"),
  textPane: document.querySelector(".pane.text"),
  sequencePane: document.querySelector(".pane.sequence"),
  stepPane: document.querySelector(".pane.step"),
  stepTitle: el("step-title"),
  stepEditor: el("step-editor"),
  textEditor: el("text-editor"),
  variables: el("variables"),
  filename: el("filename"),
  statusLight: el("status-light"),
  statusText: el("status-text"),
  versions: el("versions"),
  run: el("run"),
  menus: document.querySelector(".menus"),
  sequences: el("sequences"),
  templates: el("templates"),
  ioConfigurations: el("io-configurations"),
  output: el("output"),
  execution: el("execution"),
  statusFields: el("status-fields"),
  execControls: el("exec-controls"),
};

const state = {
  /** @type {SequenceDocument|null} */
  doc: null,
  /** @type {FileSystemFileHandle|null} */
  handle: null,
  filename: null,
  dirty: false,
  selected: null, // { phase, index }
  // Which tab and which Properties page the step settings are showing.
  // Remembered across re-renders, because every edit re-renders the pane and
  // being thrown back to General after typing a threshold is unusable.
  stepTab: "module",
  stepPage: "General",
  view: "steps",
  // Which of the docked panes is showing. TestStand's panes are tabbed, and
  // the tab is remembered across repaints for the same reason the Properties
  // page is: every edit re-renders, and being thrown back is unusable.
  leftTab: "templates",
  rightTab: "variables",
  bottomTab: "step",
  /** The phases collapsed in the step list, by name. */
  collapsed: new Set(),
  /** What the last run printed, for the Output tab. */
  output: null,
  /**
   * What the executors this sequence declares say they serve (ADR-0044).
   *
   * `null` until asked. `{ executors: {name: {describes, steps, reason}} }`
   * once answered — the engine's own document, unchanged: re-shaping it here
   * would be a second opinion about a signature, and there is only supposed to
   * be one.
   */
  catalog: null,
  /** Why the last `describe` did not answer, or null. */
  catalogError: null,
  /** True while one is in flight, so the button cannot be pressed twice. */
  catalogBusy: false,
  /**
   * The executors this editor brought up from `dev:` (ADR-0046 §5), by name:
   * `{ pid, exe, args, runtime, at }`.
   *
   * Only what **this** editor started. An executor already listening — started
   * at boot, by a service manager, on another machine — is not in here and must
   * not be: the sequence names an address, and who put something there is not
   * the editor's to claim.
   */
  dev: {},
  /** Why the last start did not happen, or how the last one died. */
  devError: null,
  /** The executor a start or stop is in flight for, so it cannot be double-pressed. */
  devBusy: null,
  text: null, // CodeMirror view
  validateTimer: null,
  bridge: null, // the URL, once connected
  // Why the shell could not start a bridge for this file (no engine found, not
  // the engine…). Kept so the next status line does not bury it: validation
  // finishes after the failed start more often than not.
  runUnavailable: null,
  // For the corner of the status bar: `{ editor, packaged }` from the shell,
  // and the version of the engine Run last used, or why there is none.
  versions: null,
  engineVersion: null,
  /**
   * What the engine is doing right now, fed by the NDJSON of `--events`.
   * The state machine that reads it lives in ./run-state.mjs, out of the DOM so
   * it can be asserted line by line.
   */
  run: null,
  /**
   * True from the click on Run until the engine comes back.
   *
   * It is separate from `run` because that one outlives the run: the verdicts
   * stay on the rows afterwards. This is what `renderAll` needs, and without it
   * any repaint — inserting a step, saving, typing — recomputed the button from
   * `bridged` alone and re-armed it **mid-run**, offering a second concurrent
   * run over the same engine and bridge.
   */
  runInFlight: false,
};

/**
 * The half of the run that has no row: what is happening inside a subsequence,
 * and whether the stream lost anything. Both go to the status bar because
 * neither belongs on a step row — the editor has no rows for a subsequence's
 * steps, and a gap belongs to the stream, not to a step.
 */
function renderRunStatus() {
  const r = state.run;
  if (!r) return;
  const partes = [];
  if (r.nested) partes.push(`inside: ${r.nested}`);
  if (r.lost) partes.push(`${r.lost} event line(s) lost`);
  if (partes.length) status("busy", partes.join(" — "));
}

/** Feeds one raw stderr line to the run state and repaints if it moved. */
function onEvent(line) {
  if (!state.run) return;
  if (!applyEvent(state.run, line)) return;
  renderSequence();
  renderExecution();
  renderRunStatus();
}

// ---------------------------------------------------------------------------
// The menu bar, built from the inventory.
//
// TestStand's ten menus, in TestStand's order. A menu Anvil does not have is
// here and greyed, and it opens onto nothing rather than onto a list of
// commands nobody checked — what TestStand puts under Execute, Debug,
// Configure and Tools is not written down in this repo, and inventing it would
// be asserting something about NI's product on no evidence (ADR-0043, and the
// second-hand warning it carries).
//
// Inside the desktop shell this bar is hidden and the platform's native menu
// carries the actions, so the greyed menus are a browser-only statement. That
// is the honest limit of doing this in the page: a native menu cannot say why
// an item is missing.
// ---------------------------------------------------------------------------

function renderMenuBar() {
  ui.menus.replaceChildren();

  for (const menu of MENU_BAR) {
    const wrap = document.createElement("div");
    wrap.className = "menu";
    wrap.dataset.parity = menu.state ?? "built";

    const button = document.createElement("button");
    button.type = "button";
    button.dataset.menu = menu.id;
    button.textContent = menu.label;
    // A menu with nothing behind it does not open. It stays on the bar so the
    // person can see it exists in TestStand and hover for why it is dead here.
    const opens = Array.isArray(menu.items) && menu.items.length > 0;
    button.disabled = !opens;
    button.title = gapTooltip(menu) ?? `${menu.label} menu`;

    const list = document.createElement("ul");
    list.hidden = true;
    for (const entry of menu.items ?? []) {
      if (entry.sep) {
        list.append(document.createElement("hr"));
        continue;
      }
      const item = document.createElement("li");
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = entry.label;
      b.dataset.parity = entry.state;
      if (entry.action) {
        b.dataset.action = entry.action;
      } else {
        b.disabled = true;
        b.title = gapTooltip(entry) ?? entry.label;
      }
      item.append(b);
      list.append(item);
    }

    wrap.append(button, list);
    ui.menus.append(wrap);
  }
}

// ---------------------------------------------------------------- rendering

/**
 * The Sequences pane: what TestStand lists beside the step list.
 *
 * TestStand shows MainSequence and the sequence file's callbacks — the model's
 * entry points, SequenceFilePreStep and the rest. Anvil has the first half:
 * the sequence being edited, and the subsequences it declares inline. It has
 * no callbacks, and that is a decision rather than a gap (ADR-0016 built the
 * process model as a wrapper sequence with none), so the row that would list
 * them says `never` and cites it.
 *
 * Selecting a subsequence is not offered yet: the document model edits one
 * sequence, and a pane that switched to a subsequence it cannot edit would be
 * a worse lie than not offering it. The names are listed because seeing what a
 * file contains is most of what this pane is for.
 */
function renderSequences() {
  ui.sequences.replaceChildren();
  const doc = state.doc;
  if (!doc) return;

  // TestStand's columns for this tab: the sequence, its comment, and the
  // requirement it is linked to. Anvil has neither of the last two, so they
  // are headed and empty rather than left off — the heading is the gap.
  const head = document.createElement("div");
  head.className = "seq-head";
  for (const h of ["Sequence", "Comment", "Requirement"]) {
    const c = document.createElement("span");
    c.textContent = h;
    head.append(c);
  }
  head.title = gapTooltip(parityById("window.sequences.columns"));
  ui.sequences.append(head);

  const row = (name, { current = false, sub = false, hint } = {}) => {
    const r = document.createElement("div");
    r.className = "seq-row";
    if (current) r.dataset.current = "true";
    if (sub) r.dataset.sub = "true";
    const n = document.createElement("span");
    n.textContent = name;
    r.append(n, document.createElement("span"), document.createElement("span"));
    if (hint) r.title = hint;
    ui.sequences.append(r);
  };

  row(doc.name ?? "MainSequence", {
    current: true,
    hint: "The sequence this editor is editing.",
  });
  for (const name of doc.subsequenceNames?.() ?? []) {
    row(name, {
      sub: true,
      hint: "A subsequence declared in this file. Edit it in the Text view.",
    });
  }

  parityRow(ui.sequences, "window.sequences.callbacks");
}

/** One greyed line standing for something TestStand lists and Anvil does not. */
function parityRow(parent, id) {
  const entry = parityById(id);
  if (!entry) throw new Error(`no parity entry: ${id}`);
  const row = document.createElement("div");
  row.className = "parity-row";
  row.dataset.parity = entry.state;
  row.textContent = entry.label;
  row.title = gapTooltip(entry) ?? entry.label;
  parent.append(row);
  return row;
}

/** TestStand's Settings column: what the step does besides call its module. */
function settingsSummary(step) {
  const bits = [];
  if (step.disable) bits.push("Skip");
  if (step.precondition) bits.push("Precondition");
  if (step.pause_on_fail) bits.push("Post Action");
  if (step.retries > 1) bits.push(`Loop ${step.retries}`);
  if (step.assign && Object.keys(step.assign).length) bits.push("Expressions");
  return bits.join(", ");
}

function renderSequence() {
  const doc = state.doc;
  ui.list.replaceChildren();
  if (!doc) return;

  const nested = state.run ? nestedRows(state.run) : new Map();

  for (const phase of PHASES) {
    const steps = doc.steps(phase);
    const collapsed = state.collapsed.has(phase);

    // TestStand's group header: a `+`/`−`, the phase capitalised, and how many
    // steps are in it — `+ Setup (17)`. Collapsing is per phase and survives a
    // repaint, because every edit repaints and a group that sprang open again
    // on each keystroke would be worse than not collapsing at all.
    const head = document.createElement("button");
    head.type = "button";
    head.className = "phase";
    head.dataset.collapsed = String(collapsed);
    head.setAttribute("aria-expanded", String(!collapsed));
    head.textContent = `${collapsed ? "+" : "\u2212"} ${phase[0].toUpperCase()}${phase.slice(1)} (${steps.length})`;
    head.addEventListener("click", () => {
      if (collapsed) state.collapsed.delete(phase);
      else state.collapsed.add(phase);
      renderSequence();
    });
    ui.list.append(head);

    // A collapsed phase is still a drop target, or dragging a step into one
    // would need it opened first.
    if (collapsed) {
      makeDropTarget(head, phase, () => steps.length);
      continue;
    }

    for (const step of steps) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "step-row";
      row.dataset.disabled = String(step.disable);
      const current =
        state.selected?.phase === phase && state.selected?.index === step.index;
      row.setAttribute("aria-current", String(current));

      // What the engine says about this row, if a run is on: `running` is the
      // step being executed, the rest is the verdict it already produced.
      const r = state.run;
      if (r) {
        const running = r.current?.phase === phase && r.current?.index === step.index;
        const verdict = r.results.get(rowKey(phase, step.index));
        if (running) row.dataset.run = "running";
        else if (verdict) row.dataset.run = verdict;
      }

      // The gutter a breakpoint would sit in. Drawn, and nothing can be set in
      // it: the engine cannot stop at a step. Hovering says so.
      const gutter = document.createElement("span");
      gutter.className = "gutter";
      gutter.title = gapTooltip(parityById("window.steps.gutter"));

      const name = document.createElement("span");
      name.className = "name c-step";
      name.textContent = step.name ?? "(unnamed)";

      // TestStand's Description column: written by the editor from the step,
      // never typed — the same text the General page shows.
      const desc = document.createElement("span");
      desc.className = "c-desc";
      desc.textContent = describeStep(step);

      const settings = document.createElement("span");
      settings.className = "c-settings";
      settings.textContent = settingsSummary(step);

      // A mark, not just a colour: a row that says pass or fail by hue alone is
      // unreadable to whoever cannot tell the hues apart, and this gets read
      // next to a bench.
      const mark = document.createElement("span");
      mark.className = "run-mark";
      const rs = row.dataset.run;
      mark.textContent = rs ? (MARKS[rs] ?? "?") : "";
      if (rs) mark.title = rs;

      row.append(gutter, name, desc, settings, mark);
      row.addEventListener("click", () => {
        state.selected = { phase, index: step.index };
        renderSequence();
        renderStep();
        renderStatusFields();
      });
      makeDraggable(row, { kind: "move", phase, index: step.index });
      makeDropTarget(row, phase, () => step.index);
      ui.list.append(row);

      // The steps that ran inside this call. Until these had rows the list was
      // flat: a `sequence_call` stayed lit and nothing on screen said what had
      // already passed or failed underneath it.
      for (const frame of nested.get(rowKey(phase, step.index)) ?? []) {
        ui.list.append(nestedRow(frame));
      }
    }

    // A phase with no steps still has to be a target, or a sequence that starts
    // empty can never receive its first step by dragging. TestStand's own
    // wording for the row.
    if (steps.length === 0) {
      const empty = document.createElement("div");
      empty.className = "phase-empty";
      empty.textContent = "<Insert Steps Here>";
      makeDropTarget(empty, phase, () => 0);
      ui.list.append(empty);
    } else {
      // TestStand closes a group with this, and it is what makes the extent of
      // a phase readable when three of them sit in one flat list.
      const end = document.createElement("div");
      end.className = "phase-end";
      end.textContent = "<End Group>";
      makeDropTarget(end, phase, () => steps.length);
      ui.list.append(end);
    }
  }
}

/** The marks a row carries for what the engine said about it. */
const MARKS = { running: "▶", pass: "✓", done: "•", fail: "✕", error: "!", skipped: "–" };

/**
 * A step that ran inside a subsequence, shown under the call that ran it.
 *
 * Not a `button`: it is not part of the document and there is nothing to
 * select. It exists only while a run's events describe it, and it is indented
 * by its depth, which the engine states on every line (ADR-0033).
 */
function nestedRow(frame) {
  const row = document.createElement("div");
  row.className = "step-row nested";
  row.style.setProperty("--depth", String(frame.depth));
  if (frame.status) row.dataset.run = frame.status;

  const gutter = document.createElement("span");
  gutter.className = "gutter";

  const name = document.createElement("span");
  name.className = "name c-step";
  name.textContent = frame.name;

  const desc = document.createElement("span");
  desc.className = "c-desc";
  desc.textContent = frame.sequence ? `in ${frame.sequence}` : "";

  const settings = document.createElement("span");
  settings.className = "c-settings";
  settings.textContent = frame.phase ?? "";

  const mark = document.createElement("span");
  mark.className = "run-mark";
  mark.textContent = frame.status ? (MARKS[frame.status] ?? "?") : "";
  if (frame.status) mark.title = frame.status;

  row.append(gutter, name, desc, settings, mark);
  return row;
}

// What is being dragged: a step being moved, or a type being inserted from the
// palette. Kept here rather than in `dataTransfer` because Chromium does not
// expose the payload during `dragover`, and the drop indicator has to know
// whether the drag is a move before the drop happens.
let dragging = null;

function makeDraggable(el, payload) {
  el.draggable = true;
  el.addEventListener("dragstart", (e) => {
    dragging = payload;
    e.dataTransfer.effectAllowed = "move";
    // Firefox refuses to start a drag with no data set.
    e.dataTransfer.setData("text/plain", payload.kind);
    el.classList.add("dragging");
  });
  el.addEventListener("dragend", () => {
    dragging = null;
    el.classList.remove("dragging");
    clearDropMarks();
  });
}

function clearDropMarks() {
  for (const el of ui.list.querySelectorAll(".drop-before, .drop-after")) {
    el.classList.remove("drop-before", "drop-after");
  }
}

/**
 * Makes an element take a drop, and say where the step would land.
 *
 * The half of the row the pointer is on decides before or after, which is what
 * every list with drag and drop does and what someone dragging expects. Order
 * inside a phase is execution order, so landing one row off is a different
 * sequence, not a cosmetic slip — hence the line showing exactly where.
 */
function makeDropTarget(el, phase, indexOf) {
  el.addEventListener("dragover", (e) => {
    if (!dragging) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    const box = el.getBoundingClientRect();
    const after = e.clientY > box.top + box.height / 2;
    clearDropMarks();
    el.classList.add(after ? "drop-after" : "drop-before");
  });
  el.addEventListener("dragleave", () => el.classList.remove("drop-before", "drop-after"));
  el.addEventListener("drop", (e) => {
    if (!dragging) return;
    e.preventDefault();
    const box = el.getBoundingClientRect();
    const after = e.clientY > box.top + box.height / 2;
    const at = indexOf() + (after ? 1 : 0);
    const payload = dragging;
    dragging = null;
    clearDropMarks();
    dropAt(payload, phase, at);
  });
}

function dropAt(payload, phase, at) {
  const doc = state.doc;
  if (!doc) return;
  try {
    if (payload.kind === "insert") {
      const why = doc.cannotAdd(payload.type);
      if (why) {
        status("fail", `Cannot insert a ${payload.type} step: ${why}`);
        return;
      }
      const index = doc.addStep(phase, payload.type, at);
      state.selected = { phase, index };
    } else {
      // Dropping a step after itself, or on its own place, is not a move.
      const sameSpot =
        payload.phase === phase && (at === payload.index || at === payload.index + 1);
      if (sameSpot) return;
      // Removing the step first shifts everything after it down by one.
      const target = payload.phase === phase && at > payload.index ? at - 1 : at;
      const index = doc.moveStepTo(payload.phase, payload.index, phase, target);
      state.selected = { phase, index };
    }
  } catch (e) {
    status("fail", e.message);
    return;
  }
  afterEdit();
}

// What each comparison code means, in the words of someone choosing one. The
// codes are TestStand's and are not translated (ADR-0040 §7): someone arriving
// from there recognises them, which is the whole reason they were adopted.
const COMPARISON_DOC = {
  EQ: "Equal to low.",
  NE: "Not equal to low.",
  GT: "Greater than low.",
  LT: "Less than low.",
  GE: "Greater than or equal to low.",
  LE: "Less than or equal to low.",
  GTLT: "Inside low..high, both excluded.",
  GELE: "Inside low..high, both included.",
  GELT: "Inside low..high, low included.",
  GTLE: "Inside low..high, high included.",
  LTGT: "Outside low..high, both excluded.",
  LEGE: "Outside low..high, both included.",
  LEGT: "Outside low..high, low included.",
  LTGE: "Outside low..high, high included.",
  EQT: "Nominal with a tolerance: nominal -lower/+upper.",
  none: "Records the value and judges nothing: the step reads 'done'.",
};

// What each step type is, in the words of someone deciding which to insert.
// These four are the engine's own (crates/cargador/src/lib.rs:2191-2203); the
// palette offers no fifth, because a step the loader does not know is a step
// the editor must not be able to create (AP-04).
const STEP_TYPE_DOC = {
  action: "Calls a module to do something; judges nothing.",
  pass_fail: "Passes or fails: on an expression, or on what a module answers.",
  numeric_limit: "Judges a number against a limit.",
  statement: "Assigns to variables. The engine runs it; no executor involved.",
  sequence_call: "Calls a subsequence.",
};

function renderPalette() {
  // The "Step Types" heading is the section's, in the HTML: the palette holds
  // two sections and each is titled once.
  ui.palette.replaceChildren();

  const phase = state.selected?.phase ?? "main";

  for (const type of STEP_TYPES) {
    // A type the document cannot take right now is offered greyed out with the
    // reason, not hidden: hiding it would leave someone looking for a step type
    // that TestStand has and wondering whether Anvil lacks it entirely.
    const blocked = state.doc?.cannotAdd(type) ?? null;

    const item = document.createElement("button");
    item.type = "button";
    item.className = "palette-item";
    item.disabled = !state.doc || Boolean(blocked);
    item.append(document.createTextNode(type));
    const doc = document.createElement("small");
    doc.textContent = blocked ? `Unavailable: ${blocked}.` : STEP_TYPE_DOC[type];
    item.append(doc);
    item.title = blocked
      ? `Cannot insert a ${type} step: ${blocked}`
      : `Drag a ${type} step where you want it, or click to add it at the end of ${phase}`;
    item.addEventListener("click", () => {
      // Clicking still appends: it is the path that needs no pointer skill, and
      // dragging is not available to everyone.
      const index = state.doc.addStep(phase, type);
      state.selected = { phase, index };
      afterEdit();
    });
    if (!item.disabled) makeDraggable(item, { kind: "insert", type });
    ui.palette.append(item);
  }

  const note = document.createElement("p");
  note.className = "palette-note";
  // The steps an executor serves are the other half of this palette, and they
  // come from asking it (ADR-0021). That needs the bridge, so saying what is
  // missing beats an empty list that looks like an executor with no steps —
  // the distinction ADR-0019's Rule 2 is about.
  note.textContent = state.doc
    ? `Drag one onto the sequence to put it where you want, or click to add it at the end of ${phase}. Steps served by executors will appear here once the bridge can ask them for their catalog.`
    : "Open a sequence to insert steps.";
  ui.palette.append(note);
}

/**
 * One labelled field of a step's settings.
 *
 * `hint` becomes the field's tooltip, not a line of prose under it. TestStand
 * explains nothing in the panel itself — a step's settings are a dense grid of
 * labelled boxes — and a paragraph under every field made this panel twice as
 * tall as the thing it is copying. The words are kept, out of the way.
 */
function field(parent, label, input, hint) {
  const l = document.createElement("label");
  l.textContent = label;
  const id = `f-${label.toLowerCase().replace(/\W+/g, "-")}`;
  // The id goes on the control, never on a wrapper. Type and Module hand in a
  // row of elements rather than a bare input, and putting the id on that <div>
  // left `<label for>` pointing at something that cannot be focused — clicking
  // the label did nothing, and anything looking the field up by id got a <div>
  // with no value. Found by driving the editor: two modules typed into the
  // Module field never reached the file.
  const control = input.matches("input, select, textarea, button")
    ? input
    : (input.querySelector("input, select, textarea, button") ?? input);
  control.id = id;
  l.htmlFor = id;
  if (hint) {
    l.title = hint;
    control.title = hint;
  }
  parent.append(l, input);
}

function group(parent, title) {
  const h = document.createElement("div");
  h.className = "group";
  h.textContent = title;
  parent.append(h);
}

function textInput(value, onChange) {
  const input = document.createElement("input");
  input.type = "text";
  input.value = value ?? "";
  input.addEventListener("change", () => onChange(input.value));
  return input;
}

/**
 * A number field that shows the same thing the file will contain.
 *
 * Deliberately `type="text"` and not `type="number"`. A number input renders
 * through the browser's locale, so on a Spanish machine a limit of 4.5 appears
 * as "4,5" while the YAML says `4.5`. In a test sequencer, a threshold that
 * reads differently on screen than in the file is not a cosmetic problem — the
 * decimal separator is the difference between 4.5 V and 45 V.
 *
 * Input that is not a number is refused and the field snaps back, rather than
 * being written as something else: a limit the editor could not read must not
 * become a limit the engine reads differently (ADR-0019, Rule 2).
 */
function numberInput(value, onChange) {
  const input = document.createElement("input");
  input.type = "text";
  input.inputMode = "decimal";
  input.value = String(value ?? "");
  input.addEventListener("change", () => {
    const raw = input.value.trim();
    const parsed = /^[+-]?(\d+\.?\d*|\.\d+)$/.test(raw) ? Number(raw) : NaN;
    if (Number.isNaN(parsed)) {
      input.value = String(value ?? "");
      status("fail", `'${raw}' is not a number — use a dot for decimals, as the file does`);
      return;
    }
    onChange(parsed);
  });
  return input;
}

function select(options, value, onChange) {
  const s = document.createElement("select");
  for (const opt of options) {
    const o = document.createElement("option");
    o.value = opt;
    o.textContent = opt;
    s.append(o);
  }
  s.value = value;
  s.addEventListener("change", () => onChange(s.value));
  return s;
}

// ---------------------------------------------------------------------------
// A step's settings, laid out the way TestStand lays them out: two tabs,
// Properties and Module, and inside Properties the list of pages down the left.
//
// The list is TestStand's, in TestStand's order, and it is deliberately
// complete. A page Anvil has not built yet is greyed out rather than left off,
// so that what is missing is visible in the place where it will appear, with a
// tooltip saying what TestStand does there. The parity inventory lives in the
// editor, where it cannot go stale, instead of in a document nobody reopens.
//
// `render` is what Anvil puts on the page; `todo` marks a page it has nothing
// for yet, and a page never has both. `types` limits a page to the step types
// that have it, as TestStand's own page list changes with the step type.
// ---------------------------------------------------------------------------

/** The step types that can call an executor (ADR-0040 §1). */
const CALLS_AN_EXECUTOR = ["action", "pass_fail", "numeric_limit"];

// ---------------------------------------------------------------------------
// The step type menu, as TestStand's: a read-only box with a button beside it
// that drops a menu of every type TestStand offers, grouped and in TestStand's
// order, with submenus opening to the right.
//
// The menu is complete on purpose, like the page list: a type Anvil does not
// have is greyed out where TestStand puts it, rather than left off. What it
// says there comes from `paridad.mjs` — the inventory lives in one place, and
// a test holds this file and that one together (ADR-0043 §5).
// ---------------------------------------------------------------------------

/**
 * Whether an entry can be used at all.
 *
 * A **leaf** is usable when it is a type Anvil has. A **group** opens whenever
 * it has something to show, even when not one type inside it exists here: the
 * submenu's job is to show what TestStand offers under that heading, and a
 * heading that will not open says nothing. So Flow Control opens onto its
 * fourteen greyed-out entries rather than being a dead word.
 *
 * A group whose contents are not written down yet — Synchronization, Database,
 * Data Streams, LabVIEW Utility — has nothing to open, and stays greyed until
 * someone reads them off TestStand. It carries its own verdict for that
 * reason: an empty group is itself a gap.
 */
function entryEnabled(entry) {
  if (entry.sep) return false;
  if (entry.items) return entry.items.length > 0;
  return Boolean(entry.type);
}

/** Whether anything inside a group is a type Anvil can actually make. */
function groupHasAType(entry) {
  return (entry.items ?? []).some((e) => (e.items ? groupHasAType(e) : Boolean(e.type)));
}

/** Every menu currently on screen, outermost first, so they close together. */
let openMenus = [];

function closeTypeMenu() {
  for (const m of openMenus) m.remove();
  openMenus = [];
  document.removeEventListener("mousedown", onMenuOutside, true);
  document.removeEventListener("keydown", onMenuKey, true);
}

function onMenuOutside(event) {
  if (!openMenus.some((m) => m.contains(event.target))) closeTypeMenu();
}

function onMenuKey(event) {
  if (event.key === "Escape") {
    event.stopPropagation();
    closeTypeMenu();
  }
}

/**
 * Builds one menu panel. `depth` is how many are already open to its left, so
 * that opening a submenu closes any deeper one still showing.
 */
function buildTypeMenu(entries, choose, depth) {
  const menu = document.createElement("div");
  menu.className = "ts-menu";
  menu.setAttribute("role", "menu");

  for (const entry of entries) {
    if (entry.sep) {
      const hr = document.createElement("div");
      hr.className = "ts-sep";
      menu.append(hr);
      continue;
    }

    const item = document.createElement("button");
    item.type = "button";
    item.className = "ts-item";
    item.setAttribute("role", "menuitem");
    item.disabled = !entryEnabled(entry);

    const text = document.createElement("span");
    text.className = "ts-label";
    text.textContent = entry.label;
    item.append(text);

    if (entry.items) {
      item.classList.add("ts-submenu");
      const arrow = document.createElement("span");
      arrow.className = "ts-arrow";
      arrow.textContent = "▸";
      item.append(arrow);
      // A group that opens onto nothing Anvil has says so, but still opens.
      const empty = !groupHasAType(entry);
      if (empty) item.classList.add("ts-none-yet");
      item.title = item.disabled
        ? (gapTooltip(entry) ?? `${entry.label}: nothing to open yet.`)
        : empty
          ? `${entry.label} step types — TestStand has these; Anvil has none of them yet.`
          : `${entry.label} step types`;

      const open = () => {
        while (openMenus.length > depth + 1) openMenus.pop().remove();
        if (item.disabled) return;
        const sub = buildTypeMenu(entry.items, choose, depth + 1);
        placeMenu(sub, item.getBoundingClientRect(), true);
      };
      // Hover opens it, as a menu does; the submenu sits against this item.
      // Click opens it too, for anyone not driving this with a mouse.
      item.addEventListener("mouseenter", open);
      item.addEventListener("click", (event) => {
        event.stopPropagation();
        open();
      });
    } else {
      item.title = entry.type
        ? `Make this a ${entry.label}`
        : (gapTooltip(entry) ?? `${entry.label}: not available.`);
      item.addEventListener("mouseenter", () => {
        while (openMenus.length > depth + 1) openMenus.pop().remove();
      });
      if (entry.type) {
        item.addEventListener("click", () => {
          const chosen = entry.type;
          closeTypeMenu();
          choose(chosen);
        });
      }
    }

    menu.append(item);
  }

  return menu;
}

/**
 * Puts a menu on screen against `anchor`, flipping it when it would fall off.
 *
 * Every menu, the first one included, opens against the right edge of what it
 * belongs to and top-aligned with it — which is where TestStand drops the step
 * type menu: beside the Type control, not under it, overlaying whatever is to
 * its right. When there is not enough room below, it rides up until it fits
 * rather than being cut off, so the whole list is always reachable.
 */
function placeMenu(menu, anchor, isSubmenu) {
  menu.style.visibility = "hidden";
  document.body.append(menu);
  openMenus.push(menu);

  const { width, height } = menu.getBoundingClientRect();
  const margin = 4;

  let left = anchor.right - (isSubmenu ? 2 : -2);
  let top = anchor.top - (isSubmenu ? 4 : 0);

  if (left + width > window.innerWidth - margin) {
    left = anchor.left - width + (isSubmenu ? 2 : -2);
  }
  if (top + height > window.innerHeight - margin) {
    top = window.innerHeight - height - margin;
  }

  menu.style.left = `${Math.max(margin, left)}px`;
  menu.style.top = `${Math.max(margin, top)}px`;
  menu.style.visibility = "";
  return menu;
}

/** A small inline icon, drawn rather than loaded: 16×16, currentColor-aware. */
function svgIcon(body) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("width", "16");
  svg.setAttribute("height", "16");
  svg.setAttribute("aria-hidden", "true");
  svg.innerHTML = body;
  return svg;
}

/** TestStand's Type row: a read-only box, and the button that drops the menu. */
function typeField(step, choose) {
  const row = document.createElement("div");
  row.className = "type-row";

  const box = document.createElement("input");
  box.type = "text";
  box.readOnly = true;
  box.className = "type-box";
  box.value = TYPE_LABELS[step.type] ?? step.type ?? "";

  const button = document.createElement("button");
  button.type = "button";
  button.className = "type-button";
  button.setAttribute("aria-haspopup", "menu");
  button.setAttribute("aria-label", "Choose the step type");
  // TestStand's step-types glyph — a little hierarchy of shapes — not a
  // dropdown arrow: this button opens the type menu, it does not expand the
  // box beside it.
  button.append(
    svgIcon(
      `<path d="M4 3v9h3M4 7.5h3" fill="none" stroke="currentColor" stroke-width="1.1"/>
       <path d="M11.5 2.2 13.4 5.2H9.6z" fill="#c0392b"/>
       <circle cx="11.5" cy="8" r="1.7" fill="#2f6fbf"/>
       <rect x="9.9" y="10.6" width="3.2" height="3.2" fill="#2e8b57"/>`,
    ),
  );
  button.addEventListener("click", (event) => {
    event.stopPropagation();
    if (openMenus.length > 0) return closeTypeMenu();
    const menu = buildTypeMenu(INSERT_MENU, choose, 0);
    placeMenu(menu, button.getBoundingClientRect(), false);
    document.addEventListener("mousedown", onMenuOutside, true);
    document.addEventListener("keydown", onMenuKey, true);
  });

  // Clicking the box itself drops the menu too: in TestStand the whole control
  // is the thing you click, and a read-only box that ignores a click reads as
  // broken.
  box.addEventListener("mousedown", (event) => {
    event.preventDefault();
    button.click();
  });

  row.append(box, button);
  return row;
}

/**
 * What fills each page Anvil has built, by the id `paridad.mjs` gives it.
 *
 * The list, its order and what a greyed page says all live there; this map is
 * only the wiring, and a test asserts the two agree — every `built` id has a
 * renderer here, and every renderer here answers to a `built` id.
 */
const PAGE_RENDERERS = {
  pageGeneral,
  pageRunOptions,
  pageLooping,
  pagePostActions,
  pageExpressions,
  pagePreconditions,
};

/** The page list as the editor uses it: the inventory, plus its renderer. */
const PROPERTY_PAGES = PARITY_PAGES.map((page) => ({
  ...page,
  render: page.render ? PAGE_RENDERERS[page.render] : undefined,
}));

/**
 * The tabs a step of this type shows, in TestStand's order.
 *
 * What a type is judged on is **not** a page inside Properties: TestStand gives
 * it a tab of its own, and a step with nothing to judge simply has no such tab.
 * So the tab strip changes with the type, while the Properties page list
 * underneath is the same for every step.
 *
 * **Module comes first.** In 2019 the strip read `Properties | Module`; in
 * 2026Q3 it reads `Module | Limits | Data Source | Properties`, which is also
 * the order of the work: what the step calls, what it is judged against, where
 * the number comes from, and then everything else.
 */
function tabsFor(type) {
  const tabs = [["module", "Module"]];
  if (type === "numeric_limit") tabs.push(["limits", "Limits"]);
  if (type === "pass_fail" || type === "numeric_limit") {
    tabs.push(["data-source", "Data Source"]);
  }
  tabs.push(["properties", "Properties"]);
  return tabs;
}

/**
 * A fresh field grid inside a page.
 *
 * `stacked` because that is how TestStand lays a step's settings out: the
 * label sits above its field, not beside it. The variables pane keeps the
 * label-beside-field grid, which is what it has always used.
 */
function newFields(parent, className = "fields stacked") {
  const f = document.createElement("div");
  f.className = className;
  parent.append(f);
  return f;
}

/** A read-only box that stands for something Anvil does not write yet. */
function disabledInput(placeholder) {
  const input = document.createElement("input");
  input.type = "text";
  input.disabled = true;
  input.value = placeholder;
  return input;
}

function checkbox(checked, onChange) {
  const box = document.createElement("input");
  box.type = "checkbox";
  box.checked = Boolean(checked);
  box.addEventListener("change", () => onChange(box.checked));
  return box;
}

/**
 * What TestStand does here that Anvil does not do yet.
 *
 * `detail` goes in the tooltip rather than on the page: the note has to fit
 * beside the thing it is about, and a paragraph pushes the panel past the
 * height TestStand's own has.
 */
function todoNote(parent, text, detail) {
  const p = document.createElement("p");
  p.className = "todo";
  p.textContent = `Not implemented: ${text}`;
  if (detail) p.title = detail;
  parent.append(p);
}

/**
 * The gaps inside a page Anvil has built, read off the inventory.
 *
 * A page is not all-or-nothing — Looping exists and holds `retries`, and none
 * of TestStand's loop types — so what a built page still lacks is declared in
 * `paridad.mjs` like any other gap, and shown at the foot of the page it
 * belongs to (ADR-0043 §4).
 */
function parityNote(parent, ...ids) {
  for (const id of ids) {
    const entry = PAGE_DETAILS.find((d) => d.id === id);
    if (!entry) throw new Error(`no parity entry: ${id}`);
    const p = document.createElement("p");
    p.className = "todo";
    p.dataset.parity = entry.state;
    p.textContent = `Not implemented: ${entry.label}`;
    p.title = gapTooltip(entry);
    parent.append(p);
  }
}

function note(parent, text) {
  const p = document.createElement("p");
  p.className = "hint span";
  p.textContent = text;
  parent.append(p);
}

/**
 * The Description box of TestStand's General page: written by the editor from
 * the step itself, never typed. TestStand shows "Action, XPA.lvproj,
 * TestNUSOBASequence.vi" — the type, then what it calls.
 */
function describeStep(step) {
  const parts = [step.type ?? "(no type)"];
  if (step.module) {
    if (step.executor) parts.push(step.executor);
    parts.push(step.module);
  } else if (step.type === "statement" && step.statement) {
    parts.push(step.statement);
  } else if (step.type === "sequence_call" && step.sequence) {
    parts.push(step.sequence);
  } else if (step.type === "pass_fail" && step.condition) {
    parts.push(step.condition);
  } else if (step.type === "numeric_limit" && step.value) {
    parts.push(step.value);
  }
  return parts.join(", ");
}

/**
 * General, whole: TestStand's own fields in TestStand's arrangement — the
 * identity of the step on the left, what it is and what it says on the right.
 */
function pageGeneral(body, ctx) {
  const { step, doc, edit } = ctx;

  const page = document.createElement("div");
  page.className = "page-general";
  body.append(page);

  const left = newFields(page);
  const right = newFields(page, "fields stacked wide");

  field(
    left,
    "Name",
    textInput(step.name, (v) => edit("name", v)),
    "What the report, the events and this list call the step.",
  );
  // Changing the type rewrites the rest of the step, so it does not go through
  // `edit`: the fields the old type owned have to go with it (ADR-0040).
  field(
    left,
    "Type",
    typeField(step, (type) => {
      doc.setStepType(ctx.sel.phase, ctx.sel.index, type);
      afterEdit();
    }),
    "How the step is judged. What it calls is its module.",
  );

  // TestStand's Adapter, under the name Anvil gives it. A step that calls
  // nothing shows <None>, exactly as TestStand does for a Statement.
  const names = doc.executorNames();
  if (CALLS_AN_EXECUTOR.includes(step.type) && step.module !== null && names.length > 0) {
    field(
      left,
      "Executor",
      select(names, step.executor ?? names[0], (v) => edit("executor", v)),
      "TestStand's Adapter: the declared executor that serves this step.",
    );
  } else {
    field(
      left,
      "Executor",
      disabledInput(names.length === 0 ? "<None declared>" : "<None>"),
      names.length === 0
        ? "TestStand's Adapter. This sequence declares no executor."
        : "TestStand's Adapter. This step calls no executor.",
    );
  }

  field(
    left,
    "Icon",
    disabledInput("<Adapter Icon>"),
    "Not implemented: a step carries no icon yet.",
  );

  // The button TestStand puts on its own, under Icon: it opens the step's
  // Attributes — the named values a step can carry for tooling to read, under
  // a namespace of your own (Company.Category.Attribute). Anvil has no such
  // thing, so the button is here and greyed, like everything else it has not
  // built yet.
  const attributes = document.createElement("button");
  attributes.type = "button";
  attributes.className = "icon-button";
  attributes.disabled = true;
  attributes.title =
    "TestStand opens the step's Attributes here. Anvil has no attributes, yet.";
  attributes.setAttribute("aria-label", "Attributes");
  attributes.append(
    svgIcon(
      `<path d="M4.5 11.5 8 6.5l3.5-2" fill="none" stroke="currentColor" stroke-width="1"/>
       <rect x="2" y="10.5" width="4" height="3.5" rx="0.6" fill="#d8a13a"/>
       <path d="M8 4.6 9.4 6 8 7.4 6.6 6z" fill="#2f6fbf"/>
       <path d="M12.8 2.2 14 3.4l-1.2 1.2-1.2-1.2z" fill="#2f6fbf"/>
       <path d="M12.6 7.4 13.8 8.6l-1.2 1.2-1.2-1.2z" fill="#2f6fbf"/>`,
    ),
  );
  left.append(attributes);

  // A box, not a line: TestStand's Description wraps, and what it says about a
  // step with a module is long enough to need it.
  const description = document.createElement("textarea");
  description.rows = 3;
  description.readOnly = true;
  description.className = "description";
  description.value = describeStep(step);
  field(
    right,
    "Description",
    description,
    "Written from the step itself, as TestStand writes it. Not editable.",
  );

  const comment = document.createElement("textarea");
  comment.rows = 6;
  comment.value = step.comment ?? "";
  comment.addEventListener("change", () =>
    edit("comment", comment.value.trim() || undefined),
  );
  field(
    right,
    "Comment",
    comment,
    "Free text for whoever reads the sequence. The engine never reads it and no verdict depends on it.",
  );
}

/** TestStand's Data Source: which value the step's verdict is taken from. */
function pageDataSource(body, ctx) {
  const { step, edit } = ctx;
  const fields = newFields(body);

  if (step.type === "pass_fail") {
    field(
      fields,
      "Condition",
      textInput(step.condition ?? "", (v) => edit("condition", v || undefined)),
      step.module
        ? "Decides the verdict instead of the module's status. Leave it empty to let the module decide."
        : "A boolean expression the engine evaluates. Required when the step calls no module.",
    );
  }
  if (step.type === "numeric_limit") {
    field(
      fields,
      "Value",
      textInput(step.value ?? "", (v) => edit("value", v || undefined)),
      "Which number to judge. Empty means the module's measurement.",
    );
  }
}

/** TestStand's Limits, in the shape ADR-0040 §7 took from it. */
function pageLimits(body, ctx) {
  const { step, doc, sel } = ctx;
  const fields = newFields(body);

  if (!step.limit) {
    note(
      fields,
      "This numeric_limit has no limit, and the loader requires one. Add `limit:` in the Text view, or insert the step again from the palette.",
    );
    return;
  }

  const setLimit = (key, v) => {
    doc.setStepLimit(sel.phase, sel.index, key, v);
    afterEdit();
  };

  // The comparison decides which fields the limit has, so it is chosen here
  // and the rest follows (ADR-0040 §7). It used to be the text view's job,
  // which meant a `numeric_limit` inserted from the palette — `comparison:
  // none` — could not be given a threshold at all without leaving the editor.
  field(
    fields,
    "Comparison",
    select(COMPARISON_CODES, step.limit.comparison ?? "none", (code) => {
      doc.setStepComparison(sel.phase, sel.index, code);
      afterEdit();
    }),
    COMPARISON_DOC[step.limit.comparison] ?? "How the number is judged.",
  );
  for (const key of ["low", "high", "nominal", "lower", "upper"]) {
    if (key in step.limit) {
      field(fields, key, numberInput(step.limit[key], (v) => setLimit(key, v)));
    }
  }
  if ("threshold" in step.limit) {
    field(
      fields,
      "threshold",
      select(["percent", "ppm", "delta"], step.limit.threshold, (v) =>
        setLimit("threshold", v),
      ),
      "Whether lower/upper are a percentage of nominal, parts per million, or an absolute amount.",
    );
  }
  field(
    fields,
    "units",
    textInput(step.limit.units ?? "", (v) => setLimit("units", v || undefined)),
    "For the report only: it does not scale or convert anything.",
  );
}

function pageRunOptions(body, ctx) {
  const { step, edit } = ctx;
  const fields = newFields(body);
  field(
    fields,
    "Disabled",
    checkbox(step.disable, (v) => edit("disable", v ? true : undefined)),
    "TestStand's Run Mode ▸ Skip: the engine records the step as skipped without calling it.",
  );
  parityNote(fields, "step.properties.run-options.run-mode");
}

function pageLooping(body, ctx) {
  const { step, edit } = ctx;
  const fields = newFields(body);
  field(
    fields,
    "Retries",
    numberInput(step.retries, (v) => edit("retries", v)),
    "How many attempts the step gets. Anvil's own, not TestStand's: it calls the step again while the step does not pass.",
  );
  parityNote(fields, "step.properties.looping.loop-types");
}

function pagePostActions(body, ctx) {
  const { step, edit } = ctx;
  const fields = newFields(body);
  field(
    fields,
    "Pause on fail",
    checkbox(step.pause_on_fail, (v) => edit("pause_on_fail", v ? true : undefined)),
    "Stops the phase if this step fails.",
  );
  parityNote(fields, "step.properties.post-actions.on-pass-on-fail");
}

/** The expressions the engine evaluates around the step. */
function pageExpressions(body, ctx) {
  const { step, doc, sel, edit } = ctx;
  const fields = newFields(body);

  if (step.type === "statement") {
    field(
      fields,
      "Statement",
      textInput(step.statement ?? "", (v) => edit("statement", v || undefined)),
      "Runs in the engine: assigns to a declared variable.",
    );
  }

  // `assign` is how a measurement outlives its step: `result.*` can only be
  // read where the step answered, so anything a later step needs is kept in a
  // variable here. Without this the only way to keep a reading was to type the
  // YAML.
  if (step.module !== null) {
    group(fields, "Store what it returns");
    const locals = Object.keys(doc.variables("locals"));
    for (const [target, expression] of Object.entries(step.assign ?? {})) {
      const row = document.createElement("div");
      row.className = "assign-row";
      row.append(
        textInput(expression, (v) => {
          doc.setStepAssign(sel.phase, sel.index, target, v);
          afterEdit();
        }),
        action("Remove", () => {
          doc.setStepAssign(sel.phase, sel.index, target, undefined);
          afterEdit();
        }),
      );
      field(fields, target, row, `Kept in locals.${target}.`);
    }
    if (locals.length === 0) {
      const p = document.createElement("p");
      p.className = "hint";
      p.textContent =
        "Declare a local variable first: an assign must name one the sequence declares.";
      fields.append(p);
    } else {
      const pick = select(
        locals.filter((l) => !(l in (step.assign ?? {}))),
        null,
        () => {},
      );
      if (pick.options.length > 0) {
        const add = document.createElement("div");
        add.className = "assign-row";
        add.append(
          pick,
          action("Keep the measurement", () => {
            doc.setStepAssign(sel.phase, sel.index, pick.value, "${result.measured_value}");
            afterEdit();
          }),
        );
        field(fields, "Add", add, "Stores result.measured_value; edit it for another field.");
      }
    }
  }

  parityNote(fields, "step.properties.expressions.pre-and-status");
}

function pagePreconditions(body, ctx) {
  const { step, edit } = ctx;
  const fields = newFields(body);
  field(
    fields,
    "Precondition",
    textInput(step.precondition ?? "", (v) => edit("precondition", v || undefined)),
    "If it is false the step is skipped, without spending an attempt.",
  );
  note(
    fields,
    "TestStand builds this with a dialog of checkboxes over the previous steps' results; Anvil writes the expression itself.",
  );
}

/**
 * A row of square icon buttons, as TestStand puts beside its path fields.
 *
 * Every one of them is disabled: none of what they do exists in Anvil yet.
 * They are here, in TestStand's arrangement, because that is what the Module
 * tab looks like and because each one names a thing still to build.
 */
function iconButtons(specs) {
  const group = document.createElement("div");
  group.className = "icon-row";
  for (const [label, glyph, onClick, busy] of specs) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "icon-button";
    // A button with something behind it works and says what it does; one
    // without stays greyed and says why, which is what it was drawn for.
    b.disabled = !onClick || Boolean(busy);
    b.title = onClick ? label : `${label}. Not implemented.`;
    b.setAttribute("aria-label", label);
    b.append(svgIcon(glyph));
    if (onClick) b.addEventListener("click", onClick);
    group.append(b);
  }
  return group;
}

// The glyphs, drawn rather than loaded. They stand for the same actions
// TestStand's icons stand for; they are not its artwork.
const GLYPH = {
  browse: `<path d="M1.5 4.5h4.2l1 1.4h7.8v7.6H1.5z" fill="#d8a13a"/>
           <circle cx="10.6" cy="9.4" r="2.6" fill="none" stroke="#333" stroke-width="1.2"/>
           <path d="m12.6 11.4 2.2 2.2" stroke="#333" stroke-width="1.4"/>`,
  edit: `<rect x="2" y="2" width="8.5" height="11" fill="#e9eef5" stroke="#4a6fa5"/>
         <path d="m9.5 11.5 4.3-4.3 1.6 1.6-4.3 4.3-2 .4z" fill="#d8a13a" stroke="#8a6a12" stroke-width=".7"/>`,
  create: `<rect x="2.5" y="2.5" width="8.5" height="11" fill="#e9eef5" stroke="#4a6fa5"/>
           <path d="M11.5 9.5v5M9 12h5" stroke="#2e8b57" stroke-width="1.8"/>`,
  reload: `<path d="M13 8a5 5 0 1 1-1.6-3.6" fill="none" stroke="#2f6fbf" stroke-width="1.6"/>
           <path d="M13.6 1.8v3.4h-3.4z" fill="#2f6fbf"/>`,
  tools: `<path d="M2 12.5 8 6.5l1.5 1.5-6 6z" fill="#8a8f96"/>
          <path d="m9.5 2.5 3.8 3.8-1.6 1.6-3.8-3.8z" fill="#c0392b"/>`,
  terminals: `<rect x="6" y="6" width="4" height="4" fill="none" stroke="currentColor" stroke-width="1.1"/>
              <path d="M1.5 5h4.5M1.5 11h4.5M10 8h4.5" stroke="currentColor" stroke-width="1.1"/>
              <circle cx="1.6" cy="5" r="1.3" fill="#2f6fbf"/>
              <circle cx="1.6" cy="11" r="1.3" fill="#2e8b57"/>
              <circle cx="14.4" cy="8" r="1.3" fill="#c0392b"/>`,
  help: `<path d="M5.8 6a2.2 2.2 0 1 1 2.6 2.2v1.4" fill="none" stroke="currentColor" stroke-width="1.4"/>
         <circle cx="8.4" cy="12.4" r="1" fill="currentColor"/>`,
  collapse: `<path d="m3 9 5-4 5 4M3 13l5-4 5 4" fill="none" stroke="currentColor" stroke-width="1.4"/>`,
  start: `<path d="M4.5 3.2 12.8 8l-8.3 4.8z" fill="#2e8b57"/>`,
  stop: `<rect x="4.2" y="4.2" width="7.6" height="7.6" fill="#c0392b"/>`,
};

/**
 * The Module tab: what the step calls, and with what.
 *
 * Laid out as TestStand's: the call and its two paths across the top, each
 * path showing what was written and, greyed beneath it, what that resolves to;
 * then the parameter table on the left and, on the right, the panel that says
 * what the thing being called looks like.
 *
 * TestStand's pair is Project Path and VI Path — the container, and the thing
 * inside it. Anvil's is the executor and the module: an executor is a
 * department of modules (ADR-0025) and a step names both (ADR-0027, ADR-0041).
 * So the two rows carry the same meaning in the same places.
 */
function renderModuleTab(body, ctx) {
  const { step, doc, edit } = ctx;

  const page = document.createElement("div");
  page.className = "page-module";
  body.append(page);

  const head = document.createElement("div");
  head.className = "fields module-head";
  page.append(head);

  if (step.type === "statement") {
    note(
      head,
      "This step calls no module: it runs in the engine. TestStand shows <None> as the adapter for a Statement.",
    );
    return;
  }

  if (step.type === "sequence_call") {
    field(
      head,
      "Call Type",
      disabledInput("Sequence Call"),
      "TestStand's Sequence Adapter, which a sequence_call always uses.",
    );
    field(
      head,
      "Sequence",
      pathRow(
        textInput(step.sequence ?? "", (v) => edit("sequence", v || undefined)),
        [["Browse for a sequence", GLYPH.browse]],
        [["Open the subsequence", GLYPH.edit]],
      ),
      "The subsequence this step calls, by name or by path.",
    );
    // Decía la frase «resolved against this file's directory» en vez de la
    // ruta. Esta línea existe para enseñar **contra qué** resolvió — decir el
    // método en lugar del resultado no atrapa la que resolvió donde no debía,
    // que es lo único para lo que sirve.
    resolved(head, sequenceResolved(step));
    // `args`, no `inputs`: un `sequence_call` cablea los `parameters` de la
    // subsecuencia, y son campos distintos del YAML a propósito (el mismo
    // bloque copiado de uno a otro cambiaba de significado en silencio). Con
    // `inputs` la tabla decía siempre «no declara inputs», incluso con
    // argumentos escritos.
    parameterTable(
      page,
      { ...step, inputs: step.args ?? step.inputs },
      "the argument table.",
      "A sequence_call's `args` wire the subsequence's parameters to locals. Only the Text view can write them today.",
    );
    return;
  }

  // What kind of call this is: in Anvil the executor decides, and there is one
  // kind of call to each — a step request over gRPC (ADR-0003).
  const executor = doc.executors().find((e) => e.name === step.executor) ?? null;
  field(
    head,
    "Call Type",
    disabledInput(executor ? `${executor.type} step call` : "step call"),
    "How the step reaches its executor. Anvil has one: a step request over gRPC (ADR-0003).",
  );

  // TestStand's Project Path, and the same job: choose the container before
  // choosing the thing inside it. Here the container is a department
  // (ADR-0025), and the list is what the sequence declares — not what the
  // machine has, which is a different question and does not have an answer yet
  // (ADR-0025 §Deferred).
  const nombres = doc.executors().map((e) => e.name);
  const elegirEjecutor = select(
    ["", ...nombres],
    step.executor ?? "",
    (v) => edit("executor", v || undefined),
  );
  elegirEjecutor.disabled = nombres.length === 0;
  // The button TestStand has no equivalent of, because TestStand's adapters
  // run in-process. Anvil's executor is a server, and on a development machine
  // somebody has to start it: `dev:` says how, and this does it (ADR-0046 §5).
  const arrancado = executor ? devOf(executor.name) : null;
  const arrancar = executor
    ? [
        arrancado ? `Stop '${executor.name}'` : `Start '${executor.name}' from its dev: entry`,
        arrancado ? GLYPH.stop : GLYPH.start,
        () => toggleDev(executor.name),
        state.devBusy === executor.name,
      ]
    : null;

  field(
    head,
    "Executor",
    pathRow(
      elegirEjecutor,
      [
        ["Declare an executor", GLYPH.create, () => declaraEjecutor()],
        ...(arrancar ? [arrancar] : []),
      ],
      [
        ["Declare a new executor", GLYPH.create],
        ["Edit this executor", GLYPH.edit],
      ],
    ),
    "TestStand's Project Path: the department the module lives in.",
  );
  resolved(head, executorResolved(doc, executor));
  // How the last start failed, or how a started executor died. It stays until
  // something else happens: a status line is gone by the time someone has read
  // the panel it was about.
  if (state.devError) resolved(head, state.devError);

  // TestStand's VI Path, and its button that picks a VI **belonging to the
  // project**. Here that is the executor's own catalog.
  //
  // Still a text field, with the list beside it rather than instead of it: an
  // executor that is not up has no catalog, and a name has to be typeable
  // anyway. Nobody should have to start a bench to write down a step's name.
  const modulos = modulesOf(step.executor);
  const moduleRow = document.createElement("div");
  moduleRow.className = "module-pick";
  moduleRow.append(textInput(step.module ?? "", (v) => edit("module", v || undefined)));
  if (modulos && modulos.length > 0) {
    const lista = select(["", ...modulos], modulos.includes(step.module) ? step.module : "", (v) => {
      if (v) edit("module", v);
    });
    lista.title = `The ${modulos.length} module(s) '${step.executor}' says it serves.`;
    moduleRow.append(lista);
  }

  field(
    head,
    "Module",
    pathRow(
      moduleRow,
      [
        ["Browse the executor's catalog", GLYPH.browse],
        ["Pick a module from the executor", GLYPH.terminals],
        ["Rename what this step calls", GLYPH.edit],
        ["Break on this step", GLYPH.tools],
      ],
      [
        ["Show the module's terminals", GLYPH.terminals],
        ["Edit the module", GLYPH.edit],
        [
          state.catalogBusy ? "Asking the executors…" : "Reload the catalog",
          GLYPH.reload,
          () => loadCatalog(),
          state.catalogBusy,
        ],
        ["Configure the call", GLYPH.tools],
        ["Create a module", GLYPH.create],
        ["Help on this module", GLYPH.help],
        ["Collapse this panel", GLYPH.collapse],
      ],
    ),
    step.type === "action"
      ? "TestStand's VI Path: what the step calls on its executor."
      : "TestStand's VI Path: what the step calls. Empty means it calls nothing.",
  );
  resolved(head, moduleResolved(step));

  if (step.module === null) return;

  const split = document.createElement("div");
  split.className = "module-split";
  page.append(split);

  parameterTable(
    split,
    step,
    "editing the parameter table.",
    "The engine already sends a step's `inputs` (ADR-0020), so two steps can call one module with different values. Only the Text view can write them today.",
  );
  modulePanel(split, step, executor);
}

// ---------------------------------------------------------------------------
// The catalog (ADR-0044).
//
// TestStand has five Module panels because it **inspects** five artefacts. It
// reads the connector pane of a `.vi`, the functions of a `.py`, the classes
// of an assembly — so each format needs its own panel and its own reader.
//
// Anvil has one, because the executor did the normalising: every department
// answers the same `Describe` with the same `StepSpec`, whatever is behind it.
// So the panel is the executor, the module, and the signature — three rows for
// every language.
//
// It is asked through `anvil describe`, not through the bridge. The bridge
// belongs to a sequence and needs the bench up; this is the case ADR-0028 was
// written for, where someone writes a sequence on a laptop with nothing
// running. In a plain browser there is no process to spawn, so the panel keeps
// saying what it cannot know — true of what the page can see.
// ---------------------------------------------------------------------------

/**
 * TestStand's *"or browse for a VI anywhere on the system"*, as ADR-0046 made
 * it mean.
 *
 * A step must name an executor (ADR-0041), and an executor is an address — so
 * there is no file to browse for here any more. Declaring one is naming where
 * it listens; bringing that address up is the `dev:` block's job, and starting
 * it is the editor's, not the sequence's.
 */
function declaraEjecutor() {
  const doc = state.doc;
  if (!doc) return;
  const ya = new Set(doc.executorNames());
  let nombre = "bench";
  let n = 1;
  while (ya.has(nombre)) nombre = `bench_${++n}`;
  doc.addExecutor(nombre, "127.0.0.1", 9101);

  const sel = state.selected;
  if (sel && CALLS_AN_EXECUTOR.includes(doc.steps(sel.phase)[sel.index]?.type)) {
    doc.setStepField(sel.phase, sel.index, "executor", nombre);
  }
  afterEdit();
  status("pass", `executor '${nombre}' declared at 127.0.0.1:9101 — point it where yours listens`);
}

// ---------------------------------------------------------------------------
// Bringing an executor up from `dev:` (ADR-0046 §5).
//
// Nothing starts an executor during a run — not the engine, not the host, not
// this editor. What this is, is the second terminal: on a development machine,
// while someone writes a sequence, the button does what they would otherwise
// type. The sequence is unchanged by it, which is the whole point of `dev:`
// being optional and ignored by the engine — a sequence that reaches a bench
// behaves identically whether the block is there or not.
//
// And it is what gives `Describe` something to ask. Until an executor is up,
// the Module panel can only say that nobody has been asked (ADR-0044).
// ---------------------------------------------------------------------------

/** What this editor started for `name`, or null. */
function devOf(name) {
  return state.dev[name] ?? null;
}

/**
 * Starts the executor `name` where the sequence says it listens — or stops the
 * one this editor started.
 *
 * Every way this cannot happen has its own sentence, because they are
 * different problems with different fixes: a browser has no processes, an
 * unsaved file has nothing for `code` to be relative to, a sequence without a
 * `dev:` entry is not saying how, and an address elsewhere is somebody else's
 * bench. The remaining ones — a runtime that is not installed, a `code` folder
 * that is not there — come back from the shell already naming where it looked.
 */
async function toggleDev(name) {
  if (state.devBusy) return;
  const doc = state.doc;
  if (!doc || !name) return;

  if (devOf(name)) {
    state.devBusy = name;
    renderStep();
    try {
      await window.anvil.stopDev(name);
    } finally {
      delete state.dev[name];
      state.devBusy = null;
      state.devError = null;
      renderStep();
    }
    status("pass", `'${name}' stopped`);
    return;
  }

  if (!inShell()) {
    state.devError = "a browser cannot start a process; open this in the desktop app";
    renderStep();
    status("fail", state.devError);
    return;
  }
  const path = state.handle?.path;
  if (!path) {
    state.devError = "save the sequence first: `dev:` names its code relative to the file";
    renderStep();
    status("fail", state.devError);
    return;
  }
  const dev = doc.devFor(name);
  if (!dev) {
    state.devError = `this sequence has no 'dev:' entry for '${name}' — it says where it listens, not how to bring it up`;
    renderStep();
    status("fail", state.devError);
    return;
  }
  const executor = doc.executors().find((e) => e.name === name);

  state.devBusy = name;
  state.devError = null;
  renderStep();
  try {
    const started = await window.anvil.startDev(path, {
      name,
      host: executor?.host,
      port: executor?.port,
      runtime: dev.runtime,
      code: dev.code,
    });
    state.dev[name] = started;
    status("pass", `'${name}' started at ${started.at} — ${started.runtime}, pid ${started.pid}`);
  } catch (e) {
    // The shell's own words: it is the half that knows the install folder and
    // where it looked in it.
    state.devError = String(e?.message ?? e);
    status("fail", state.devError);
  } finally {
    state.devBusy = null;
    renderStep();
  }

  // The reason to have started it. An executor that is up has a catalog, and
  // the Module panel has been saying nobody was asked.
  if (state.dev[name]) await loadCatalog();
}

/**
 * One of these died on its own. Nothing polls it, so this is the only way the
 * page finds out — and it has to, because the row would otherwise go on
 * offering to stop something that is gone, and the catalog beside it would be
 * about an executor that is no longer there.
 */
function devExited({ name, reason }) {
  if (!(name in state.dev)) return;
  delete state.dev[name];
  state.devError = reason;
  renderStep();
  status("fail", reason);
}

/**
 * The Executor row's greyed line: the address, and who is at it.
 *
 * The four answers are different and must stay different. An executor this
 * editor started is the only one it can say anything about; one with a `dev:`
 * entry is one it *could* start, and only in the desktop shell; and the last
 * case is the normal one on a bench, where somebody else put it there and the
 * editor has no business implying otherwise.
 */
function executorResolved(doc, executor) {
  if (!executor) return "this step names no executor";
  const at = `${executor.host}:${executor.port}`;
  const running = devOf(executor.name);
  if (running) return `${at} — started from here: ${running.runtime}, pid ${running.pid}`;
  const dev = doc.devFor(executor.name);
  if (dev && inShell()) return `${at} — 'dev:' can bring it up here with ${dev.runtime}`;
  if (dev) return `${at} — 'dev:' says a ${dev.runtime} executor serves it; a browser cannot start one`;
  return `${at} — whoever runs this bench started it`;
}

/**
 * stderr with the event stream taken out, for the Output tab.
 *
 * `--events` writes one JSON object per line to stderr (ADR-0029), and fd 2 is
 * shared with the engine's logs and the executors'. Put verbatim on the
 * Output tab, the NDJSON is most of what is there and it buries the lines
 * someone actually needs — a warning from an executor, a connection that was
 * retried. It is not dropped because it is unimportant: it is dropped here
 * because it has **already been read**, by the state machine that lit the rows
 * and filled the Call Stack. This tab is the other half, the part written for
 * a person.
 *
 * The test is the same one `applyEvent` uses, and inverted on purpose: a line
 * that parses as an object with an `event` is an event. Anything else is
 * someone talking, and it stays.
 */
function sinEventos(texto) {
  return texto
    .split("\n")
    .filter((linea) => {
      const t = linea.trim();
      if (!t.startsWith("{")) return true;
      try {
        const e = JSON.parse(t);
        return !(e && typeof e.event === "string");
      } catch {
        return true;
      }
    })
    .join("\n");
}

/** The catalog of one executor, or null if none was asked for or it declined. */
function catalogOf(name) {
  const e = state.catalog?.executors?.[name];
  return e && e.describes ? e : null;
}

/** The modules this executor serves, by name, or null when it is not known. */
function modulesOf(name) {
  const e = catalogOf(name);
  return e ? e.steps.map((s) => s.name) : null;
}

/** What the executor says this module takes and returns, or null. */
function specOf(executor, module) {
  return catalogOf(executor)?.steps.find((s) => s.name === module) ?? null;
}

/**
 * Asks the engine for the catalog of everything this sequence declares.
 *
 * Only in the desktop shell, and only for a file on disk: `describe` loads the
 * sequence the way a run does, so it needs the file and its neighbours to be
 * where they say they are. A document from File ▸ New has neither.
 */
async function loadCatalog() {
  if (state.catalogBusy) return;
  const path = inShell() ? state.handle?.path : null;
  if (!path) {
    state.catalogError = inShell()
      ? "save the sequence first: the catalog is asked of what its executors point at, relative to the file"
      : "a browser cannot start the engine; open this in the desktop app to ask the executors what they serve";
    renderStep();
    return;
  }

  state.catalogBusy = true;
  state.catalogError = null;
  renderStep();
  try {
    state.catalog = await window.anvil.describe(path);
    state.catalogError = null;
  } catch (e) {
    // The engine's own words. Rewording them would mean two sources for the
    // same message, and its is the one naming the executor and its address.
    state.catalog = null;
    state.catalogError = String(e?.message ?? e);
  } finally {
    state.catalogBusy = false;
    renderStep();
  }
}

/**
 * The greyed line under Module: what this step actually calls, and whether the
 * executor agrees that it serves it.
 *
 * The three answers are different and have to stay different. A module the
 * catalog knows is confirmed; one it does not know is a step that will not run
 * — the loader's `EntradaDesconocida` of `catalogo.rs`, caught here instead of
 * on the bench. And "nobody has been asked" is neither, so it says that rather
 * than implying the name is fine (ADR-0019, Rule 2).
 */
function moduleResolved(step) {
  if (!step.module) return "this step calls nothing";
  const donde = `${step.module} on '${step.executor ?? "?"}'`;
  if (!step.executor) return donde;
  const cat = catalogOf(step.executor);
  if (!cat) return `${donde} — not asked; the catalog would say whether it is served`;
  return specOf(step.executor, step.module)
    ? `${donde} — served`
    : `${donde} — '${step.executor}' does not serve it`;
}

/**
 * The greyed line under Sequence: what a `sequence_call` will actually open.
 *
 * A name is an inline subsequence of this same file; anything with a slash or
 * a sequence extension is a path, and the loader resolves it against the
 * file's own directory (`es_path`, mirrored in `neighbours.mjs`). Showing the
 * two differently is the point — the mistake this catches is a name that was
 * meant to be a path, which loads as "no such subsequence" and sends someone
 * looking for a typo in the wrong place.
 */
function sequenceResolved(step) {
  const target = step.sequence;
  if (!target) return "nothing named yet";
  if (!isPath(target)) {
    const declaradas = state.doc?.subsequenceNames() ?? [];
    return declaradas.includes(target)
      ? `inline subsequence '${target}' of this file`
      : `inline subsequence '${target}' — this file declares none by that name`;
  }
  const dir = state.filename ? state.filename.replace(/[^/\\]+$/, "") : "";
  return dir ? `${dir}${target.replace(/^\.\//, "")}` : `${target}, relative to this file`;
}

/** A field with its two rows of buttons, as TestStand hangs them off a path. */
function pathRow(input, near, far) {
  const row = document.createElement("div");
  row.className = "path-row";
  row.append(input, iconButtons(near));
  const spacer = document.createElement("span");
  spacer.className = "path-spacer";
  row.append(spacer, iconButtons(far));
  return row;
}

/**
 * The greyed line TestStand puts under a path: what the thing above resolves
 * to. A relative path means nothing without the place it is relative to, and
 * showing both is how you catch the one that resolved somewhere unexpected.
 */
function resolved(parent, text) {
  const line = document.createElement("p");
  line.className = "resolved";
  line.textContent = text;
  parent.append(line);
}

/** TestStand's parameter grid, kept recognisable while it is still read-only. */
/**
 * TestStand's parameter grid — and, with a catalog, no longer a guess.
 *
 * It used to read the type off the literal in the YAML with `typeof`, which
 * says what someone typed, not what the step takes: a `canal` written `"1"`
 * looked like a text parameter because it was written as one. Now the rows are
 * **what the executor declares** (ADR-0021), and the YAML supplies the values.
 *
 * Three things a row can be, and they must not look alike:
 *
 * - declared and given a value — ordinary;
 * - declared, **required** and empty — the step will not run (the engine's own
 *   `EntradaObligatoria`), so it is marked here instead of on the bench;
 * - written in the YAML and **not** in the catalog — `EntradaUnknown`: the
 *   executor would drop it and measure something else, which is why the engine
 *   calls it a finding and never a warning.
 *
 * With no catalog it falls back to the old shape and says so. Showing nothing
 * would be worse: a step that has inputs would look like one that has none.
 */
function parameterTable(parent, step, todo, detail) {
  const box = document.createElement("div");
  box.className = "fields param-box";
  parent.append(box);

  group(box, "Parameters");
  const table = document.createElement("div");
  table.className = "param-table";
  for (const heading of ["Parameter Name", "Type", "In/Out", "Value"]) {
    const h = document.createElement("span");
    h.className = "param-heading";
    h.textContent = heading;
    table.append(h);
  }

  const escritos = step.inputs ?? {};
  const spec = specOf(step.executor, step.module);
  const filas = filasDeParametros(spec, escritos);

  if (filas.length === 0) {
    const empty = document.createElement("span");
    empty.className = "param-empty";
    empty.textContent = spec
      ? `'${step.module}' takes no inputs.`
      : "This step declares no inputs.";
    table.append(empty);
  } else {
    for (const fila of filas) table.append(...celdasDeParametro(step, fila));
  }

  box.append(table);
  if (spec) {
    // The note that said only the Text view could write these is not true any
    // more for a step whose executor answered.
    if (spec.doc) note(box, spec.doc);
  } else {
    todoNote(box, todo, detail);
  }
}

/**
 * The rows of the table: what the executor declares, then anything written in
 * the YAML that it did not declare.
 *
 * The declared ones come first and in the executor's own order, which is the
 * order of the function's signature — the one whoever wrote the step chose.
 */
function filasDeParametros(spec, escritos) {
  if (!spec) {
    return Object.entries(escritos).map(([name, value]) => ({
      name,
      type: typeof value,
      value,
      tiene: true,
      estado: "unknown-catalog",
    }));
  }
  const filas = spec.inputs.map((p) => ({
    name: p.name,
    type: p.type,
    required: p.required,
    doc: p.doc,
    hasDefault: p.has_default,
    default: p.default,
    value: escritos[p.name],
    tiene: Object.hasOwn(escritos, p.name),
    estado: "declared",
  }));
  for (const [name, value] of Object.entries(escritos)) {
    if (spec.inputs.some((p) => p.name === name)) continue;
    filas.push({ name, type: "—", value, tiene: true, estado: "not-served" });
  }
  return filas;
}

/** One row's four cells. The Value one is editable; the rest are the truth. */
function celdasDeParametro(step, fila) {
  const celda = (texto, clase = "param-cell") => {
    const c = document.createElement("span");
    c.className = clase;
    c.textContent = texto;
    return c;
  };

  const nombre = celda(fila.name);
  if (fila.estado === "not-served") {
    // The executor would drop it and measure something else. That is a finding
    // in the engine, never a warning, so it is not a quiet grey here either.
    nombre.dataset.param = "not-served";
    nombre.title = `'${step.executor}' does not declare '${fila.name}'. It would be ignored, and the step would measure something other than what this says.`;
  } else if (fila.required && !fila.tiene) {
    nombre.dataset.param = "missing";
    nombre.title = `'${fila.name}' is required and has no value: the step will not run.`;
  } else if (fila.doc) {
    nombre.title = fila.doc;
  }

  const tipo = celda(fila.type);
  if (fila.type === "unspecified") {
    // Proto3's default, so it is also what an executor that said nothing
    // produces. Unchecked, never guessed.
    tipo.title = "The executor did not say. Nothing checks this one.";
  }

  const dir = celda(fila.required === false ? "in (optional)" : "in");
  if (fila.hasDefault) {
    dir.title = `Defaults to ${JSON.stringify(fila.default)} if left empty. The step applies it, not the engine (ADR-0021 §5).`;
  }

  return [nombre, tipo, dir, celdaDeValor(step, fila)];
}

/**
 * The Value cell: the one thing in this table someone writes.
 *
 * Editable only for a step that is part of the open sequence — a `sel` — and
 * written with its type, by the loader's own rule: `4.5` is a number, `true` a
 * boolean, the rest text. Same inference the engine makes reading the file
 * back, and the same field the Variables pane uses, so the decimal separator
 * trap is avoided the same way: a text input, never `type="number"`, because a
 * number input renders through the browser's locale and on a Spanish machine
 * `4.5` would show as `4,5` while the YAML says `4.5`. In a test sequencer
 * that is the difference between 4.5 V and 45 V.
 *
 * Emptying it removes the input rather than writing `""`: a parameter that is
 * not there takes the step's own default, and one written empty does not.
 */
function celdaDeValor(step, fila) {
  const c = document.createElement("span");
  c.className = "param-cell param-value";
  const sel = state.selected;
  const escribible = sel && fila.estado !== "unknown-catalog";

  if (!escribible) {
    c.textContent = fila.tiene ? String(fila.value) : "";
    return c;
  }

  const input = textInput(fila.tiene ? scalarText(fila.value) : "", (raw) => {
    const t = raw.trim();
    state.doc.setStepInput(sel.phase, sel.index, fila.name, t === "" ? undefined : parseScalar(t));
    afterEdit();
  });
  input.className = "v";
  input.placeholder = fila.hasDefault ? scalarText(fila.default) : fila.required ? "required" : "";
  if (fila.required && !fila.tiene) input.dataset.param = "missing";
  c.append(input);
  return c;
}

/**
 * The panel TestStand fills with the VI: its project and name, its connector
 * pane, and the documentation off the VI itself.
 *
 * Anvil's is the executor, the module, and what `Describe` says it takes and
 * returns (`StepSpec`, ADR-0021) — the same three things. The terminals used
 * to be drawn from the YAML's `inputs` on the left and the hard-coded pair
 * `measured value` / `status` on the right, because nothing had asked. Now the
 * right-hand side is the module's real outputs, and when nobody has been asked
 * the panel says exactly that instead of showing a shape that looks like an
 * answer.
 */
function modulePanel(parent, step, executor) {
  const panel = document.createElement("div");
  panel.className = "module-panel";
  parent.append(panel);

  const spec = specOf(step.executor, step.module);

  const project = document.createElement("div");
  project.className = "module-project";
  project.textContent = executor?.name ?? "no executor";
  panel.append(project);

  const name = document.createElement("div");
  name.className = "module-name";
  name.textContent = step.module ?? "";
  panel.append(name);

  // The connector pane: what goes in on the left, what comes out on the right,
  // around the thing being called.
  const pane = document.createElement("div");
  pane.className = "connector";

  const terminales = (clase, nombres, vacio) => {
    const d = document.createElement("div");
    d.className = `terminals ${clase}`;
    for (const t of nombres.length > 0 ? nombres : [vacio]) {
      const s = document.createElement("span");
      s.textContent = t;
      // Greyed means "not known", never "none": a module that genuinely takes
      // nothing is a fact, and it should not look like an unanswered question.
      if (nombres.length === 0) s.className = "unknown";
      d.append(s);
    }
    return d;
  };

  const ins = spec
    ? terminales("in", spec.inputs.map((p) => p.name), "(no inputs)")
    : terminales("in", Object.keys(step.inputs ?? {}), "(inputs)");

  const box = document.createElement("div");
  box.className = "connector-box";
  box.textContent = (step.module ?? "").split("/").pop() ?? "";

  const outs = spec
    ? terminales("out", spec.outputs.map((o) => o.name), "(no outputs)")
    : terminales("out", [], "(outputs)");

  pane.append(ins, box, outs);
  panel.append(pane);

  const doc = document.createElement("p");
  doc.className = "module-doc";
  if (spec) {
    doc.textContent = spec.doc || `'${step.module}' carries no documentation.`;
    doc.title = `Answered by '${step.executor}' (ADR-0021). Reload the catalog to ask again.`;
  } else if (state.catalogBusy) {
    doc.textContent = "Asking the executors…";
  } else if (state.catalogError) {
    // The engine's own words, or the reason there was nothing to ask.
    doc.textContent = state.catalogError;
  } else if (catalogOf(step.executor)) {
    doc.textContent = `'${step.executor}' did not name a module called '${step.module}'.`;
  } else {
    doc.textContent = "Not asked yet: use Reload the catalog to ask the executors what they serve.";
    doc.title =
      "What a module takes and returns, and what it is for, is what its executor answers to Describe (StepSpec in paso.proto, ADR-0021). The editor asks with `anvil describe` (ADR-0044), which needs no bridge and no bench.";
  }
  panel.append(doc);
}


function renderStep() {
  // The menu is anchored to a button this call is about to throw away, so it
  // would otherwise float over the panel pointing at nothing.
  closeTypeMenu();
  ui.stepEditor.replaceChildren();
  const sel = state.selected;
  const doc = state.doc;

  if (!doc || !sel) {
    ui.stepTitle.textContent = "Step Settings";
    const p = document.createElement("p");
    p.className = "empty";
    p.textContent = doc ? "Select a step." : "Open a sequence to begin.";
    ui.stepEditor.append(p);
    return;
  }

  const step = doc.steps(sel.phase)[sel.index];
  if (!step) {
    state.selected = null;
    return renderStep();
  }

  ui.stepTitle.textContent = `Step Settings for ${step.name ?? "(unnamed)"}`;

  const edit = (key, value) => {
    doc.setStepField(sel.phase, sel.index, key, value);
    afterEdit();
  };
  const ctx = { step, doc, sel, edit };

  const tabs = document.createElement("div");
  tabs.className = "step-tabs";
  tabs.setAttribute("role", "tablist");
  const available = tabsFor(step.type);
  // The tab strip changes with the type, so a tab the new type does not have
  // must not stay selected — Limits is gone the moment a numeric_limit becomes
  // an action.
  if (!available.some(([key]) => key === state.stepTab)) state.stepTab = "properties";
  for (const [key, label] of available) {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = "step-tab";
    tab.setAttribute("role", "tab");
    tab.setAttribute("aria-selected", String(state.stepTab === key));
    tab.textContent = label;
    tab.addEventListener("click", () => {
      state.stepTab = key;
      renderStep();
    });
    tabs.append(tab);
  }
  ui.stepEditor.append(tabs);

  const body = document.createElement("div");
  body.className = "step-body";
  ui.stepEditor.append(body);

  if (state.stepTab === "module") {
    renderModuleTab(body, ctx);
  } else if (state.stepTab === "data-source") {
    pageDataSource(body, ctx);
  } else if (state.stepTab === "limits") {
    pageLimits(body, ctx);
  } else {
    const pages = PROPERTY_PAGES.filter(
      (page) => !page.types || page.types.includes(step.type),
    );
    // A page that the current type does not have, or that Anvil cannot fill,
    // must not stay selected when the type changes underneath it.
    if (!pages.some((page) => page.label === state.stepPage && page.state === "built")) {
      state.stepPage = "General";
    }

    const nav = document.createElement("nav");
    nav.className = "prop-pages";
    for (const page of pages) {
      const item = document.createElement("button");
      item.type = "button";
      item.className = "prop-page";
      item.textContent = page.label;
      // Greyed out, not left off: the page is where TestStand puts it, and the
      // tooltip says what TestStand does there and where Anvil stands — which
      // is not the same sentence for a debt, another road and a decision
      // (ADR-0043 §4).
      item.disabled = page.state !== "built";
      item.dataset.parity = page.state;
      item.title = gapTooltip(page) ?? `${page.label} settings for this step`;
      if (page.label === state.stepPage) item.setAttribute("aria-current", "true");
      item.addEventListener("click", () => {
        state.stepPage = page.label;
        renderStep();
      });
      nav.append(item);
    }

    const content = document.createElement("div");
    content.className = "prop-content";
    body.append(nav, content);
    pages.find((page) => page.label === state.stepPage)?.render?.(content, ctx);
  }

  // Moving and deleting a step are not step properties — TestStand keeps them
  // on the toolbar and the context menu — so they stay outside the tabs, where
  // they do not disappear when a page changes.
  const actions = document.createElement("div");
  actions.className = "step-actions";
  actions.append(
    action("Move up", () => {
      const to = doc.moveStep(sel.phase, sel.index, -1);
      state.selected = { phase: sel.phase, index: to };
      afterEdit();
    }, sel.index === 0),
    action("Move down", () => {
      const to = doc.moveStep(sel.phase, sel.index, +1);
      state.selected = { phase: sel.phase, index: to };
      afterEdit();
    }, sel.index === doc.steps(sel.phase).length - 1),
    action("Delete", () => {
      doc.removeStep(sel.phase, sel.index);
      state.selected = null;
      afterEdit();
    }),
  );
  ui.stepEditor.append(actions);
}


function action(label, onClick, disabled = false) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "action";
  b.textContent = label;
  b.disabled = disabled;
  b.addEventListener("click", onClick);
  return b;
}

function renderVariables() {
  ui.variables.replaceChildren();
  if (!state.doc) return;
  const doc = state.doc;

  renderSequenceFields(doc);
  renderExecutors(doc);

  // TestStand's column headings for this pane. The type is not editable and
  // not stored: it is what the loader will read the scalar as, shown so that
  // `4.5` and `"4.5"` stop looking like the same declaration (RF-31).
  const heads = document.createElement("div");
  heads.className = "var-head";
  for (const h of ["Name", "Value", "Type"]) {
    const c = document.createElement("span");
    c.textContent = h;
    heads.append(c);
  }
  ui.variables.append(heads);

  for (const scope of SCOPES) {
    const vars = doc.variables(scope);
    const names = Object.keys(vars);

    const head = document.createElement("div");
    head.className = "scope";
    // As TestStand names them: the scope, and whose it is.
    head.textContent = `${SCOPE_LABELS[scope]} ('${doc.name ?? NEW_SEQUENCE_NAME}')`;
    ui.variables.append(head);

    for (const name of names) {
      const row = document.createElement("div");
      row.className = "var";

      // The name is editable in place: renaming through delete-and-add would
      // move the declaration to the end of its scope, and the file is read in
      // diffs.
      const k = textInput(name, (v) => {
        if (!v || v === name) return;
        try {
          doc.renameVariable(scope, name, v);
        } catch (e) {
          status("fail", e.message);
          return;
        }
        afterEdit();
      });
      k.className = "k";

      // What is typed decides the type, exactly as the loader reads the
      // scalar: `true` is a bool, `4.5` a number, the rest text (RF-31). A
      // number field would make `4.5` unreachable on a Spanish locale, and a
      // text field would turn every variable into a string.
      const v = textInput(scalarText(vars[name]), (raw) => {
        doc.setVariable(scope, name, parseScalar(raw));
        afterEdit();
      });
      v.className = "v";

      const t = document.createElement("span");
      t.className = "t";
      t.textContent = scalarType(vars[name]);

      row.append(k, v, t, action("Remove", () => {
        doc.removeVariable(scope, name);
        afterEdit();
      }));
      ui.variables.append(row);
    }

    const add = document.createElement("button");
    add.type = "button";
    add.className = "action add-var";
    add.textContent = `Declare a ${scope.replace(/s$/, "")}`;
    add.addEventListener("click", () => {
      let n = 1;
      let name = "new_variable";
      while (name in doc.variables(scope)) name = `new_variable_${++n}`;
      doc.setVariable(scope, name, 0);
      afterEdit();
    });
    ui.variables.append(add);
  }

  // The fourth scope TestStand has, and what this pane cannot show yet.
  parityRow(ui.variables, "variables.station-globals");
  parityRow(ui.variables, "variables.live-values");
}

/** What each scope is called in TestStand's Variables pane. */
const SCOPE_LABELS = {
  locals: "Locals",
  parameters: "Parameters",
  file_globals: "FileGlobals",
};

/**
 * The type the loader will read this scalar as (RF-31).
 *
 * Read off the value rather than stored, because the YAML has no type
 * declaration: what is typed decides it. Showing it is how `4.5` and `"4.5"`
 * stop looking like the same line — which in a test sequencer is the
 * difference between a number and a label.
 */
function scalarType(value) {
  if (value === null || value === undefined) return "";
  const t = typeof value;
  if (t === "boolean") return "Boolean";
  if (t === "number") return "Number";
  return "String";
}

/** The sequence's own fields. Its name is what the report is headed with. */
function renderSequenceFields(doc) {
  const head = document.createElement("div");
  head.className = "scope";
  head.textContent = "sequence";
  ui.variables.append(head);

  const row = document.createElement("div");
  row.className = "var";
  const n = textInput(doc.name ?? "", (v) => {
    if (!v) return;
    doc.setName(v);
    afterEdit();
  });
  n.className = "v";
  row.append(n);
  ui.variables.append(row);
}

/**
 * The executors the sequence declares.
 *
 * Without one, no step can call anything and the palette refuses half its
 * types (ADR-0041) — so a sequence started from File ▸ New could not be
 * completed in the editor at all until this existed.
 */
function renderExecutors(doc) {
  const head = document.createElement("div");
  head.className = "scope";
  head.textContent = "executors";
  ui.variables.append(head);

  for (const ex of doc.executors()) {
    const row = document.createElement("div");
    row.className = "var";

    const name = textInput(ex.name, (v) => {
      if (!v || v === ex.name) return;
      doc.setExecutorField(ex.name, "name", v);
      afterEdit();
    });
    name.className = "k";

    // `wasm` is the executor binary by path, with its modules beside it;
    // `grpc` is a process of your own on a host and a port (ADR-0025, ADR-0027).
    const where =
      ex.type === "grpc"
        ? textInput(`${ex.host ?? ""}:${ex.port ?? ""}`, (v) => {
            // Sin guarda, un valor sin `:` dejaba `Number(undefined)` — un
            // `port: .nan` escrito en el fichero. El motor lo rechaza en el
            // siguiente validate, pero para entonces ya está en el YAML, y
            // `.nan` no le dice nada a nadie.
            const [host, port] = v.split(":");
            const n = Number(port);
            if (!host?.trim() || !Number.isInteger(n) || n <= 0 || n > 65535) {
              status("fail", `'${v}' is not host:port — 127.0.0.1:9101, for instance`);
              renderVariables();
              return;
            }
            doc.setExecutorField(ex.name, "host", host.trim());
            doc.setExecutorField(ex.name, "port", n);
            afterEdit();
          })
        : textInput(ex.path ?? "", (v) => {
            doc.setExecutorField(ex.name, "path", v);
            afterEdit();
          });
    where.className = "v";
    where.title = ex.type === "grpc" ? "host:port" : "path to the executor binary";

    row.append(name, where, action("Remove", () => {
      doc.removeExecutor(ex.name);
      afterEdit();
    }));
    ui.variables.append(row);
  }

  // One kind since ADR-0046, so there is nothing to choose: the dropdown that
  // used to pick `wasm` or `grpc` had one option left.
  const add = document.createElement("div");
  add.className = "assign-row add-var";
  add.append(
    action("Declare an executor", () => {
      let n = 1;
      let name = "bench";
      while (doc.executorNames().includes(name)) name = `bench_${++n}`;
      doc.addExecutor(name, "127.0.0.1", 9101);
      afterEdit();
    }),
  );
  ui.variables.append(add);
}

/** A scalar as it should appear in the file, which is how it is shown. */
function scalarText(value) {
  return typeof value === "string" ? value : String(value);
}

/**
 * The type a typed scalar has, by the loader's rule (RF-31): `true`/`false` is
 * a bool, a number is a number, anything else is text. Inferring it here rather
 * than asking is what makes a variable one field instead of a form — and it is
 * the same inference the engine does when it reads the file back.
 */
function parseScalar(raw) {
  const t = raw.trim();
  if (t === "true") return true;
  if (t === "false") return false;
  if (/^[+-]?(\d+\.?\d*|\.\d+)$/.test(t)) return Number(t);
  return raw;
}

function renderText() {
  if (!state.doc) return;
  const current = state.text?.state.doc.toString();
  if (current === state.doc.text) return;

  if (state.text) state.text.destroy();
  state.text = new EditorView({
    parent: ui.textEditor,
    state: EditorState.create({
      doc: state.doc.text,
      extensions: [
        basicSetup,
        yamlLang(),
        EditorView.updateListener.of((u) => {
          if (!u.docChanged) return;
          state.doc.setText(u.state.doc.toString());
          state.dirty = true;
          renderAll({ skipText: true });
          scheduleValidate();
        }),
      ],
    }),
  });
}

function setView(view) {
  state.view = view;
  const steps = view === "steps";
  // The text view replaces the whole middle column — sequence and settings
  // both — rather than just the lower pane: it is the same document seen
  // another way, not a third panel.
  ui.sequencePane.hidden = !steps;
  ui.stepPane.hidden = !steps;
  ui.textPane.hidden = steps;
  for (const b of document.querySelectorAll(".views button")) {
    b.setAttribute("aria-selected", String(b.dataset.view === view));
  }
  if (!steps) renderText();
}

// The shell holds a downloaded update back while either is true
// (`offerRestart` in electron/main.mjs): restarting under a run leaves the
// bench wherever it was. The name is what the shell's unsaved-changes question
// at close calls the document (`guardUnsaved`).
function reportWork() {
  window.anvil?.setWorkState({
    running: state.runInFlight,
    dirty: state.dirty,
    name: state.filename ?? NEW_SEQUENCE_NAME,
  });
}

function renderAll({ skipText = false } = {}) {
  reportWork();
  ui.panes.dataset.stale = String(state.doc?.stale ?? false);
  ui.filename.textContent = state.filename ?? (state.doc ? NEW_SEQUENCE_NAME : "no file");
  ui.filename.dataset.dirty = String(state.dirty);
  const boton = runButton({
    unavailable: state.runUnavailable,
    hasDoc: !!state.doc,
    bridged: engine.bridged,
    inFlight: state.runInFlight,
  });
  ui.run.disabled = boton.disabled;
  ui.run.title = boton.title;
  ui.sequenceTitle.textContent = state.doc?.name
    ? `Steps — ${state.doc.name}`
    : "Steps";
  renderPalette();
  renderTemplates();
  renderSequences();
  renderSequence();
  renderStep();
  renderVariables();
  renderExecution();
  renderOutput();
  renderStatusFields();
  if (!skipText && state.view === "text") renderText();
}

/**
 * TestStand's status bar fields, in TestStand's order.
 *
 * Model, Step Selected and Number of Steps are Anvil's own; User and
 * Environment are greyed, because Anvil has no notion of who is running it and
 * configures a run from CLI switches rather than from a station environment.
 */
function renderStatusFields() {
  ui.statusFields.replaceChildren();
  const doc = state.doc;

  for (const entry of STATUS_BAR) {
    const span = document.createElement("span");
    span.className = "status-field";
    span.dataset.parity = entry.state;

    if (entry.state !== "built") {
      span.textContent = `${entry.label}: —`;
      span.title = gapTooltip(entry);
      ui.statusFields.append(span);
      continue;
    }

    if (entry.id === "status.model") {
      // `--process-model` is what wraps a sequence in a model; the editor does
      // not pass one, so a run here is the sequence on its own.
      span.textContent = "Model: <None>";
      span.title = "Anvil's process model is a wrapper sequence (ADR-0016), passed with --process-model. The editor runs the sequence on its own.";
    } else if (entry.id === "status.selected") {
      // TestStand's own wording, index and all: `1 Step Selected [0]`.
      span.textContent = state.selected
        ? `1 Step Selected [${state.selected.index}]`
        : "No Steps Selected";
    } else {
      const n = doc ? PHASES.reduce((t, ph) => t + doc.steps(ph).length, 0) : 0;
      span.textContent = `Number of Steps: ${n}`;
    }
    ui.statusFields.append(span);
  }
}

/**
 * TestStand's execution toolbar: Run, and everything Anvil cannot do yet.
 *
 * This is the loudest thing the inventory produces, and it is meant to be.
 * Five controls, and four of them wait on the same missing piece of the
 * engine — stop, look, resume. Terminate waits on cancellation that still runs
 * cleanup, which is the risk `editor/README.md` already names: killing the
 * worker mid-sequence leaves the bench exactly as it was, with no cleanup run.
 *
 * They are drawn where TestStand draws them rather than left off, so that what
 * is missing is visible at the moment someone would reach for it.
 */
const EXEC_TOOLBAR = [
  "execution.break",
  "execution.resume",
  "execution.step-into",
  "execution.terminate",
  "execution.abort",
];

function renderExecControls() {
  // Run is in the HTML and owns its own wiring; everything after it is built
  // from the inventory.
  for (const node of [...ui.execControls.children]) {
    if (node !== ui.run) node.remove();
  }
  for (const id of EXEC_TOOLBAR) {
    const entry = parityById(id);
    const b = document.createElement("button");
    b.type = "button";
    b.className = "exec";
    b.dataset.parity = entry.state;
    b.textContent = entry.label;
    b.disabled = true;
    b.title = gapTooltip(entry);
    ui.execControls.append(b);
  }
}

/**
 * The Execution pane: TestStand's execution window, as much of it as the
 * engine's event stream supports.
 *
 * The Call Stack is real and always was possible: `--events` has carried
 * `parent_run_id` and `depth` on every line since ADR-0033, and nothing read
 * them. Watch and Breakpoints are greyed, and they say what they wait on.
 */
function renderExecution() {
  ui.execution.replaceChildren();

  const head = document.createElement("div");
  head.className = "scope";
  head.textContent = "Call Stack";
  ui.execution.append(head);

  if (!state.run) {
    const idle = document.createElement("p");
    idle.className = "hint";
    idle.textContent = "Nothing has run yet. Run the sequence and the stack fills in as it goes.";
    ui.execution.append(idle);
  } else {
    const { frames, truncated } = callStack(state.run);
    if (truncated) {
      // A stack that quietly begins halfway up reads as the whole truth
      // (ADR-0019, Rule 2).
      const note = document.createElement("p");
      note.className = "todo";
      note.textContent = "…the line naming the caller was lost, so this stack is partial.";
      ui.execution.append(note);
    }
    if (frames.length === 0 && !truncated) {
      const idle = document.createElement("p");
      idle.className = "hint";
      idle.textContent = state.run.done ? "The run finished." : "Waiting for the first step.";
      ui.execution.append(idle);
    }
    for (const frame of frames) {
      const row = document.createElement("div");
      row.className = "stack-frame";
      row.style.setProperty("--depth", String(frame.depth));
      const name = document.createElement("span");
      name.className = "k";
      name.textContent = frame.name;
      const where = document.createElement("span");
      where.className = "v";
      where.textContent = `${frame.sequence ?? "?"} · ${frame.phase ?? ""}`;
      row.append(name, where);
      ui.execution.append(row);
    }
  }

  parityRow(ui.execution, "execution.watch");
  parityRow(ui.execution, "execution.breakpoints");
}

/** Templates and IO Configurations: TestStand has both, Anvil neither. */
function renderTemplates() {
  ui.templates.replaceChildren();
  parityRow(ui.templates, "window.palette.templates");
  ui.ioConfigurations.replaceChildren();
  parityRow(ui.ioConfigurations, "window.palette.io-configurations");
}

/**
 * The Output tab: what the run printed.
 *
 * `run()` used to send this to the console with a note saying it would live
 * here one day — "the full report goes to the console until there is somewhere
 * to put it". This is that somewhere, and it is where TestStand puts it.
 */
function renderOutput() {
  ui.output.replaceChildren();
  const pre = document.createElement("pre");
  pre.className = "output";
  pre.textContent = state.output ?? "Nothing has run yet.";
  ui.output.append(pre);
}

function afterEdit() {
  state.dirty = true;
  renderAll();
  scheduleValidate();
}

// ---------------------------------------------------------------- validation

function status(stateName, text) {
  ui.statusLight.dataset.state = stateName;
  ui.statusText.dataset.state = stateName;
  ui.statusText.textContent = text;
  // The status bar is where this editor says what went wrong, so it is also
  // the trail worth having off-screen: inside the shell this reaches the
  // process's stdout (ADR-0037 §e), and in a browser it is one console line.
  console.log(`[status:${stateName}] ${text}`);
}

function scheduleValidate() {
  clearTimeout(state.validateTimer);
  state.validateTimer = setTimeout(validate, 250);
}

async function validate() {
  const doc = state.doc;
  if (!doc) return;

  // While the text does not parse, the engine has nothing useful to say about
  // it — the YAML error is the answer, and it is more precise than anything a
  // second parser would produce (AP-12).
  if (doc.stale) {
    const at = doc.error?.line ? ` (line ${doc.error.line})` : "";
    status("stale", `not valid YAML${at}: ${doc.error?.message ?? "parse error"}`);
    return;
  }

  status("busy", "validating…");
  const name = state.filename ?? NEW_SEQUENCE_NAME;
  try {
    const { exitCode, stderr } = await engine.run({
      args: [name, "--validate"],
      files: await filesFor(name, doc.text),
    });
    // The engine's own diagnostics, verbatim. Rewriting them here would mean
    // two sources for the same message, and the loader's is the one with the
    // field name, the location and the suggestion
    // (crates/cargador/src/lib.rs:686-763).
    const last = stderr.trim().split("\n").filter(Boolean).pop() ?? "";
    const verdict = last || (exitCode === 0 ? "valid" : "rejected");
    const run = state.runUnavailable ? ` — Run is unavailable: ${state.runUnavailable}` : "";
    // A sequence built with File ▸ New is rejected for a reason the loader
    // cannot know: its paths are relative to a file it does not have yet.
    const hint = exitCode === 0 ? null : unsavedPathHint(Boolean(state.handle), last);
    status(exitCode === 0 ? "pass" : "fail", verdict + (hint ? ` — ${hint}` : "") + run);
  } catch (e) {
    // A host failure is not a verdict about the sequence, and must not read as
    // one (ADR-0019, Rule 2). It gets its own colour, not "fail".
    const what = e instanceof EngineHostError ? e.message : String(e?.message ?? e);
    status("error", `editor could not run the engine: ${what}`);
  }
}

// ---------------------------------------------------------------- running

/**
 * Runs the open sequence for real.
 *
 * The engine invokes steps through the bridge, so this reaches actual
 * executors and, through them, actual hardware.
 *
 * Progress is live: `--events` makes the engine narrate the run as NDJSON on
 * stderr (ADR-0029, ADR-0033), and each line reaches this thread as it is
 * written, so the row being executed lights up while it runs. The rows are found
 * by the `locator`, which is a **hint** at where a step sits in the file — the
 * identity on the wire is the execution, and a hint is all a row needs. What is
 * never inferred here is a verdict: that is the report and the exit code.
 */
async function run() {
  if (!state.doc || !engine.bridged) return;

  const name = state.filename ?? NEW_SEQUENCE_NAME;
  status("busy", `running ${name}…`);
  state.runInFlight = true;
  reportWork();
  state.run = newRunState();
  state.output = null;
  ui.run.disabled = true;
  // TestStand brings the execution forward when a run starts; there is no
  // second window here, so the docked pane switches to it.
  setRightTab("execution");
  renderSequence();
  renderExecution();

  try {
    const { exitCode, stdout, stderr } = await engine.run({
      // The bridge's arguments come first, the sequence last, matching how the
      // native host builds argv (main.rs:604-608). `--events` is what makes the
      // run visible while it happens.
      args: [...engine.engineArgs, "--events", name],
      files: await filesFor(name, state.doc.text),
      onLine: onEvent,
    });
    // The verdict is the console sink's frozen header, `=== name: state ===`,
    // and it goes to **stdout** (crates/result_sink/src/consola.rs); stderr
    // carries the engine's logs. Taking the last line of the two concatenated
    // showed "4 paso(s) comprobados contra el catálogo de su ejecutor" — a
    // start-up note — as though it were the result.
    //
    // The text is the engine's own, never re-worded here: two renderings of one
    // verdict drift, and then the editor and the CLI disagree (ADR-0031).
    const report = `${stdout}\n${sinEventos(stderr)}`.trim();
    const verdict =
      stdout
        .split("\n")
        .map((l) => l.trim())
        .filter((l) => l.startsWith("===") && l.endsWith("==="))
        .pop() ??
      stdout.split("\n").filter(Boolean).pop() ??
      `finished with exit ${exitCode}`;

    if (state.run) {
      state.run.current = null;
      state.run.nested = null;
    }
    // A stream that lost lines leaves rows that never got their verdict. Saying
    // so beats a view that looks complete and is not.
    const perdidas = state.run?.lost
      ? ` (${state.run.lost} event line(s) lost — some rows may be blank)`
      : "";
    status(exitCode === 0 ? "pass" : "fail", verdict + perdidas);
    // The report lands on the Output tab, where TestStand puts it. The console
    // keeps it too: inside the shell that reaches the process's stdout
    // (ADR-0037 §e), which is the trail worth having when the window is gone.
    state.output = report;
    console.log(report);
  } catch (e) {
    const what = e instanceof EngineHostError ? e.message : String(e?.message ?? e);
    status("error", `could not run: ${what}`);
    state.output = `could not run: ${what}`;
  } finally {
    state.runInFlight = false;
    renderAll();
  }
}

/**
 * Says the bridge is gone, and stops offering what needs it.
 *
 * The pool has already cleared `bridged` by the time this runs, so repainting
 * is what takes Run away. Saying nothing and leaving the button armed would
 * offer a run that cannot happen — and the failure would only show up on the
 * next click, worded as if the sequence were at fault.
 */
function bridgeLost(reason) {
  const expected = state.bridge === null;
  state.bridge = null;
  // Not while a run is on screen: that run's own outcome is the more useful
  // thing to be looking at, and `run()` reports the failure itself. Nor when
  // the editor stopped the bridge itself (`newFile`): that is not news, and
  // it would land on top of what the engine says about the new document.
  if (!state.runInFlight && !expected) status("error", `${reason} — Run is no longer available`);
  renderAll();
}

/**
 * Connects to a bridge given as `?bridge=<ws url>`.
 *
 * That URL is what `anvil <sequence.yaml> --bridge` prints, token included.
 * Without it the editor still opens, edits and validates — none of which needs
 * a socket — and Run stays disabled saying why.
 */
async function openBridge(url) {
  status("busy", "connecting to the bridge…");
  try {
    await engine.attachBridge(url, connectBridge, bridgeLost);
    state.bridge = url;
    state.runUnavailable = null;
    status("pass", "bridge connected — Run is available");
  } catch (e) {
    // Not being connected is a normal state, not a broken editor, so this says
    // what is missing rather than looking like a crash.
    //
    // And the reason is **kept**, not just printed: a page opened with both
    // `?bridge=` and `?open=` loads the file right after this, and its status
    // overwrites this line within the same tick. Without keeping it, all that
    // is left on screen is a Run button whose tooltip says "start a bridge"
    // while a bridge is running and refusing — which is exactly the state this
    // was found in, with no way to tell a refused token from a dead port.
    state.runUnavailable = e?.message ?? "could not connect to the bridge";
    status("error", state.runUnavailable);
  }
  renderAll();
}

// ---------------------------------------------------------------- files

/**
 * What the engine is handed: the document, and the files it references beside
 * it on disk (editor/src/neighbours.mjs). Only the desktop shell can read the
 * disk, and only for a document that is a file there; otherwise the document
 * goes alone, and the loader says what it cannot find.
 */
function filesFor(name, text) {
  const path = inShell() ? state.handle?.path : null;
  if (!path) return gatherFiles(name, text);
  const dir = path.slice(0, Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")));
  const onDisk = (rel) => `${dir}/${rel}`;
  return gatherFiles(name, text, {
    exists: (rel) => window.anvil.fileExists(onDisk(rel)),
    readText: (rel) => window.anvil.readTextFileIfAny(onDisk(rel)),
  });
}

// `.yseq` first: it is a sequence's own extension and is still YAML inside.
// `.yaml` and `.yml` stay, because sequences already exist under them
// (ADR-0039).
const PICKER = {
  types: [
    { description: "Anvil sequence", accept: { "application/yaml": [".yseq", ".yaml", ".yml"] } },
  ],
};

// What a document with no filename yet is offered as. A file that already has
// a name keeps it (ADR-0039 §3).
const NEW_SEQUENCE_NAME = "sequence.yseq";

// What File ▸ New starts from: the least the loader reads as a sequence. It is
// not a valid one yet — `main` may not be empty
// (crates/cargador/src/lib.rs:2138) — and that is left for the engine to say
// in the status bar rather than papered over with a placeholder step nobody
// asked for: the first step inserted from the palette makes it valid.
const NEW_SEQUENCE_TEXT = "name: sequence\nmain: []\n";

// `window.anvil` is injected by the shell's preload
// (`editor/electron/preload.cjs`) and exists whether or not the File System
// Access API also happens to. Checked first so a packaged build never falls
// through to the download fallback, which makes no sense once there is a
// real filesystem and a native dialog underneath the page (ADR-0037 §d).
const inShell = () => Boolean(window.anvil);

// A handle shaped like the two methods the rest of this file uses from a
// `FileSystemFileHandle` (`getFile()`, `createWritable()`), so `loadText`
// and `saveFile` do not need to know which world they are in.
function shellHandle(path) {
  return {
    path,
    name: path.split(/[\\/]/).pop(),
    async getFile() {
      const text = await window.anvil.readTextFile(path);
      return { name: this.name, text: async () => text };
    },
    async createWritable() {
      return {
        async write(text) {
          await window.anvil.writeTextFile(path, text);
        },
        async close() {},
      };
    },
  };
}

// Same machine, no second terminal: asks the shell to start
// `anvil <sequence_path> --bridge` itself and connects to the URL it
// prints, through the same `openBridge` a `?bridge=` link already uses.
// Which `anvil` is the shell's call (`findEngine` in electron/main.mjs): the
// one installed on the machine, not one inside the editor (#80).
async function connectLocalBridge(sequencePath) {
  state.runUnavailable = null;
  try {
    status("busy", "starting the engine…");
    const { url, engine } = await window.anvil.startBridge(sequencePath);
    state.engineVersion = engine.version;
    renderVersions();
    await openBridge(url);
    if (state.bridge) {
      status("pass", `bridge connected — Run is available (${engineNote(engine)})`);
    }
  } catch (e) {
    state.runUnavailable = ipcMessage(e) || "could not start the local engine";
    if (e?.code === "no-engine") {
      state.engineVersion = "not found";
      renderVersions();
    }
    status("error", state.runUnavailable);
    renderAll();
  }
}

// Which engine Run uses, for the status bar. The editor validates with the
// engine it carries and runs with the one installed, and when their versions
// differ that is worth seeing before a run rather than after (ADR-0031).
function engineNote({ path, version, editor }) {
  const differs = editor && version !== editor;
  return `anvil ${version} at ${path}` + (differs ? ` — the editor is ${editor}` : "");
}

// "Editor 0.6.2 · anvil 0.6.2" in the status bar's corner. The engine's half
// is marked when it differs from a released editor's version: the editor
// validates with the engine it carries and runs with the installed one
// (ADR-0031), so a mismatch means the two can disagree about one file.
function renderVersions() {
  if (!state.versions) return;
  const { editor, packaged } = state.versions;
  const engine = state.engineVersion;
  const known = engine && engine !== "not found";
  ui.versions.replaceChildren(
    document.createTextNode(`Editor ${packaged ? editor : "dev"} · `),
    Object.assign(document.createElement("span"), {
      className: "engine",
      textContent: engine === "not found" ? "anvil not found" : `anvil ${engine ?? "—"}`,
    }),
  );
  ui.versions.dataset.mismatch = String(packaged && known && engine !== editor);
  ui.versions.title = packaged
    ? "The editor's version, and the version of the anvil engine Run uses"
    : "Running from a source checkout, and the anvil engine Run uses";
  ui.versions.hidden = false;
}

// Electron wraps an error thrown in the shell as "Error invoking remote method
// 'anvil:…': Error: <message>". The message is the part meant for a person.
function ipcMessage(e) {
  return String(e?.message ?? "").replace(/^Error invoking remote method '[^']*': (Error: )?/, "");
}

// File ▸ Locate Anvil Engine…: once there is an engine, the open sequence gets
// the bridge it could not get before.
async function locateEngine() {
  try {
    const engine = await window.anvil.locateEngine();
    if (!engine) return;
    state.engineVersion = engine.version;
    renderVersions();
    const path = state.handle?.path;
    if (path && !state.runInFlight) await connectLocalBridge(path);
    else status("pass", `engine located: ${engineNote(engine)}`);
  } catch (e) {
    status("error", ipcMessage(e));
  }
}

async function fetchText(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.text();
}

async function openFile() {
  if (!(await confirmDiscard())) return;
  if (inShell()) {
    const path = await window.anvil.openDialog();
    if (!path) return;
    const handle = shellHandle(path);
    const file = await handle.getFile();
    loadText(await file.text(), file.name, handle);
    await connectLocalBridge(path);
    return;
  }
  if (window.showOpenFilePicker) {
    const [handle] = await window.showOpenFilePicker(PICKER);
    const file = await handle.getFile();
    loadText(await file.text(), file.name, handle);
    return;
  }
  // Firefox and Safari have no File System Access API. Saving then goes
  // through a download, which cannot write back to the same file — so the
  // editor says which mode it is in rather than pretending Save works.
  const input = document.createElement("input");
  input.type = "file";
  input.accept = ".yseq,.yaml,.yml";
  input.addEventListener("change", async () => {
    const file = input.files?.[0];
    if (file) loadText(await file.text(), file.name, null);
  });
  input.click();
}

async function newFile() {
  // Stopping the bridge under a run would leave the bench wherever the run
  // had it, with no `cleanup` (engine-pool.mjs, `terminateAll`).
  if (state.runInFlight) {
    status("error", "a run is in flight — wait for it to finish before starting a new sequence");
    return;
  }
  if (!(await confirmDiscard())) return;
  // The bridge was started for the file that was open: its executors and its
  // network allowance are that file's (packaging/anvil-host/src/main.rs). A
  // Run of a different sequence through it would reach the wrong equipment,
  // so the new document starts without one. Saving it gives it its own.
  if (inShell() && state.bridge) {
    state.bridge = null;
    await window.anvil.stopBridge();
  }
  // Nor does the reason the previous file had no Run carry over to it.
  state.runUnavailable = null;
  loadText(NEW_SEQUENCE_TEXT, null, null);
}

function loadText(text, filename, handle) {
  state.doc = new SequenceDocument(text);
  state.handle = handle;
  state.filename = filename;
  state.dirty = false;
  state.selected = null;
  if (state.text) {
    state.text.destroy();
    state.text = null;
  }
  const firstMain = state.doc.steps("main")[0];
  if (firstMain) state.selected = { phase: "main", index: 0 };
  renderAll();
  if (state.view === "text") renderText();
  validate();
}

async function saveFile({ forceDialog = false, connect = true } = {}) {
  if (!state.doc) return;

  let handle = state.handle;
  let savedAs = null;
  if (!handle || forceDialog) {
    if (inShell()) {
      const path = await window.anvil.saveDialog(state.filename ?? NEW_SEQUENCE_NAME);
      if (!path) return;
      handle = shellHandle(path);
      savedAs = path;
    } else if (window.showSaveFilePicker) {
      handle = await window.showSaveFilePicker({
        ...PICKER,
        suggestedName: state.filename ?? NEW_SEQUENCE_NAME,
      });
    } else {
      downloadFallback();
      return;
    }
    state.handle = handle;
    state.filename = handle.name;
  }

  const writable = await handle.createWritable();
  await writable.write(state.doc.text);
  await writable.close();
  state.dirty = false;
  renderAll();
  status("pass", `saved ${state.filename}`);
  // A document that had no file had no bridge either (`newFile`); now that it
  // is somewhere on disk, it gets one the same way an opened file does.
  // Not when saving on the way out (`saveThenLeave`): an engine started only
  // to be stopped as the window closes is a second of waiting for nothing.
  if (connect && savedAs && !state.bridge) await connectLocalBridge(savedAs);
}

// ------------------------------------------------------ unsaved changes

/**
 * Whether the open document may be thrown away, asking first if it has unsaved
 * changes (#84). New and Open call it before they replace the document.
 *
 * In the shell the question is Save / Don't Save / Cancel. "Save" saves — through
 * Save As if the document has never had a file — and the answer is yes only if
 * the save went through: dismissing its dialog, or a write that fails, keeps
 * the document. A plain browser has no native dialog to put three buttons on,
 * so there it is only whether to discard.
 */
async function confirmDiscard() {
  if (!state.doc || !state.dirty) return true;
  const name = state.filename ?? NEW_SEQUENCE_NAME;
  if (!inShell()) return window.confirm(`Discard the unsaved changes to ${name}?`);

  const answer = await window.anvil.askUnsaved(name);
  if (answer === "discard") return true;
  if (answer === "save") return saveKeepingTheAnswer({ connect: false });
  return false;
}

/** Saves, and says whether the document is saved afterwards. */
async function saveKeepingTheAnswer(options) {
  try {
    await saveFile(options);
  } catch (e) {
    // The browser's picker rejects when it is dismissed; anything else is a
    // write that failed. Either way the changes are still only in here.
    if (e?.name !== "AbortError") status("error", `could not save: ${e?.message ?? e}`);
  }
  return !state.dirty;
}

// Closing, quitting and reloading unload the page. While there are unsaved
// changes it refuses: a plain browser shows its own "leave site?" prompt, and
// the shell asks Save / Don't Save / Cancel in its place (`guardUnsaved` in
// electron/main.mjs). A browser only honours this after the person has
// interacted with the page — which anyone with unsaved edits has.
window.addEventListener("beforeunload", (e) => {
  if (!state.doc || !state.dirty) return;
  e.preventDefault();
  e.returnValue = "";
});

// "Save" answered at close or reload: the shell held the leaving back so this
// can save first, and does it once the save went through.
async function saveThenLeave(kind) {
  if (await saveKeepingTheAnswer({ connect: false })) window.anvil.leave(kind);
}

function downloadFallback() {
  const blob = new Blob([state.doc.text], { type: "application/yaml" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = state.filename ?? NEW_SEQUENCE_NAME;
  a.click();
  URL.revokeObjectURL(a.href);
  state.dirty = false;
  renderAll();
}

// ---------------------------------------------------------------- menus

function wireMenus() {
  const closeAll = () => {
    for (const m of document.querySelectorAll(".menu")) {
      const list = m.querySelector("ul");
      if (list) list.hidden = true;
      m.querySelector("button")?.setAttribute("aria-expanded", "false");
    }
  };

  // Delegated, because the bar is rendered from the inventory rather than
  // written into the HTML: binding each button once at start-up would miss
  // every menu built after this runs.
  ui.menus.addEventListener("click", (e) => {
    const button = e.target.closest?.("[data-menu]");
    if (!button || button.disabled) return;
    e.stopPropagation();
    const list = button.parentElement.querySelector("ul");
    const open = list.hidden;
    closeAll();
    list.hidden = !open;
    button.setAttribute("aria-expanded", String(open));
  });

  document.addEventListener("click", closeAll);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeAll();
  });

  const actions = {
    new: newFile,
    open: openFile,
    "locate-engine": locateEngine,
    save: () => saveFile(),
    "save-as": () => saveFile({ forceDialog: true }),
    "view-steps": () => setView("steps"),
    "view-text": () => setView("text"),
  };

  document.addEventListener("click", (e) => {
    const action = e.target.closest?.("[data-action]")?.dataset.action;
    if (action && actions[action]) actions[action]();
  });

  // Inside the shell the native menu bar carries these same actions, so the
  // page's own bar would be a second copy of it.
  if (inShell()) {
    ui.menus.hidden = true;
    window.anvil.onMenu((action) => actions[action]?.());
    window.anvil.onSaveThenLeave(saveThenLeave);
    window.anvil.onDevExit(devExited);
    window.anvil.versions().then((v) => {
      state.versions = v;
      renderVersions();
    });
  }

  for (const b of document.querySelectorAll(".views button")) {
    b.addEventListener("click", () => setView(b.dataset.view));
  }

  // TestStand's panes are tabbed, and each dock has its own strip.
  wirePaneTabs("left", setLeftTab);
  wirePaneTabs("right", setRightTab);
  wirePaneTabs("bottom", setBottomTab);

  ui.run.addEventListener("click", run);
}

function wirePaneTabs(dock, set) {
  for (const b of document.querySelectorAll(`[data-${dock}]`)) {
    b.addEventListener("click", () => set(b.dataset[dock]));
  }
}

/**
 * Shows one of a dock's panes and marks its tab.
 *
 * `hidden` rather than unmounting, because these panes hold live state — the
 * text editor's cursor, the output of a run that has ended — and rebuilding
 * them on every tab change would throw it away.
 */
function selectPane(dock, which, panes) {
  for (const [name, node] of Object.entries(panes)) node.hidden = name !== which;
  for (const b of document.querySelectorAll(`[data-${dock}]`)) {
    b.setAttribute("aria-selected", String(b.dataset[dock] === which));
  }
}

/** The Templates / IO Configurations pair, inside the palette. */
function setLeftTab(which) {
  state.leftTab = which;
  selectPane("left", which, { templates: ui.templates, io: ui.ioConfigurations });
}

function setRightTab(which) {
  state.rightTab = which;
  selectPane("right", which, {
    variables: ui.variables,
    sequences: ui.sequences,
    execution: ui.execution,
  });
}

function setBottomTab(which) {
  // Analysis Results is the Sequence Analyzer, which is out of scope
  // (ADR-0043 §3). Its tab is there and refuses to open, saying why.
  if (which === "analysis") return;
  state.bottomTab = which;
  selectPane("bottom", which, { step: ui.stepEditor, output: ui.output });
  ui.stepTitle.hidden = which !== "step";
}

/**
 * The docked tabs Anvil does not fill, greyed where TestStand puts them.
 *
 * Only the Analysis Results tab today: the Sequence Analyzer is out of the
 * parity scope, and what Anvil checks statically the loader checks, live, in
 * the status bar as the file is typed.
 */
function markGreyedTabs() {
  for (const [selector, id] of [
    ['[data-bottom="analysis"]', "execution.analysis-results"],
  ]) {
    const tab = document.querySelector(selector);
    const entry = parityById(id);
    if (!tab || !entry) continue;
    tab.disabled = true;
    tab.dataset.parity = entry.state;
    tab.title = gapTooltip(entry);
  }
}

// ---------------------------------------------------------------- start

renderMenuBar();
wireMenus();
renderExecControls();
markGreyedTabs();
renderAll();

// `?open=<path>` loads a sequence over HTTP instead of through the file picker.
// It is how the editor is opened on a known fixture, and the only way to
// exercise it without driving the browser's native file dialog. A file opened
// this way has no handle, so Save falls back to a download until it is saved
// somewhere with Save As.
const params = new URLSearchParams(location.search);

// `?bridge=<ws url>` is what `anvil <sequence.yaml> --bridge` prints. Connecting
// first means Run is already available by the time the file is on screen.
const bridgeUrl = params.get("bridge");
if (bridgeUrl) await openBridge(bridgeUrl);

const wanted = params.get("open");
if (wanted) {
  status("busy", `opening ${wanted}…`);
  try {
    // Inside the shell this is usually a filesystem path and there is no
    // server to ask: `fetch` only worked because Vite's dev middleware
    // happened to be serving the repo's `ejemplos/`, so a packaged build
    // failed here with a 404 and no way for the person to tell why.
    //
    // Both are still tried, in that order, because both are real: a packaged
    // shell is handed an absolute path, and `npm run app` in the dev tree is
    // handed `/ejemplos/…`, which means nothing to the filesystem and
    // everything to the dev server.
    let text = inShell() ? await window.anvil.readTextFileIfAny(wanted) : null;
    // A handle only when it came off the filesystem: a sequence the dev
    // server handed over has no file to write back to, and saying so beats
    // saving somewhere else.
    const handle = text === null ? null : shellHandle(wanted);
    if (text === null) text = await fetchText(wanted);
    loadText(text, wanted.split(/[\\/]/).pop(), handle);
    // Same as opening through the dialog: on the same machine the editor
    // starts the engine itself rather than asking for a second terminal.
    if (inShell() && !bridgeUrl) await connectLocalBridge(wanted);
  } catch (e) {
    status("error", `could not open ${wanted}: ${e.message}`);
  }
} else {
  status("unknown", "no file open — File ▸ Open…");
}
