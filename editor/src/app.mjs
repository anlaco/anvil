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
import { browserPool, connectBridge, EngineHostError } from "./engine-pool.mjs";
import { gatherFiles, unsavedPathHint } from "./neighbours.mjs";
import { applyEvent, newRunState, rowKey, runButton } from "./run-state.mjs";

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
  stepTab: "properties",
  stepPage: "General",
  view: "steps",
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
  renderRunStatus();
}

// ---------------------------------------------------------------- rendering

function renderSequence() {
  const doc = state.doc;
  ui.list.replaceChildren();
  if (!doc) return;

  for (const phase of PHASES) {
    const steps = doc.steps(phase);
    const head = document.createElement("div");
    head.className = "phase";
    head.textContent = `${phase} (${steps.length})`;
    ui.list.append(head);

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

      const name = document.createElement("span");
      name.className = "name";
      name.textContent = step.name ?? "(unnamed)";

      const kind = document.createElement("span");
      kind.className = "kind";
      // A step with no type does not load; saying so on the row is how the
      // person finds it.
      kind.textContent = step.type ?? "no type";

      // A mark, not just a colour: a row that says pass or fail by hue alone is
      // unreadable to whoever cannot tell the hues apart, and this gets read
      // next to a bench.
      const mark = document.createElement("span");
      mark.className = "run-mark";
      const rs = row.dataset.run;
      const MARKS = { running: "▶", pass: "✓", done: "•", fail: "✕", error: "!", skipped: "–" };
      mark.textContent = rs ? (MARKS[rs] ?? "?") : "";
      if (rs) mark.title = rs;

      row.append(name, kind, mark);
      row.addEventListener("click", () => {
        state.selected = { phase, index: step.index };
        renderSequence();
        renderStep();
      });
      makeDraggable(row, { kind: "move", phase, index: step.index });
      makeDropTarget(row, phase, () => step.index);
      ui.list.append(row);
    }

    // A phase with no steps still has to be a target, or a sequence that starts
    // empty can never receive its first step by dragging.
    if (steps.length === 0) {
      const empty = document.createElement("div");
      empty.className = "phase-empty";
      empty.textContent = "(empty)";
      makeDropTarget(empty, phase, () => 0);
      ui.list.append(empty);
    }
  }
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
  ui.palette.replaceChildren();

  const group = document.createElement("div");
  group.className = "palette-group";
  group.textContent = "Step Types";
  ui.palette.append(group);

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
  l.htmlFor = id;
  input.id = id;
  if (hint) {
    l.title = hint;
    // A wrapper (the Type row) explains itself through the control inside it.
    const target = input.matches("input, select, textarea")
      ? input
      : input.querySelector("input, select, textarea, button");
    if (target) target.title = hint;
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
// have is greyed out where TestStand puts it, rather than left off. That is
// what makes this the inventory — you can see what is coming and what is not.
// ---------------------------------------------------------------------------

/** What each of Anvil's types is called in TestStand's menu. */
const TYPE_LABELS = {
  action: "Action",
  pass_fail: "Pass/Fail Test",
  numeric_limit: "Numeric Limit Test",
  statement: "Statement",
  sequence_call: "Sequence Call",
};

const NOT_YET = "TestStand has this step type. Anvil does not, yet.";

/** TestStand's Insert Step menu, separators and all (2019 screenshots). */
const TYPE_MENU = [
  {
    label: "Tests",
    items: [
      { label: "Pass/Fail Test", type: "pass_fail" },
      { label: "Numeric Limit Test", type: "numeric_limit" },
      { label: "Multiple Numeric Limit Test" },
      { label: "String Value Test" },
    ],
  },
  { label: "Action", type: "action" },
  { sep: true },
  { label: "FTP Files" },
  { label: "Additional Results" },
  { label: "Sequence Call", type: "sequence_call" },
  { label: "Statement", type: "statement" },
  { label: "Property Loader" },
  { label: "Label" },
  { label: "Message Popup" },
  { label: "Call Executable" },
  { sep: true },
  {
    label: "Flow Control",
    items: [
      { label: "If" },
      { label: "Else" },
      { label: "Else If" },
      { sep: true },
      { label: "For" },
      { label: "For Each" },
      { label: "While" },
      { label: "Do While" },
      { label: "Sweep Loop" },
      { sep: true },
      { label: "Break" },
      { label: "Continue" },
      { sep: true },
      { label: "Select" },
      { label: "Case" },
      { sep: true },
      { label: "Goto" },
      { sep: true },
      { label: "End" },
    ],
  },
  { sep: true },
  { label: "Synchronization", items: [] },
  { label: "Database", items: [] },
  { label: "Data Streams", items: [] },
  { label: "LabVIEW Utility", items: [] },
  { label: "DataLogger" },
];

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
 * someone reads them off TestStand.
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
        ? `${entry.label}: what TestStand puts here is not written down in Anvil yet.`
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
        : `${NOT_YET} (${entry.label})`;
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
    const menu = buildTypeMenu(TYPE_MENU, choose, 0);
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

const PROPERTY_PAGES = [
  { name: "General", render: pageGeneral },
  { name: "Run Options", render: pageRunOptions },
  { name: "Looping", render: pageLooping },
  { name: "Post Actions", render: pagePostActions },
  {
    name: "Switching",
    todo: "TestStand drives an NI Switch Executive route around the step. Anvil has no switching.",
  },
  {
    name: "Synchronization",
    todo: "TestStand's locks, rendezvous, queues, notifications, semaphores and batches. Anvil runs one UUT at a time, so none of it exists yet.",
  },
  { name: "Expressions", render: pageExpressions },
  { name: "Preconditions", render: pagePreconditions },
  {
    name: "Requirements",
    todo: "TestStand links a step to a requirement in a requirements management tool. Anvil has no such link.",
  },
  {
    name: "Additional Results",
    todo: "TestStand logs extra expressions into the report per step. In Anvil a measurement reaches the report through `assign`, on the Expressions page; naming arbitrary expressions to log is not implemented.",
  },
  {
    name: "Property Browser",
    todo: "TestStand browses the step's raw property tree. In Anvil the raw form of a step is the YAML itself — use the Text view.",
  },
];

/**
 * The tabs a step of this type shows, in TestStand's order.
 *
 * What a type is judged on is **not** a page inside Properties: TestStand gives
 * it a tab of its own beside Properties, and a step with nothing to judge
 * simply has no such tab. So the tab strip changes with the type, while the
 * Properties page list underneath is the same for every step.
 */
function tabsFor(type) {
  const tabs = [["properties", "Properties"]];
  if (type === "pass_fail" || type === "numeric_limit") {
    tabs.push(["data-source", "Data Source"]);
  }
  if (type === "numeric_limit") tabs.push(["limits", "Limits"]);
  tabs.push(["module", "Module"]);
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
  todoNote(
    fields,
    "Run Mode ▸ Force Pass and Force Fail, Record Results, and Step Failure Causes Sequence Failure — which ADR-0040 put out of its own scope.",
  );
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
  todoNote(
    fields,
    "TestStand's loop types — Fixed number, While, Do While, Pass/Fail count — their pass and fail counts, and the Looping status. Retries is not a loop; issue #76 asks how the two should meet.",
  );
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
  todoNote(
    fields,
    "TestStand's On Pass and On Fail actions — Goto step, Call sequence, Terminate — and the custom conditions that choose between them.",
  );
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

  todoNote(
    fields,
    "TestStand's Pre-Expression, which runs before the step, and its Status Expression. Anvil's `assign` runs after the step and can only write a declared local.",
  );
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
  for (const [label, glyph] of specs) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "icon-button";
    b.disabled = true;
    b.title = `${label}. Not implemented.`;
    b.setAttribute("aria-label", label);
    b.append(svgIcon(glyph));
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
    resolved(head, step.sequence ? `resolved against this file's directory` : "nothing named yet");
    parameterTable(
      page,
      step,
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

  field(
    head,
    "Executor",
    pathRow(
      disabledInput(step.executor ?? "<None>"),
      [["Browse for an executor", GLYPH.browse]],
      [
        ["Declare a new executor", GLYPH.create],
        ["Edit this executor", GLYPH.edit],
      ],
    ),
    "TestStand's Project Path: the department the module lives in. Chosen on the General page.",
  );
  resolved(
    head,
    executor
      ? executor.type === "grpc"
        ? `${executor.host}:${executor.port}`
        : (executor.path ?? "no path declared")
      : "this step names no executor",
  );

  field(
    head,
    "Module",
    pathRow(
      textInput(step.module ?? "", (v) => edit("module", v || undefined)),
      [
        ["Browse the executor's catalog", GLYPH.browse],
        ["Pick a module from the executor", GLYPH.terminals],
        ["Rename what this step calls", GLYPH.edit],
        ["Break on this step", GLYPH.tools],
      ],
      [
        ["Show the module's terminals", GLYPH.terminals],
        ["Edit the module", GLYPH.edit],
        ["Reload the catalog", GLYPH.reload],
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
  resolved(
    head,
    step.module
      ? `${step.module} on '${step.executor ?? "?"}'`
      : "this step calls nothing",
  );

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
  const rows = Object.entries(step.inputs ?? {});
  if (rows.length === 0) {
    const empty = document.createElement("span");
    empty.className = "param-empty";
    empty.textContent = "This step declares no inputs.";
    table.append(empty);
  } else {
    for (const [name, value] of rows) {
      for (const cell of [name, typeof value, "in", String(value)]) {
        const c = document.createElement("span");
        c.className = "param-cell";
        c.textContent = cell;
        table.append(c);
      }
    }
  }
  box.append(table);
  todoNote(box, todo, detail);
}

/**
 * The panel TestStand fills with the VI: its project and name, its connector
 * pane, and the documentation off the VI itself.
 *
 * Anvil's equivalent exists already on the wire and is not plugged in here
 * yet: an executor describes its steps — each one's inputs, outputs and a line
 * of documentation (`StepSpec` in `paso.proto`, ADR-0021) — which is the same
 * three things this panel shows. Asking for it needs the bridge, so the shape
 * is here and the content says what is missing rather than looking empty.
 */
function modulePanel(parent, step, executor) {
  const panel = document.createElement("div");
  panel.className = "module-panel";
  parent.append(panel);

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

  const ins = document.createElement("div");
  ins.className = "terminals in";
  const inputs = Object.keys(step.inputs ?? {});
  for (const t of inputs.length > 0 ? inputs : ["(inputs)"]) {
    const s = document.createElement("span");
    s.textContent = t;
    if (inputs.length === 0) s.className = "unknown";
    ins.append(s);
  }

  const box = document.createElement("div");
  box.className = "connector-box";
  box.textContent = (step.module ?? "").split("/").pop() ?? "";

  const outs = document.createElement("div");
  outs.className = "terminals out";
  for (const t of ["measured value", "status"]) {
    const s = document.createElement("span");
    s.className = "unknown";
    s.textContent = t;
    outs.append(s);
  }

  pane.append(ins, box, outs);
  panel.append(pane);

  const doc = document.createElement("p");
  doc.className = "module-doc";
  doc.textContent = "Not connected: the executor has not been asked to describe this module.";
  doc.title =
    "What a module takes and returns, and what it is for, is what its executor answers to Describe (StepSpec in paso.proto, ADR-0021). Asking needs the bridge, which the editor cannot use yet, so these terminals are the shape of the answer rather than the answer.";
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
    if (!pages.some((page) => page.name === state.stepPage && !page.todo)) {
      state.stepPage = "General";
    }

    const nav = document.createElement("nav");
    nav.className = "prop-pages";
    for (const page of pages) {
      const item = document.createElement("button");
      item.type = "button";
      item.className = "prop-page";
      item.textContent = page.name;
      // Greyed out, not left off: the page is where TestStand puts it, and the
      // tooltip says what it would hold.
      item.disabled = Boolean(page.todo);
      item.title = page.todo ?? `${page.name} settings for this step`;
      if (page.name === state.stepPage) item.setAttribute("aria-current", "true");
      item.addEventListener("click", () => {
        state.stepPage = page.name;
        renderStep();
      });
      nav.append(item);
    }

    const content = document.createElement("div");
    content.className = "prop-content";
    body.append(nav, content);
    pages.find((page) => page.name === state.stepPage)?.render?.(content, ctx);
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

  for (const scope of SCOPES) {
    const vars = doc.variables(scope);
    const names = Object.keys(vars);

    const head = document.createElement("div");
    head.className = "scope";
    head.textContent = scope;
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

      row.append(k, v, action("Remove", () => {
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
            const [host, port] = v.split(":");
            doc.setExecutorField(ex.name, "host", host.trim());
            doc.setExecutorField(ex.name, "port", Number(port));
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

  const add = document.createElement("div");
  add.className = "assign-row add-var";
  const kind = select(["wasm", "grpc"], "wasm", () => {});
  add.append(
    kind,
    action("Declare", () => {
      let n = 1;
      let name = "bench";
      while (doc.executorNames().includes(name)) name = `bench_${++n}`;
      if (kind.value === "grpc") doc.addExecutor(name, "grpc", "127.0.0.1", 9101);
      else doc.addExecutor(name, "wasm", "departamento/dist/anvil-exec-wasm");
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
    ? `Sequence — ${state.doc.name}`
    : "Sequence";
  renderPalette();
  renderSequence();
  renderStep();
  renderVariables();
  if (!skipText && state.view === "text") renderText();
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
  ui.run.disabled = true;
  renderSequence();

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
    const report = `${stdout}\n${stderr}`.trim();
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
    // The full report goes to the console until there is somewhere to put it.
    // A run's detail — per-step results, measurements, limits — needs a pane of
    // its own, and that arrives with the live events of ADR-0029.
    console.log(report);
  } catch (e) {
    const what = e instanceof EngineHostError ? e.message : String(e?.message ?? e);
    status("error", `could not run: ${what}`);
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
    status("pass", "bridge connected — Run is available");
  } catch (e) {
    // Not being connected is a normal state, not a broken editor, so this says
    // what is missing rather than looking like a crash.
    status("error", e?.message ?? "could not connect to the bridge");
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
      m.querySelector("ul").hidden = true;
      m.querySelector("button").setAttribute("aria-expanded", "false");
    }
  };

  for (const menu of document.querySelectorAll(".menu")) {
    const button = menu.querySelector("button");
    const list = menu.querySelector("ul");
    button.setAttribute("aria-expanded", "false");
    button.addEventListener("click", (e) => {
      e.stopPropagation();
      const open = list.hidden;
      closeAll();
      list.hidden = !open;
      button.setAttribute("aria-expanded", String(open));
    });
  }

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
    window.anvil.versions().then((v) => {
      state.versions = v;
      renderVersions();
    });
  }

  for (const b of document.querySelectorAll(".views button")) {
    b.addEventListener("click", () => setView(b.dataset.view));
  }

  ui.run.addEventListener("click", run);
}

// ---------------------------------------------------------------- start

wireMenus();
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
