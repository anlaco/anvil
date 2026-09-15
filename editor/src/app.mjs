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
import { gatherFiles } from "./neighbours.mjs";
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
      ui.list.append(row);
    }
  }
}

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
    select(STEP_TYPES, step.type, (v) => edit("type", v)),
    "How the step is judged. What it calls is its module.",
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

  if (step.module !== null || step.type === "action") {
    field(
      fields,
      "Module",
      textInput(step.module, (v) => edit("module", v || undefined)),
      "What the step calls on its executor.",
    );
    field(
      fields,
      "Executor",
      textInput(step.executor, (v) => edit("executor", v || undefined)),
      "Which declared executor serves this step. Required: anvil has no executor of its own.",
    );
  }

  if (step.limit) {
    group(fields, `Limit — ${step.limit.comparison ?? "?"}`);
    const setLimit = (key, v) => {
      doc.setStepLimit(sel.phase, sel.index, key, v);
      afterEdit();
    };
    // Only the fields the limit already has: which fields a comparison uses is
    // the loader's rule (ADR-0040 §7), and offering the others would build a
    // limit it refuses. Changing the comparison itself is the text view's job
    // for now.
    for (const key of ["low", "high", "nominal", "lower", "upper"]) {
      if (key in step.limit) {
        field(fields, key, numberInput(step.limit[key], (v) => setLimit(key, v)));
      }
    }
    if ("units" in step.limit) {
      field(fields, "units", textInput(step.limit.units, (v) => setLimit("units", v)));
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
    status(exitCode === 0 ? "pass" : "fail", verdict + run);
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
