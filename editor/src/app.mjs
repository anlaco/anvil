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

import { SequenceDocument, PHASES, SCOPES, STEP_TYPES } from "./document.mjs";
import { browserPool, connectBridge, EngineHostError } from "./engine-pool.mjs";

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
  run: el("run"),
};

const state = {
  /** @type {SequenceDocument|null} */
  doc: null,
  /** @type {FileSystemFileHandle|null} */
  handle: null,
  filename: null,
  dirty: false,
  selected: null, // { phase, index }
  view: "steps",
  text: null, // CodeMirror view
  validateTimer: null,
  bridge: null, // the URL, once connected
};

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

      const name = document.createElement("span");
      name.className = "name";
      name.textContent = step.name ?? "(unnamed)";

      const kind = document.createElement("span");
      kind.className = "kind";
      // `grpc` is the default and saying so on every row is noise; the other
      // three are the ones worth seeing at a glance.
      kind.textContent = step.type === "grpc" ? "" : step.type;

      row.append(name, kind);
      row.addEventListener("click", () => {
        state.selected = { phase, index: step.index };
        renderSequence();
        renderStep();
      });
      ui.list.append(row);
    }
  }
}

// What each step type is, in the words of someone deciding which to insert.
// These four are the engine's own (crates/cargador/src/lib.rs:2191-2203); the
// palette offers no fifth, because a step the loader does not know is a step
// the editor must not be able to create (AP-04).
const STEP_TYPE_DOC = {
  grpc: "Calls a step served by an executor.",
  statement: "Assigns to variables. The engine runs it; no executor involved.",
  pass_fail: "Passes or fails on an expression.",
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
      : `Insert a ${type} step at the end of ${phase}`;
    item.addEventListener("click", () => {
      const index = state.doc.addStep(phase, type);
      state.selected = { phase, index };
      afterEdit();
    });
    ui.palette.append(item);
  }

  const note = document.createElement("p");
  note.className = "palette-note";
  // The steps an executor serves are the other half of this palette, and they
  // come from asking it (ADR-0021). That needs the bridge, so saying what is
  // missing beats an empty list that looks like an executor with no steps —
  // the distinction ADR-0019's Rule 2 is about.
  note.textContent = state.doc
    ? `Inserts at the end of ${phase}. Steps served by executors will appear here once the bridge can ask them for their catalog.`
    : "Open a sequence to insert steps.";
  ui.palette.append(note);
}

