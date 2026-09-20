// The parity inventory: what TestStand has, and where Anvil stands on each of
// it (ADR-0043).
//
// Pure data. No DOM, no imports, nothing that runs — for the same reason
// `run-state.mjs` is kept out of the DOM: this is what a test asserts on and
// what `scripts/paridad-a-doc.mjs` turns into `docs/paridad-teststand.md`, and
// neither of those can afford to boot an interface to read it.
//
// The rule it exists to enforce (AP-13): a thing TestStand has is never simply
// left off. It appears where TestStand puts it, greyed, saying what TestStand
// does there and which of three situations Anvil is in. The engine is what
// unlocks a cell — never the editor (ADR-0043 §6).
//
// Everything here about TestStand is **second-hand**: read from NI's
// documentation and from screenshots of TestStand 2026Q3. Same warning
// ADR-0040 carries.

/** The TestStand these were read off (ADR-0043 §2). */
export const TESTSTAND = "2026Q3";

/**
 * The four states an entry can be in (ADR-0043 §4).
 *
 * `built`     — Anvil has it. Carries no TestStand prose: what exists explains
 *               itself.
 * `todo`      — Anvil intends to and does not yet. Must name, in `needs`, the
 *               engine capability that unlocks it. A debt that cannot say what
 *               it waits for is a wish; and several debts naming the same
 *               capability are a priority, which is most of the point of this.
 * `elsewhere` — Anvil solves it, differently. Must say how, in `anvil`.
 * `never`     — deliberately out of scope. Must say why, in `why`, and the
 *               reason has to be a decision already written down somewhere,
 *               not one invented while filling this table in.
 */
export const VERDICTS = ["built", "todo", "elsewhere", "never"];

/** The extra field each verdict owes. `built` owes nothing. */
export const REQUIRED_FIELD = {
  todo: "needs",
  elsewhere: "anvil",
  never: "why",
};

// ---------------------------------------------------------------------------
// Step Settings ▸ Properties — the page list down the left.
//
// TestStand's list, in TestStand's order, and deliberately complete.
// `render` names the function in `app.mjs` that fills the page; only a `built`
// page has one.
// ---------------------------------------------------------------------------

export const PROPERTY_PAGES = [
  { id: "step.properties.general", label: "General", state: "built", render: "pageGeneral" },
  {
    id: "step.properties.run-options",
    label: "Run Options",
    state: "built",
    render: "pageRunOptions",
  },
  { id: "step.properties.looping", label: "Looping", state: "built", render: "pageLooping" },
  {
    id: "step.properties.post-actions",
    label: "Post Actions",
    state: "built",
    render: "pagePostActions",
  },
  {
    id: "step.properties.switching",
    label: "Switching",
    state: "todo",
    teststand: "Drives an NI Switch Executive route around the step.",
    needs: "engine: an instrument-switching layer. Anvil has no switching.",
  },
  {
    id: "step.properties.synchronization",
    label: "Synchronization",
    state: "todo",
    teststand:
      "Locks, rendezvous, queues, notifications, semaphores and batches around the step.",
    needs:
      "engine: parallelism with hierarchical cancellation (RF-39, post-MVP). Anvil runs one UUT at a time, so none of it exists yet.",
  },
  {
    id: "step.properties.expressions",
    label: "Expressions",
    state: "built",
    render: "pageExpressions",
  },
  {
    id: "step.properties.preconditions",
    label: "Preconditions",
    state: "built",
    render: "pagePreconditions",
  },
  {
    id: "step.properties.requirements",
    label: "Requirements",
    state: "todo",
    teststand: "Links the step to a requirement in a requirements management tool.",
    needs: "engine: a requirements link. Nothing in Anvil models one.",
  },
  {
    id: "step.properties.additional-results",
    label: "Additional Results",
    state: "todo",
    teststand: "Logs extra named expressions into the report, per step.",
    needs:
      "engine: naming arbitrary expressions to log. In Anvil a measurement reaches the report through `assign`, on the Expressions page.",
  },
  {
    id: "step.properties.property-browser",
    label: "Property Browser",
    state: "elsewhere",
    teststand: "Browses the step's raw property tree.",
    anvil: "The raw form of a step is the YAML itself — use the Text view.",
  },
];

