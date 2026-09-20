# Anvil against TestStand, feature by feature

**This page is generated** from `editor/src/paridad.mjs` by
`editor/scripts/paridad-a-doc.mjs`. Do not edit it by hand: the same file
decides what the Sequence Editor greys out, and the point of generating
this is that the two cannot disagree. CI checks it.

The reference is **NI TestStand 2026Q3**, and everything asserted here about
TestStand is **second-hand**: read from NI's documentation and from screenshots,
not exercised in TestStand. The decision behind all of this is
[ADR-0043](adr/0043-the-editor-is-laid-out-as-teststand-and-declares-what-it-does-not-do.md);
the editor's own rules are in [diseno/principios-del-editor.md](diseno/principios-del-editor.md).

## Where it stands

| | Count |
|---|---:|
| Not yet | 59 |
| Done another way | 9 |
| Not planned | 3 |
| Built | 39 |
| **Total** | **110** |

A gap is never simply left off the interface: it appears where TestStand puts
it, greyed, saying what TestStand does there and which of these it is. The
engine is what unlocks a cell — a feature is built and verified headless, and
only then does the editor stop greying it.

## Not yet

Anvil intends to have these. Each one names the engine capability it is waiting on — so several rows naming the same capability are a priority, not a coincidence.

### Step Settings ▸ Properties

| | In TestStand | Waiting on |
|---|---|---|
| **Switching** | Drives an NI Switch Executive route around the step. | engine: an instrument-switching layer. Anvil has no switching. |
| **Synchronization** | Locks, rendezvous, queues, notifications, semaphores and batches around the step. | engine: parallelism with hierarchical cancellation (RF-39, post-MVP). Anvil runs one UUT at a time, so none of it exists yet. |
| **Requirements** | Links the step to a requirement in a requirements management tool. | engine: a requirements link. Nothing in Anvil models one. |
| **Additional Results** | Logs extra named expressions into the report, per step. | engine: naming arbitrary expressions to log. In Anvil a measurement reaches the report through `assign`, on the Expressions page. |
| **Run Mode ▸ Force Pass / Force Fail, Record Results, Step Failure Causes Sequence Failure** | Forces a verdict on the step, keeps it out of the report, or stops its failure from failing the sequence. | engine: per-step verdict overrides — which ADR-0040 put out of its own scope. |
| **Loop types** | Fixed number, While, Do While and Pass/Fail count loops, their pass and fail counts, and the Looping status. | engine: per-step looping. Retries is not a loop; issue #76 asks how the two should meet. |
| **On Pass / On Fail actions** | Goto step, Call sequence or Terminate after the step, and the custom conditions that choose between them. | engine: post actions, and a Terminate that runs cleanup. |
| **Pre-Expression and Status Expression** | An expression evaluated before the step, and one that sets its status. | engine: an expression hook before the step. Anvil's `assign` runs after it and can only write a declared local. |

### Step Settings ▸ Module

| | In TestStand | Waiting on |
|---|---|---|
| **Remembering a catalog between sessions** | Keeps a step's parameters on screen with nothing running, because they live in a type file on the developer's machine. | engine: a per-module hash on the wire. ADR-0025 §5 says a catalog cache is invalidated by hash and not by re-describing, and no hash crosses `Describe` today — the executors compute one and only `--list` shows it. |
| **Dropping a module file onto the step** | A `.vi` dragged onto the step fills its path and reads its terminals. | engine: nothing — `anvil describe` answers the signature already (ADR-0044). What is missing is turning a dropped file into a declared department, which the executors can already be pointed at. |

### Sequence file window

| | In TestStand | Waiting on |
|---|---|---|
| **Sequence Comment and Requirement columns** | Beside each sequence's name, its comment and the requirement it is linked to. | engine: a comment on a sequence (a step has one, a sequence does not) and a requirements link, which nothing in Anvil models. |
| **One tab per open sequence file** | Each open sequence file gets a tab above the step list, and they are switched between. | engine: nothing — the editor holds one document and one engine worker. Several files at once is the editor's own work. |
| **Breakpoint gutter** | The margin down the left of the step list where a breakpoint is set and shown. | engine: stopping at a step. The gutter is drawn; nothing can be set in it. |
| **Templates pane** | Holds step, variable and sequence templates to drag into a sequence. | engine: nothing — the editor has no template store. A step someone reuses is copy-paste today. |
| **IO Configurations pane** | Holds the instrument IO configurations a sequence can drag in, beside Templates. | engine: an instrument profile of its own (see IO Configuration, in the palette). |