function field(parent, label, input, hint) {
  const l = document.createElement("label");
  l.textContent = label;
  const id = `f-${label.toLowerCase().replace(/\W+/g, "-")}`;
  l.htmlFor = id;
  input.id = id;
  parent.append(l, input);
  if (hint) {
    const p = document.createElement("p");
    p.className = "hint";
    p.textContent = hint;
    parent.append(p);
  }
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

function renderStep() {
  ui.stepEditor.replaceChildren();
  const sel = state.selected;
  const doc = state.doc;

  if (!doc || !sel) {
    ui.stepTitle.textContent = "Step";
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

  ui.stepTitle.textContent = `Step — ${step.name ?? "(unnamed)"}`;

  const fields = document.createElement("div");
  fields.className = "fields";

  const edit = (key, value) => {
    doc.setStepField(sel.phase, sel.index, key, value);
    afterEdit();
  };

  field(fields, "Name", textInput(step.name, (v) => edit("name", v)));
  field(
    fields,
    "Type",
    select(STEP_TYPES, step.type, (v) => edit("type", v === "grpc" ? undefined : v)),
    "grpc calls an executor; the other three the engine runs itself.",
  );
  field(
    fields,
    "Retries",
    numberInput(step.retries, (v) => edit("retries", v)),
  );

  const disabled = document.createElement("input");
  disabled.type = "checkbox";
  disabled.checked = step.disable;
  disabled.addEventListener("change", () =>
    edit("disable", disabled.checked ? true : undefined),
  );
  field(fields, "Disabled", disabled);

  if (step.type === "grpc") {
    field(
      fields,
      "Executor",
      textInput(step.executor, (v) => edit("executor", v || undefined)),
      "Which declared executor serves this step. Empty means the embedded one.",
    );
  }

  if (step.limit) {
    group(fields, `Limit — ${step.limit.type}`);
    const setLimit = (key, v) => {
      doc.setStepLimit(sel.phase, sel.index, key, v);
      afterEdit();
    };
    if (step.limit.type === "range") {
      field(fields, "Min", numberInput(step.limit.min, (v) => setLimit("min", v)));
      field(fields, "Max", numberInput(step.limit.max, (v) => setLimit("max", v)));
    } else {
      field(fields, "Operator", textInput(step.limit.op, () => {}));
      field(fields, "Expected", numberInput(step.limit.expected, (v) => setLimit("expected", v)));
    }
  }

  group(fields, "Step");
  const actions = document.createElement("div");
  actions.className = "actions";
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
  fields.append(actions);

  ui.stepEditor.append(fields);
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

  let any = false;
  for (const scope of SCOPES) {
    const vars = state.doc.variables(scope);
    const names = Object.keys(vars);
    if (names.length === 0) continue;
    any = true;

    const head = document.createElement("div");
    head.className = "scope";
    head.textContent = scope;
    ui.variables.append(head);

    for (const name of names) {
      const row = document.createElement("div");
      row.className = "var";
      const k = document.createElement("span");
      k.className = "k";
      k.textContent = name;
      const v = document.createElement("span");
      v.className = "v";
      v.textContent = JSON.stringify(vars[name]);
      row.append(k, v);
      ui.variables.append(row);
    }
  }

  if (!any) {
    const p = document.createElement("p");
    p.className = "empty";
    p.textContent = "No variables declared.";
    ui.variables.append(p);
  }
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

function renderAll({ skipText = false } = {}) {
  ui.panes.dataset.stale = String(state.doc?.stale ?? false);
  ui.filename.textContent = state.filename ?? "no file";
  ui.filename.dataset.dirty = String(state.dirty);
  ui.run.disabled = !state.doc || !engine.bridged;
  ui.run.title = engine.bridged
    ? "Run this sequence"
    : "Run needs a bridge: start `anvil <sequence.yaml> --bridge` and open the URL it prints";
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
  const name = state.filename ?? "sequence.yaml";
  try {
    const { exitCode, stderr } = await engine.run({
      args: [name, "--validate"],
      files: { [name]: doc.text },
    });
    // The engine's own diagnostics, verbatim. Rewriting them here would mean
    // two sources for the same message, and the loader's is the one with the
    // field name, the location and the suggestion
    // (crates/cargador/src/lib.rs:686-763).
    const last = stderr.trim().split("\n").filter(Boolean).pop() ?? "";
    status(exitCode === 0 ? "pass" : "fail", last || (exitCode === 0 ? "valid" : "rejected"));
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
 * executors and, through them, actual hardware. There is no live progress yet:
 * the engine reports at the end, and streaming it as it goes is ADR-0029,
 * unimplemented. So the button says "running…" and means it.
 */
async function run() {
  if (!state.doc || !engine.bridged) return;

  const name = state.filename ?? "sequence.yaml";
  status("busy", `running ${name}…`);
  ui.run.disabled = true;

  try {
    const { exitCode, stdout, stderr } = await engine.run({
      // The bridge's arguments come first, the sequence last, matching how the
      // native host builds argv (main.rs:604-608).
      args: [...engine.engineArgs, name],
      files: { [name]: state.doc.text },
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

    status(exitCode === 0 ? "pass" : "fail", verdict);
    // The full report goes to the console until there is somewhere to put it.
    // A run's detail — per-step results, measurements, limits — needs a pane of
    // its own, and that arrives with the live events of ADR-0029.
    console.log(report);
  } catch (e) {
    const what = e instanceof EngineHostError ? e.message : String(e?.message ?? e);
    status("error", `could not run: ${what}`);
  } finally {
    renderAll();
  }
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
    await engine.attachBridge(url, connectBridge);
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

const PICKER = {
  types: [
    { description: "Anvil sequence", accept: { "application/yaml": [".yaml", ".yml"] } },
  ],
};

async function openFile() {
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
  input.accept = ".yaml,.yml";
  input.addEventListener("change", async () => {
    const file = input.files?.[0];
    if (file) loadText(await file.text(), file.name, null);
  });
  input.click();
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

async function saveFile({ forceDialog = false } = {}) {
  if (!state.doc) return;

  let handle = state.handle;
  if (!handle || forceDialog) {
    if (!window.showSaveFilePicker) {
      downloadFallback();
      return;
    }
    handle = await window.showSaveFilePicker({
      ...PICKER,
      suggestedName: state.filename ?? "sequence.yaml",
    });
    state.handle = handle;
    state.filename = handle.name;
  }

  const writable = await handle.createWritable();
  await writable.write(state.doc.text);
  await writable.close();
  state.dirty = false;
  renderAll();
  status("pass", `saved ${state.filename}`);
}

function downloadFallback() {
  const blob = new Blob([state.doc.text], { type: "application/yaml" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = state.filename ?? "sequence.yaml";
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
    open: openFile,
    save: () => saveFile(),
    "save-as": () => saveFile({ forceDialog: true }),
    "view-steps": () => setView("steps"),
    "view-text": () => setView("text"),
  };

  document.addEventListener("click", (e) => {
    const action = e.target.closest?.("[data-action]")?.dataset.action;
    if (action && actions[action]) actions[action]();
  });

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
    const res = await fetch(wanted);
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    loadText(await res.text(), wanted.split("/").pop(), null);
  } catch (e) {
    status("error", `could not open ${wanted}: ${e.message}`);
  }
} else {
  status("unknown", "no file open — File ▸ Open…");
}