// ---------------------------------------------------------------------------
// What a page Anvil has built still does not carry.
//
// A page is not all-or-nothing: Looping exists and holds `retries`, and none
// of TestStand's loop types. These are the gaps inside a `built` page, shown
// as a note at the foot of it. Keyed by the page id they hang under.
// ---------------------------------------------------------------------------

export const PAGE_DETAILS = [
  {
    id: "step.properties.run-options.run-mode",
    under: "step.properties.run-options",
    label: "Run Mode ▸ Force Pass / Force Fail, Record Results, Step Failure Causes Sequence Failure",
    state: "todo",
    teststand:
      "Forces a verdict on the step, keeps it out of the report, or stops its failure from failing the sequence.",
    needs: "engine: per-step verdict overrides — which ADR-0040 put out of its own scope.",
  },
  {
    id: "step.properties.looping.loop-types",
    under: "step.properties.looping",
    label: "Loop types",
    state: "todo",
    teststand:
      "Fixed number, While, Do While and Pass/Fail count loops, their pass and fail counts, and the Looping status.",
    needs:
      "engine: per-step looping. Retries is not a loop; issue #76 asks how the two should meet.",
  },
  {
    id: "step.properties.post-actions.on-pass-on-fail",
    under: "step.properties.post-actions",
    label: "On Pass / On Fail actions",
    state: "todo",
    teststand:
      "Goto step, Call sequence or Terminate after the step, and the custom conditions that choose between them.",
    needs: "engine: post actions, and a Terminate that runs cleanup.",
  },
  {
    id: "step.properties.expressions.pre-and-status",
    under: "step.properties.expressions",
    label: "Pre-Expression and Status Expression",
    state: "todo",
    teststand: "An expression evaluated before the step, and one that sets its status.",
    needs:
      "engine: an expression hook before the step. Anvil's `assign` runs after it and can only write a declared local.",
  },
];

// ---------------------------------------------------------------------------
// The Insert Step palette: TestStand's Step Types tree.
//
// A tree, because that is what it is on screen. A leaf with `type` is a step
// type Anvil can make; every other leaf carries its verdict like any other
// entry. A group exists to show what TestStand offers under that heading, so
// it opens even when nothing inside it is built — a heading that will not open
// says nothing. A group with no `items` has not been read off TestStand yet.
//
// No separators: 2019's Insert Step *menu* had them, and 2026Q3's palette is a
// plain tree without. They went when the reference version moved.
// ---------------------------------------------------------------------------

const TODO_STEP_TYPE = "engine: a step type that judges this way, and a loader that accepts it.";