### Variables

| | In TestStand | Waiting on |
|---|---|---|
| **StationGlobals** | Variables shared by every sequence run on this station, persisted between runs. | engine: a station-wide scope (RF-32, post-MVP). The engine has three scopes and no fourth. |
| **Filter by name** | Narrows the pane to the variables whose name matches what is typed. | engine: nothing — this is the editor's own. It earns its place at a scale Anvil's sequences have not reached. |
| **Live values during a run** | Shows what each variable holds as the sequence runs, and lets it be edited at a breakpoint. | engine: reading variables out of a running sequence. The event stream carries steps, not scopes. |

### Status bar

| | In TestStand | Waiting on |
|---|---|---|
| **User** | The logged-in TestStand user, whose privileges decide what the editor allows. | engine: users and roles with a login separate from the OS (RF-41, post-MVP). Anvil has no notion of who is running it. |
| **Environment** | Which TestStand environment — the set of configuration files — is in force. | engine: station-wide configuration. Anvil configures a run from its CLI switches. |

### Execution and debugging

| | In TestStand | Waiting on |
|---|---|---|
| **Terminate** | Stops the execution and runs the cleanup of every sequence on the stack. | engine: cancellation that still runs cleanup. Today the editor can only kill the worker thread, which leaves the bench exactly as it was, with no cleanup run. |
| **Abort** | Stops the execution immediately, without running any cleanup. | engine: a cancellation the engine acknowledges. Killing the thread is not the same thing: the engine never learns the run ended. |
| **Break** | Suspends the execution where it is. | engine: stopping a run at a step and resuming it. Nothing in Anvil can pause a sequence mid-flight. |
| **Resume** | Continues a suspended execution. | engine: stopping a run at a step and resuming it. Nothing in Anvil can pause a sequence mid-flight. |
| **Step Into / Over / Out** | Advances a suspended execution one step, over a call, or out of the current sequence. | engine: stopping a run at a step and resuming it. Nothing in Anvil can pause a sequence mid-flight. |
| **Breakpoints** | Marks steps the execution stops at, and lists them in a window of its own. | engine: stopping a run at a step and resuming it. Nothing in Anvil can pause a sequence mid-flight. |
| **Watch** | Shows the value of named expressions while the execution is suspended. | engine: reading variables out of a suspended run. It needs the stop of the four above before it needs anything of its own. |

### Insert Step

| | In TestStand | Waiting on |
|---|---|---|
| **Multiple Numeric Limit Test** | Judges several measurements from one module against limits of their own. | engine: a step that judges more than one measurement (ADR-0040 put it out of scope). |
| **String Value Test** | Compares a string the module returns against an expected one. | engine: a string comparison step type (ADR-0040 put it out of scope). |
| **FTP Files** | Transfers files to or from an FTP server as a step. | engine: nothing — this is a step someone writes. Anvil ships no such module. |
| **Additional Results** | Adds named expressions to the report as a step of its own. | engine: naming arbitrary expressions to log (see Additional Results, above). |
| **Label** | A named marker in the step list, for Goto to jump to. | engine: post actions with Goto — a label with nothing to jump to it is decoration. |
| **Message Popup** | Shows a dialog to the operator and waits for an answer. | engine: a way to ask the front end something mid-run (the UIMsgs of ui-vs-headless.md). |
| **Call Executable** | Runs a program and waits for its exit code. | engine: nothing — an executor can do this. Anvil ships no such module, and the engine's sandbox is the open question. |
| **If** | TestStand's If step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **Else** | TestStand's Else step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **Else If** | TestStand's Else If step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **For** | TestStand's For step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **For Each** | TestStand's For Each step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **While** | TestStand's While step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **Do While** | TestStand's Do While step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **Sweep Loop** | TestStand's Sweep Loop step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **Break** | TestStand's Break step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **Continue** | TestStand's Continue step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **Select** | TestStand's Select step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **Case** | TestStand's Case step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **Goto** | TestStand's Goto step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **End** | TestStand's End step, which nests the steps under it. | engine: nested control flow. The engine has `precondition`, `condition`, `statement` and a subsequence call, and no blocks — so the editor offers none either (AP-03). |
| **Synchronization** | Locks, rendezvous, queues, notifications, semaphores and batch steps. | engine: parallelism with hierarchical cancellation (RF-39). The entries under it have not been read off TestStand yet. |
| **Database** | Opens databases, runs SQL statements and iterates over result sets as steps. | engine: nothing — an executor can do this. The entries under it have not been read off TestStand yet. |
| **Data Streams** | Reads and writes files and streams as steps. | engine: nothing — an executor can do this. The entries under it have not been read off TestStand yet. |
| **IO Configuration** | Steps that declare and open the instrument IO a sequence uses. | engine: an instrument profile of its own. Anvil declares executors, not instruments, and a session lives behind an object reference (ADR-0022). The entries under it have not been read off TestStand yet. |

