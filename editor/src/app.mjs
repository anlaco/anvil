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
import { runEngine } from "./engine.mjs";

// The generated core modules are served alongside the app.
const load = async (name) => {
  const res = await fetch(`/generated/${name}`);
  if (!res.ok) {
    throw new Error(
      `could not load ${name} (${res.status}). Run 'npm run transpile' first.`,
    );
  }
  return new Uint8Array(await res.arrayBuffer());
};

const el = (id) => document.getElementById(id);

const ui = {
  panes: document.querySelector(".panes"),
  list: el("sequence-list"),
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

  ui.stepEditor.append(fields);
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
  ui.stepEditor.hidden = !steps;
  ui.textEditor.hidden = steps;
  for (const b of document.querySelectorAll(".views button")) {
    b.setAttribute("aria-selected", String(b.dataset.view === view));
  }
  if (!steps) renderText();
}

function renderAll({ skipText = false } = {}) {
  ui.panes.dataset.stale = String(state.doc?.stale ?? false);
  ui.filename.textContent = state.filename ?? "no file";
  ui.filename.dataset.dirty = String(state.dirty);
  ui.run.disabled = !state.doc;
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
    const { exitCode, stderr } = await runEngine({
      args: [name, "--validate"],
      files: { [name]: doc.text },
      load,
    });
    // The engine's own diagnostics, verbatim. Rewriting them here would mean
    // two sources for the same message, and the loader's is the one with the
    // field name, the location and the suggestion
    // (crates/cargador/src/lib.rs:686-763).
    const last = stderr.trim().split("\n").filter(Boolean).pop() ?? "";
    status(exitCode === 0 ? "pass" : "fail", last || (exitCode === 0 ? "valid" : "rejected"));
  } catch (e) {
    // A host failure is not a verdict about the sequence, and must not read as
    // one (ADR-0019, Rule 2).
    status("error", `editor could not run the engine: ${e.message}`);
  }
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

  ui.run.addEventListener("click", () => {
    // The engine cannot invoke a step from here: reaching an executor needs the
    // bridge (ADR-0030), which is not built. Saying so plainly beats a button
    // that looks like it worked.
    status("error", "Run needs the bridge, which is not implemented yet (ADR-0030).");
  });
}

// ---------------------------------------------------------------- start

wireMenus();
renderAll();

// `?open=<path>` loads a sequence over HTTP instead of through the file picker.
// It is how the editor is opened on a known fixture, and the only way to
// exercise it without driving the browser's native file dialog. A file opened
// this way has no handle, so Save falls back to a download until it is saved
// somewhere with Save As.
const wanted = new URLSearchParams(location.search).get("open");
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