export const INSERT_MENU = [
  {
    id: "insert.tests",
    label: "Tests",
    items: [
      { id: "insert.pass-fail", label: "Pass/Fail Test", type: "pass_fail", state: "built" },
      {
        id: "insert.numeric-limit",
        label: "Numeric Limit Test",
        type: "numeric_limit",
        state: "built",
      },
      {
        id: "insert.multiple-numeric-limit",
        label: "Multiple Numeric Limit Test",
        state: "todo",
        teststand: "Judges several measurements from one module against limits of their own.",
        needs: "engine: a step that judges more than one measurement (ADR-0040 put it out of scope).",
      },
      {
        id: "insert.string-value",
        label: "String Value Test",
        state: "todo",
        teststand: "Compares a string the module returns against an expected one.",
        needs: "engine: a string comparison step type (ADR-0040 put it out of scope).",
      },
    ],
  },
  { id: "insert.action", label: "Action", type: "action", state: "built" },
  {
    id: "insert.ftp-files",
    label: "FTP Files",
    state: "todo",
    teststand: "Transfers files to or from an FTP server as a step.",
    needs: "engine: nothing — this is a step someone writes. Anvil ships no such module.",
  },
  {
    id: "insert.additional-results",
    label: "Additional Results",
    state: "todo",
    teststand: "Adds named expressions to the report as a step of its own.",
    needs: "engine: naming arbitrary expressions to log (see Additional Results, above).",
  },
  {
    id: "insert.sequence-call",
    label: "Sequence Call",
    type: "sequence_call",
    state: "built",
  },
  { id: "insert.statement", label: "Statement", type: "statement", state: "built" },
  {
    id: "insert.property-loader",
    label: "Property Loader",
    state: "elsewhere",
    teststand: "Loads limits and properties from a file at run time, as a step.",
    anvil:
      "The limits sidecar, `--limits fichero.limits.yaml` (RF-30). It is a switch on the run, not a step in the sequence.",
  },
  {
    id: "insert.label",
    label: "Label",
    state: "todo",
    teststand: "A named marker in the step list, for Goto to jump to.",
    needs: "engine: post actions with Goto — a label with nothing to jump to it is decoration.",
  },
  {
    id: "insert.message-popup",
    label: "Message Popup",
    state: "todo",
    teststand: "Shows a dialog to the operator and waits for an answer.",
    needs: "engine: a way to ask the front end something mid-run (the UIMsgs of ui-vs-headless.md).",
  },
  {
    id: "insert.call-executable",
    label: "Call Executable",
    state: "todo",
    teststand: "Runs a program and waits for its exit code.",
    needs:
      "engine: nothing — an executor can do this. Anvil ships no such module, and the engine's sandbox is the open question.",
  },
  {
    id: "insert.flow-control",
    label: "Flow Control",
    items: [
      { id: "insert.flow.if", label: "If", ...flowControl("If") },
      { id: "insert.flow.else", label: "Else", ...flowControl("Else") },
      { id: "insert.flow.else-if", label: "Else If", ...flowControl("Else If") },
      { id: "insert.flow.for", label: "For", ...flowControl("For") },
      { id: "insert.flow.for-each", label: "For Each", ...flowControl("For Each") },
      { id: "insert.flow.while", label: "While", ...flowControl("While") },
      { id: "insert.flow.do-while", label: "Do While", ...flowControl("Do While") },
      { id: "insert.flow.sweep-loop", label: "Sweep Loop", ...flowControl("Sweep Loop") },
      { id: "insert.flow.break", label: "Break", ...flowControl("Break") },
      { id: "insert.flow.continue", label: "Continue", ...flowControl("Continue") },
      { id: "insert.flow.select", label: "Select", ...flowControl("Select") },
      { id: "insert.flow.case", label: "Case", ...flowControl("Case") },
      { id: "insert.flow.goto", label: "Goto", ...flowControl("Goto") },
      { id: "insert.flow.end", label: "End", ...flowControl("End") },
    ],
  },
  {
    id: "insert.synchronization",
    label: "Synchronization",
    items: [],
    state: "todo",
    teststand: "Locks, rendezvous, queues, notifications, semaphores and batch steps.",
    needs:
      "engine: parallelism with hierarchical cancellation (RF-39). The entries under it have not been read off TestStand yet.",
  },
  {
    id: "insert.database",
    label: "Database",
    items: [],
    state: "todo",
    teststand: "Opens databases, runs SQL statements and iterates over result sets as steps.",
    needs:
      "engine: nothing — an executor can do this. The entries under it have not been read off TestStand yet.",
  },
  {
    id: "insert.data-streams",
    label: "Data Streams",
    items: [],
    state: "todo",
    teststand: "Reads and writes files and streams as steps.",
    needs:
      "engine: nothing — an executor can do this. The entries under it have not been read off TestStand yet.",
  },
  {
    id: "insert.labview-utility",
    label: "LabVIEW Utility",
    items: [],
    state: "never",
    teststand: "Steps that drive LabVIEW itself — opening VIs, setting controls, closing panels.",
    why: "roadmap.md lists integration with LabVIEW/CVI as out of scope; escaping the LabVIEW lock-in is half of why Anvil exists (vision.md).",
  },
  {
    id: "insert.io-configuration",
    label: "IO Configuration",
    items: [],
    state: "todo",
    teststand: "Steps that declare and open the instrument IO a sequence uses.",
    needs:
      "engine: an instrument profile of its own. Anvil declares executors, not instruments, and a session lives behind an object reference (ADR-0022). The entries under it have not been read off TestStand yet.",
  },
];