### Menu bar

| | In TestStand | Waiting on |
|---|---|---|
| **Recent Files** | Reopens one of the sequence files last edited. | engine: nothing — the editor has to remember paths across sessions, which it does not. |
| **Edit** | Undo, redo, cut, copy, paste, delete and find across the sequence file. | engine: nothing — this is the editor's own, and it has no undo stack. The entries have not been read off TestStand yet. |
| **Execute** | Runs the sequence — whole, single pass, from a step, or restarted — and terminates a run in progress. | engine: cancellation that still runs cleanup, and entry points other than the whole sequence. Run itself is on the toolbar. |
| **Debug** | Break, resume, step into, step over, step out and the breakpoints. | engine: stopping at a step, reading variables there and resuming. Nothing in Anvil can stop a run mid-sequence. |
| **Configure** | Station options, report options, database options, adapters and the process model. | engine: nothing for some of it — report options are CLI switches today (--json, --csv, --limits). Station-wide settings have nowhere to live. |
| **Tools** | The sequence analyzer, the deployment utility, import/export and user management. | engine: nothing — each of these is a separate tool, and ADR-0043 §3 puts most of them out of the parity scope. |
| **Help** | The TestStand help, the examples, and the About box. | engine: nothing — the editor ships no help of its own. The Book is a website. |

## Done another way

Anvil solves these, differently. A missing cell here is not a missing feature; it is a feature that lives somewhere else.

### Step Settings ▸ Properties

| | In TestStand | In Anvil |
|---|---|---|
| **Property Browser** | Browses the step's raw property tree. | The raw form of a step is the YAML itself — use the Text view. |

### Step Settings ▸ Module

| | In TestStand | In Anvil |
|---|---|---|
| **Call Type** | Chooses how the module is called — VI Call, Class Member Call, Property Node Call. | There is one kind of call to an executor: a step request over gRPC (ADR-0003). Which technology is behind it is the department's business, not the sequence's. |

### Sequence file window

| | In TestStand | In Anvil |
|---|---|---|
| **Adapter selector** | A dropdown at the top of the palette — LabVIEW, LabWindows/CVI, C/C++ DLL, .NET, ActiveX/COM, Python, None — deciding which adapter a newly inserted step gets. | Anvil's adapter is gRPC and there is only one (ADR-0003). What varies is the executor that serves a step, and a step names it: it is a field on the step, not a mode of the palette. |
| **Windows tab** | Switches the docked pane between the Insertion Palette and the list of open windows. | There is one window and one file, so there is nothing to switch between. |

### Execution and debugging

| | In TestStand | In Anvil |
|---|---|---|
| **Report tab** | Shows the run's report inside the editor, formatted by the model. | Results are open data, not a formatted document (vision.md): a run writes JSON and CSV through a ResultSink, and the editor shows each step's verdict on its own row as it happens. |

### Insert Step