/** Flow control is one gap with fourteen faces, so it gets written once. */
function flowControl(what) {
  return {
    state: "todo",
    teststand: `TestStand's ${what} step, which nests the steps under it.`,
    needs:
      "engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03).",
  };
}

// ---------------------------------------------------------------------------
// The menu bar.
//
// A `action` is one of `app.mjs`'s own, wired by the same name the page's
// `data-action` attributes use; anything else carries a verdict.
//
// A menu Anvil does not have opens onto nothing rather than onto a list of
// invented commands. What TestStand puts under Execute, Debug, Configure and
// Tools is not written down in this repo, and guessing it here would be
// asserting something about NI's product that nobody checked — which is
// exactly what the second-hand warning on ADR-0043 is there to stop. They stay
// empty and greyed until someone reads them off TestStand, the same way the
// Synchronization and Database submenus already do.
// ---------------------------------------------------------------------------

export const MENU_BAR = [
  {
    id: "menu.file",
    label: "File",
    items: [
      { id: "menu.file.new", label: "New", action: "new", state: "built" },
      { id: "menu.file.open", label: "Open…", action: "open", state: "built" },
      { id: "menu.file.save", label: "Save", action: "save", state: "built" },
      { id: "menu.file.save-as", label: "Save As…", action: "save-as", state: "built" },
      { sep: true },
      {
        id: "menu.file.recent",
        label: "Recent Files",
        state: "todo",
        teststand: "Reopens one of the sequence files last edited.",
        needs: "engine: nothing — the editor has to remember paths across sessions, which it does not.",
      },
      {
        id: "menu.file.print",
        label: "Print…",
        state: "elsewhere",
        anvil: "A sequence is a YAML file: print it from the Text view, or from anything that prints text.",
        teststand: "Prints the sequence file through its own report formatting.",
      },
    ],
  },
  {
    id: "menu.edit",
    label: "Edit",
    items: [],
    state: "todo",
    teststand: "Undo, redo, cut, copy, paste, delete and find across the sequence file.",
    needs:
      "engine: nothing — this is the editor's own, and it has no undo stack. The entries have not been read off TestStand yet.",
  },
  {
    id: "menu.view",
    label: "View",
    items: [
      { id: "menu.view.steps", label: "Steps", action: "view-steps", state: "built" },
      { id: "menu.view.text", label: "Text", action: "view-text", state: "built" },
    ],
  },
  {
    id: "menu.execute",
    label: "Execute",
    items: [],
    state: "todo",
    teststand:
      "Runs the sequence — whole, single pass, from a step, or restarted — and terminates a run in progress.",
    needs:
      "engine: cancellation that still runs cleanup, and entry points other than the whole sequence. Run itself is on the toolbar.",
  },
  {
    id: "menu.debug",
    label: "Debug",
    items: [],
    state: "todo",
    teststand: "Break, resume, step into, step over, step out and the breakpoints.",
    needs:
      "engine: stopping at a step, reading variables there and resuming. Nothing in Anvil can stop a run mid-sequence.",
  },
  {
    id: "menu.configure",
    label: "Configure",
    items: [],
    state: "todo",
    teststand: "Station options, report options, database options, adapters and the process model.",
    needs:
      "engine: nothing for some of it — report options are CLI switches today (--json, --csv, --limits). Station-wide settings have nowhere to live.",
  },
  {
    id: "menu.source-control",
    label: "Source Control",
    items: [],
    state: "never",
    teststand: "Checks a sequence file in and out of a source control provider from inside the editor.",
    why: "ADR-0043 §3 puts source control integration out of the parity scope. A sequence is a diffable text file (AP-05), so it is handled by whatever handles the rest of the repository.",
  },
  {
    id: "menu.tools",
    label: "Tools",
    items: [],
    state: "todo",
    teststand: "The sequence analyzer, the deployment utility, import/export and user management.",
    needs:
      "engine: nothing — each of these is a separate tool, and ADR-0043 §3 puts most of them out of the parity scope.",
  },
  {
    id: "menu.window",
    label: "Window",
    state: "elsewhere",
    teststand: "Arranges the editor's child windows, and switches between open sequence files.",
    anvil: "The editor is one window with one file open. In the desktop shell, the platform's own Window menu does this.",
  },
  {
    id: "menu.help",
    label: "Help",
    items: [],
    state: "todo",
    teststand: "The TestStand help, the examples, and the About box.",
    needs: "engine: nothing — the editor ships no help of its own. The Book is a website.",
  },
];

// ---------------------------------------------------------------------------
// The sequence file window: the panes around the step list.
// ---------------------------------------------------------------------------

export const SEQUENCE_WINDOW = [
  { id: "window.sequences", label: "Sequences tab", state: "built" },
  {
    id: "window.sequences.columns",
    label: "Sequence Comment and Requirement columns",
    state: "todo",
    teststand: "Beside each sequence's name, its comment and the requirement it is linked to.",
    needs:
      "engine: a comment on a sequence (a step has one, a sequence does not) and a requirements link, which nothing in Anvil models.",
  },
  {
    id: "window.sequences.callbacks",
    label: "Engine and model callbacks",
    state: "never",
    teststand:
      "Lists the callbacks a sequence file can override — SequenceFilePreStep, ProcessSetup, and the model's entry points — beside MainSequence.",
    why: "ADR-0016 built the process model as a wrapper sequence with no callbacks, and roadmap.md keeps «callbacks que rompen secuencias existentes» out of scope. It is the fragility the product set out to leave behind, not a gap.",
  },
  { id: "window.steps.column.step", label: "Step column", state: "built" },
  { id: "window.steps.column.description", label: "Description column", state: "built" },
  { id: "window.steps.column.settings", label: "Settings column", state: "built" },
  { id: "window.steps.groups", label: "Setup / Main / Cleanup groups", state: "built" },
  { id: "window.steps.collapse", label: "Collapsing a phase", state: "built" },
  {
    id: "window.file-tabs",
    label: "One tab per open sequence file",
    state: "todo",
    teststand: "Each open sequence file gets a tab above the step list, and they are switched between.",
    needs:
      "engine: nothing — the editor holds one document and one engine worker. Several files at once is the editor's own work.",
  },
  {
    id: "window.steps.gutter",
    label: "Breakpoint gutter",
    state: "todo",
    teststand: "The margin down the left of the step list where a breakpoint is set and shown.",
    needs: "engine: stopping at a step. The gutter is drawn; nothing can be set in it.",
  },
  {
    id: "window.palette.templates",
    label: "Templates pane",
    state: "todo",
    teststand: "Holds step, variable and sequence templates to drag into a sequence.",
    needs:
      "engine: nothing — the editor has no template store. A step someone reuses is copy-paste today.",
  },
  {
    id: "window.palette.io-configurations",
    label: "IO Configurations pane",
    state: "todo",
    teststand: "Holds the instrument IO configurations a sequence can drag in, beside Templates.",
    needs: "engine: an instrument profile of its own (see IO Configuration, in the palette).",
  },
  {
    id: "window.palette.adapter",
    label: "Adapter selector",
    state: "elsewhere",
    teststand:
      "A dropdown at the top of the palette — LabVIEW, LabWindows/CVI, C/C++ DLL, .NET, ActiveX/COM, Python, None — deciding which adapter a newly inserted step gets.",
    anvil:
      "Anvil's adapter is gRPC and there is only one (ADR-0003). What varies is the executor that serves a step, and a step names it: it is a field on the step, not a mode of the palette.",
  },
  {
    id: "window.palette.windows-tab",
    label: "Windows tab",
    state: "elsewhere",
    teststand: "Switches the docked pane between the Insertion Palette and the list of open windows.",
    anvil: "There is one window and one file, so there is nothing to switch between.",
  },
];

// ---------------------------------------------------------------------------
// The Variables pane.
// ---------------------------------------------------------------------------