| | In TestStand | In Anvil |
|---|---|---|
| **Property Loader** | Loads limits and properties from a file at run time, as a step. | The limits sidecar, `--limits fichero.limits.yaml` (RF-30). It is a switch on the run, not a step in the sequence. |
| **LabVIEW Utility** | Steps that drive LabVIEW itself — opening VIs, setting controls, closing panels. | Modules of a LabVIEW department, not step types of Anvil's. A LabVIEW executor opens the project and serves its VIs over gRPC like any other department (ADR-0025); the inspecting happens inside it, and the engine never learns what a VI is. What stays out of scope is a LabVIEW adapter inside the test executive — which is what roadmap.md's line means. |

### Menu bar

| | In TestStand | In Anvil |
|---|---|---|
| **Print…** | Prints the sequence file through its own report formatting. | A sequence is a YAML file: print it from the Text view, or from anything that prints text. |
| **Window** | Arranges the editor's child windows, and switches between open sequence files. | The editor is one window with one file open. In the desktop shell, the platform's own Window menu does this. |

## Not planned

Deliberately out of scope. Each one cites the decision that put it there, and changing one of these takes an ADR.

### Sequence file window

| | In TestStand | Why not |
|---|---|---|
| **Engine and model callbacks** | Lists the callbacks a sequence file can override — SequenceFilePreStep, ProcessSetup, and the model's entry points — beside MainSequence. | ADR-0016 built the process model as a wrapper sequence with no callbacks, and roadmap.md keeps «callbacks que rompen secuencias existentes» out of scope. It is the fragility the product set out to leave behind, not a gap. |

### Execution and debugging

| | In TestStand | Why not |
|---|---|---|
| **Analysis Results tab** | Shows what the Sequence Analyzer found in the sequence file. | ADR-0043 §3 puts the Sequence Analyzer out of the parity scope. What Anvil checks statically, the loader checks, and it says so in the status bar as the file is typed. |

### Menu bar

| | In TestStand | Why not |
|---|---|---|
| **Source Control** | Checks a sequence file in and out of a source control provider from inside the editor. | ADR-0043 §3 puts source control integration out of the parity scope. A sequence is a diffable text file (AP-05), so it is handled by whatever handles the rest of the repository. |

## Built

Anvil has these. They are listed so the inventory is a census rather than a list of holes.

### Step Settings ▸ Properties

| | |
|---|---|
| **General** | `step.properties.general` |
| **Run Options** | `step.properties.run-options` |
| **Looping** | `step.properties.looping` |
| **Post Actions** | `step.properties.post-actions` |
| **Expressions** | `step.properties.expressions` |
| **Preconditions** | `step.properties.preconditions` |

### Step Settings ▸ Module

| | |
|---|---|
| **Executor picker** | `module.executor` |
| **Module picker** | `module.module` |
| **Parameter table** | `module.parameters` |
| **Connector pane** | `module.connector` |

### Sequence file window

| | |
|---|---|
| **Sequences tab** | `window.sequences` |
| **Step column** | `window.steps.column.step` |
| **Description column** | `window.steps.column.description` |
| **Settings column** | `window.steps.column.settings` |
| **Setup / Main / Cleanup groups** | `window.steps.groups` |
| **Collapsing a phase** | `window.steps.collapse` |

### Variables

| | |
|---|---|
| **Locals** | `variables.locals` |
| **Parameters** | `variables.parameters` |
| **FileGlobals** | `variables.file-globals` |
| **Value column** | `variables.column.value` |
| **Type column** | `variables.column.type` |

### Status bar

| | |
|---|---|
| **Model** | `status.model` |
| **Step Selected** | `status.selected` |
| **Number of Steps** | `status.count` |

### Execution and debugging

| | |
|---|---|
| **Execution window** | `execution.window` |
| **Call Stack** | `execution.call-stack` |
| **Trace into subsequences** | `execution.trace-subsequences` |
| **Output tab** | `execution.output` |

### Insert Step

| | |
|---|---|
| **Pass/Fail Test** | `insert.pass-fail` |
| **Numeric Limit Test** | `insert.numeric-limit` |
| **Action** | `insert.action` |
| **Sequence Call** | `insert.sequence-call` |
| **Statement** | `insert.statement` |

### Menu bar

| | |
|---|---|
| **New** | `menu.file.new` |
| **Open…** | `menu.file.open` |
| **Save** | `menu.file.save` |
| **Save As…** | `menu.file.save-as` |
| **Steps** | `menu.view.steps` |
| **Text** | `menu.view.text` |