export const VARIABLES_PANE = [
  { id: "variables.locals", label: "Locals", state: "built" },
  { id: "variables.parameters", label: "Parameters", state: "built" },
  { id: "variables.file-globals", label: "FileGlobals", state: "built" },
  {
    id: "variables.station-globals",
    label: "StationGlobals",
    state: "todo",
    teststand: "Variables shared by every sequence run on this station, persisted between runs.",
    needs: "engine: a station-wide scope (RF-32, post-MVP). The engine has three scopes and no fourth.",
  },
  {
    id: "variables.filter",
    label: "Filter by name",
    state: "todo",
    teststand: "Narrows the pane to the variables whose name matches what is typed.",
    needs:
      "engine: nothing — this is the editor's own. It earns its place at a scale Anvil's sequences have not reached.",
  },
  { id: "variables.column.value", label: "Value column", state: "built" },
  { id: "variables.column.type", label: "Type column", state: "built" },
  {
    id: "variables.live-values",
    label: "Live values during a run",
    state: "todo",
    teststand: "Shows what each variable holds as the sequence runs, and lets it be edited at a breakpoint.",
    needs:
      "engine: reading variables out of a running sequence. The event stream carries steps, not scopes.",
  },
];

// ---------------------------------------------------------------------------
// The status bar.
// ---------------------------------------------------------------------------

export const STATUS_BAR = [
  { id: "status.model", label: "Model", state: "built" },
  { id: "status.selected", label: "Step Selected", state: "built" },
  { id: "status.count", label: "Number of Steps", state: "built" },
  {
    id: "status.user",
    label: "User",
    state: "todo",
    teststand: "The logged-in TestStand user, whose privileges decide what the editor allows.",
    needs:
      "engine: users and roles with a login separate from the OS (RF-41, post-MVP). Anvil has no notion of who is running it.",
  },
  {
    id: "status.environment",
    label: "Environment",
    state: "todo",
    teststand: "Which TestStand environment — the set of configuration files — is in force.",
    needs: "engine: station-wide configuration. Anvil configures a run from its CLI switches.",
  },
];

// ---------------------------------------------------------------------------
// Execution and debugging.
//
// This is where the greyed cells stop being tidy. Four of them wait on one
// engine capability — stop, look, resume — and Terminate waits on cancellation
// that still runs cleanup, which `editor/README.md` already calls the editor's
// biggest risk now that Run reaches real hardware. That they come out naming
// the same things is the argument this inventory exists to make.
// ---------------------------------------------------------------------------

const NEEDS_STOP = "engine: stopping a run at a step and resuming it. Nothing in Anvil can pause a sequence mid-flight.";

export const EXECUTION = [
  { id: "execution.window", label: "Execution window", state: "built" },
  { id: "execution.call-stack", label: "Call Stack", state: "built" },
  { id: "execution.trace-subsequences", label: "Trace into subsequences", state: "built" },
  { id: "execution.output", label: "Output tab", state: "built" },
  {
    id: "execution.terminate",
    label: "Terminate",
    state: "todo",
    teststand: "Stops the execution and runs the cleanup of every sequence on the stack.",
    needs:
      "engine: cancellation that still runs cleanup. Today the editor can only kill the worker thread, which leaves the bench exactly as it was, with no cleanup run.",
  },
  {
    id: "execution.abort",
    label: "Abort",
    state: "todo",
    teststand: "Stops the execution immediately, without running any cleanup.",
    needs:
      "engine: a cancellation the engine acknowledges. Killing the thread is not the same thing: the engine never learns the run ended.",
  },
  {
    id: "execution.break",
    label: "Break",
    state: "todo",
    teststand: "Suspends the execution where it is.",
    needs: NEEDS_STOP,
  },
  {
    id: "execution.resume",
    label: "Resume",
    state: "todo",
    teststand: "Continues a suspended execution.",
    needs: NEEDS_STOP,
  },
  {
    id: "execution.step-into",
    label: "Step Into / Over / Out",
    state: "todo",
    teststand: "Advances a suspended execution one step, over a call, or out of the current sequence.",
    needs: NEEDS_STOP,
  },
  {
    id: "execution.breakpoints",
    label: "Breakpoints",
    state: "todo",
    teststand: "Marks steps the execution stops at, and lists them in a window of its own.",
    needs: NEEDS_STOP,
  },
  {
    id: "execution.watch",
    label: "Watch",
    state: "todo",
    teststand: "Shows the value of named expressions while the execution is suspended.",
    needs:
      "engine: reading variables out of a suspended run. It needs the stop of the four above before it needs anything of its own.",
  },
  {
    id: "execution.analysis-results",
    label: "Analysis Results tab",
    state: "never",
    teststand: "Shows what the Sequence Analyzer found in the sequence file.",
    why: "ADR-0043 §3 puts the Sequence Analyzer out of the parity scope. What Anvil checks statically, the loader checks, and it says so in the status bar as the file is typed.",
  },
  {
    id: "execution.report-tab",
    label: "Report tab",
    state: "elsewhere",
    teststand: "Shows the run's report inside the editor, formatted by the model.",
    anvil:
      "Results are open data, not a formatted document (vision.md): a run writes JSON and CSV through a ResultSink, and the editor shows each step's verdict on its own row as it happens.",
  },
];

// ---------------------------------------------------------------------------
// The census
// ---------------------------------------------------------------------------

/** What each of Anvil's step types is called in TestStand's menu. */
export const TYPE_LABELS = Object.fromEntries(
  flattenMenu(INSERT_MENU)
    .filter((e) => e.type)
    .map((e) => [e.type, e.label]),
);

/** Every leaf and group of the insert menu, flat, separators dropped. */
function flattenMenu(entries, out = []) {
  for (const entry of entries) {
    if (entry.sep) continue;
    out.push(entry);
    if (entry.items) flattenMenu(entry.items, out);
  }
  return out;
}

/**
 * The whole inventory as one flat census, which is what the test and the
 * documentation generator read.
 *
 * A menu leaf that names a `type` is `built` by definition, and a group that
 * only holds other entries is not an entry of its own — it is a heading, and
 * it carries a verdict only when it opens onto nothing (Database, Data
 * Streams) or is a decision in itself (LabVIEW Utility).
 */
export function parityEntries() {
  const census = [];

  const flat = [
    ["Step Settings \u25b8 Properties", PROPERTY_PAGES],
    ["Step Settings \u25b8 Properties", PAGE_DETAILS],
    ["Sequence file window", SEQUENCE_WINDOW],
    ["Variables", VARIABLES_PANE],
    ["Status bar", STATUS_BAR],
    ["Execution and debugging", EXECUTION],
  ];
  for (const [area, list] of flat) {
    for (const entry of list) census.push({ ...entry, area });
  }

  const trees = [
    ["Insert Step", INSERT_MENU],
    ["Menu bar", MENU_BAR],
  ];
  for (const [area, tree] of trees) {
    for (const entry of flattenMenu(tree)) {
      // A heading that only groups other entries is not an entry of its own;
      // one that opens onto nothing is, and carries its own verdict.
      if (entry.items && !entry.state) continue;
      census.push({
        ...entry,
        area,
        state: entry.state ?? (entry.type || entry.action ? "built" : undefined),
      });
    }
  }

  return census;
}

/**
 * What a greyed cell says on hover.
 *
 * Two halves, always in the same order: what TestStand does there, and where
 * Anvil is. The second half is what the three verdicts are for — "not yet,
 * waiting on X" and "not planned, because Y" are different things to tell
 * someone deciding whether to migrate, and a single "not implemented" tells
 * them neither.
 *
 * Returns null for a cell that is built: it explains itself.
 */
export function gapTooltip(entry) {
  if (!entry || entry.state === "built") return null;
  const anvil =
    entry.state === "todo"
      ? `Anvil: not yet. Waiting on ${entry.needs}`
      : entry.state === "elsewhere"
        ? `Anvil: ${entry.anvil}`
        : `Anvil: not planned. ${entry.why}`;
  return entry.teststand ? `TestStand: ${entry.teststand}\n${anvil}` : anvil;
}

/** The entry with this id, or undefined. */
export function parityById(id) {
  return parityEntries().find((e) => e.id === id);
}
