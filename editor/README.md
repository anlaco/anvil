# Anvil editor

The graphical sequence editor. It runs **the engine itself** — the same
`anvil-guest.wasm` the native binary embeds (ADR-0011) — in a JavaScript host,
so a sequence can be loaded, validated and reported on with no wasmtime, no
bridge and no bench (AP-07).

Status: **milestone 6, partly** (`docs/roadmap.md`). It opens, edits, validates
live, saves and **runs** — the engine in the tab invokes real steps through the bridge and
reports the same verdict the CLI does — and the run is **visible while it
happens**: the row being executed lights up, and each row keeps the verdict it
produced.

Live progress comes from `--events`, the engine's NDJSON stream (ADR-0029,
ADR-0033). Each line reaches the page as it is written, and the state machine
that reads it is `src/run-state.mjs` — kept out of the DOM on purpose, because
lighting up the wrong row while a unit is on the bench is the failure that
matters, and a pure function is what can be asserted line by line.

A step inside a **subsequence now has a row**, under the call that ran it, and
the **Call Stack** in the Execution pane names what is running and what it is
running under. Both come off `parent_run_id` and `depth`, which the engine has
put on every line since ADR-0033 and nothing read until the execution window
was built.

The honest limit that remains: if the stream loses a line, the view **says so**
rather than showing a run that looks complete — a row whose ancestry was lost
is not filed under a guessed call, and a call stack that ran out of parents
says it is partial instead of starting halfway up.

## Getting it running

```sh
make release          # from the repo root: builds target/wasm32-wasip2/release/anvil-guest.wasm
cd editor
npm install
npm run transpile     # engine guest -> editor/generated/ (git-ignored)
npm test
npm run dev           # http://localhost:5180/
```

`?open=<path>` opens a sequence over HTTP instead of through the file picker,
and the dev server serves the repo's own `ejemplos/`:

```
http://localhost:5180/?open=/ejemplos/basica.yseq
```

That is dev-only, and it is how the editor gets exercised against the real
fixtures — a browser's native file dialog cannot be driven from a test.

`npm run transpile` must be re-run whenever the engine changes; `generated/` is
build output and is not committed.

## What runs where

```
editor/
  scripts/transpile.mjs   engine guest -> JavaScript, via jco
  src/engine.mjs          runs the guest: args in, exit code + stdout/stderr out
  src/wasi/               the WASI imports the guest is given
  generated/              transpiler output (git-ignored)
  test/                   the engine, exercised through the host
```

The engine is **not forked** for the editor. It is the same component, given a
different provider for its WASI imports. Anything that would require changing
the engine to suit the editor is a design mistake, and the reason this holds is
that the editor never asks the engine for something the engine does not already
do (AP-04).

The **AP** are the editor's principles, and they are written down in
[`docs/diseno/principios-del-editor.md`](../docs/diseno/principios-del-editor.md).
AP-04 is the load-bearing one: the editor cannot build a sequence the loader
refuses.

## One Module panel, for every language

TestStand has five — LabVIEW, CVI, C/C++, .NET, Python — because it **inspects**
five different artefacts: a VI's connector pane, a `.py`'s functions, an
assembly's classes. Each format needs its own reader, so each needs its own
panel.

Anvil has one, because the executor already normalised them. Every department
answers the same `Describe` with the same `StepSpec`, so the panel is three
rows for every language:

| TestStand | Anvil |
|---|---|
| Adapter, five of them | — the adapter *is* gRPC, and there is one (ADR-0003) |
| Project Path / Assembly / Module | **Executor** — the department (ADR-0025) |
| VI Path / Function / Method | **Module** — the logical name (ADR-0027) |
| The connector pane | **`StepSpec`**, from `Describe` (ADR-0021) |

The catalog is asked with **`anvil describe <sequence>`**
([ADR-0044](../docs/adr/0044-the-catalog-comes-out-as-data-anvil-describe.md)),
spawned by the shell — **not** through the bridge. The bridge belongs to a
sequence and needs the bench up; this is the case ADR-0028 was written for,
where someone writes a sequence on a laptop with nothing running. In a plain
browser there is no process to spawn, so the panel says what it cannot know.

What that buys, concretely: the module list is what the executor really serves,
and the parameter table is what it really takes. It used to read a parameter's
type off the literal in the YAML with `typeof` — which says what someone typed,
not what the step takes.

## What TestStand has and Anvil does not

`src/paridad.mjs` is the inventory: every page, menu entry and setting
TestStand has, with where Anvil stands on each. It is plain data, and it is
what decides the greyed cells — a gap is never left off the interface, it
appears where TestStand puts it with a tooltip saying what TestStand does there
and which of three things Anvil is:

| | |
|---|---|
| `todo` | Anvil intends to, and names the **engine capability** it waits on. |
| `elsewhere` | Anvil does it another way, and says where. |
| `never` | Out of scope by a decision, and cites it. |

`docs/paridad-teststand.md` is **generated** from that file
(`node scripts/paridad-a-doc.mjs`, and `--check` in CI), so the published page
and the product cannot drift. `test/paridad.test.mjs` holds the rest together:
no greyed cell that is not declared, no declared cell that is never shown, and
no verdict missing the field it owes.

The direction never inverts: **the engine unlocks a cell.** A capability is
built and verified headless, and only then does the editor stop greying it
(AP-13, [ADR-0043](../docs/adr/0043-the-editor-is-laid-out-as-teststand-and-declares-what-it-does-not-do.md)).

## Four things that will bite you

Each of these cost time to find. They are written down so they cost it once.

**`jco` on npm is not jco.** The package named `jco` is a placeholder published
by a third party to head off dependency-confusion attacks; it contains no usable
code. The real one is `@bytecodealliance/jco`.

**We install `jco-transpile`, not `jco`.** The full `jco` package depends on
`componentize-js`, which exists to *build* components out of JavaScript —
something this project never does — and which pulls in `weval` and then
`decompress`, whose Zip Slip advisory has no patched version because the package
is abandoned. Depending only on `@bytecodealliance/jco-transpile` audits clean
(`npm audit`: 0 vulnerabilities) and does everything needed here. Do not
"simplify" this back to `jco`.

**The guest is single-use, so there is one instance per run.** It is a
`wasi:cli/run` command: it starts, runs and exits. Calling `run` twice on one
instance traps with `unreachable` — which surfaces as a confusing failure in the
*second* test, not the first. Hence `instantiation: "async"` in the transpiler
options and a fresh instance in `runEngine`. The native host has the same
constraint and answers it the same way, with a `Store` per guest (ADR-0011).

**We use the shim's browser build, even under Node.**
`@bytecodealliance/preview2-shim` resolves to a Node-specific build under Node's
`node` export condition, and that build routes stdio through an I/O worker whose
streams cannot be swapped for a capture. The browser build takes a plain
`{ write }` handler and is what actually ships, so `src/wasi/` re-exports it by
file path — the package's `exports` map admits no subpath that names it.

## The engine runs on a worker thread

`src/engine-pool.mjs` runs sequences on Web Workers, never on the page's own
thread. The engine is a synchronous WASM component: on the main thread it
freezes the interface — dead buttons, no way to abort — for as long as a
sequence takes. That is invisible for the 80 ms a validate costs and
unacceptable for a real run.

It is written as a pool of one rather than as a single worker because that is
the shape multi-UUT takes: one worker per unit under test, each with its own
engine instance and no shared memory. Web Workers are real OS threads, so this
is genuine parallelism — and the isolation is the property `docs/vision.md`
wants and that TestStand's shared-memory threading does not give.

It does **not** make the engine parallel. The engine is single-threaded
(`crates/motor/src` has no `thread` or `spawn`) and in-sequence parallelism is
post-MVP by decision (`docs/diseno/motor-de-ejecucion.md:137`). N workers are N
sequences, not one sequence going faster — and a front end may not invent what
the engine does not do (ADR-0031).

**Abort is not solved.** `terminateAll()` kills the thread, which is the only
stop available because the engine has no cancellation of its own. That is
survivable while nothing is executed — a validate touches only memory. It will
not be survivable once Run reaches hardware: killing the thread mid-sequence
leaves the bench exactly as it was, with no `cleanup` run.

Since ADR-0043 this sentence is also a cell on screen: **Terminate** and
**Abort** sit on the execution toolbar, greyed, saying what they wait on. Four
of the six controls there wait on one thing — stopping a run at a step and
resuming it — which is the argument for building that before anything else the
inventory lists.

## A step's type, and its module

The step panel shows `type` as a closed list —`action`, `pass_fail`,
`numeric_limit`, `statement`, `sequence_call`— because the type is *how the
step is judged*, and what it calls goes in **Module**
([ADR-0040](../docs/adr/0040-a-step-type-says-how-a-step-is-judged-not-what-it-calls.md)).
It is the same split as TestStand's step-properties and Module tabs.

A new `numeric_limit` is created with `comparison: none`, which reads `done`
until a comparison is chosen: an empty limit that judged nothing would still
have to report *something*, and that something must not be a `pass`. The panel
edits only the fields the limit already uses — which fields each comparison
code takes is a rule of the loader, and inventing them here would produce a
file the engine refuses. Changing the comparison itself is the text view's job.

## Numbers are not localised

Limit and retry fields are `type="text"`, not `type="number"`, on purpose. A
number input renders through the browser's locale, so on a Spanish machine a
limit of `4.5` appears as `4,5` while the YAML says `4.5`. In a test sequencer
that is not cosmetic — the decimal separator is the difference between 4.5 V
and 45 V — so the field shows exactly what the file will contain. Input that
does not parse as a number is refused and the field snaps back, rather than
being written as something else.

## Sockets, and the three threads it takes

The engine reaches executors over gRPC on raw TCP (ADR-0006), a browser has no
TCP, and — the hard part — the engine's calls are **synchronous**:
`blockingRead` returns bytes, `Pollable.block()` returns nothing. There is
exactly one way to block a thread in JavaScript, `Atomics.wait` on a
`SharedArrayBuffer`, and it is forbidden on the page's own thread.

So a run spans three threads:

```
main thread          the interface. Never blocks, never runs the engine.
engine worker        the engine. Blocks in Atomics.wait during a socket call.
network worker       owns the WebSocket to the bridge. Wakes the engine.
```

`src/wasi/channel.mjs` is the shared memory between the last two;
`src/net-worker.mjs` speaks the bridge's frame format;
`src/wasi/sockets.mjs` is the WASI implementation that blocks.

Two consequences worth knowing. The page **must** be cross-origin isolated or
`SharedArrayBuffer` does not exist, which is why `vite.config.mjs` sets
COOP/COEP. And with no bridge attached the shim **throws**, naming what is
missing: answering "connection refused" would let an absent host read as an
executor that was asked and said no — the false red of ADR-0019's Rule 2.

The streams handed to the engine are built with the shim's own `_create`, not
hand-rolled: the generated module checks `ret instanceof InputStream` and
refuses anything else.

## Running against a bridge

```sh
anvil ejemplos/basica.yseq --bridge      # prints a URL with a token
```

Open the URL it prints, adding `&open=/ejemplos/basica.yseq`. Run is disabled
until a bridge is attached, and says so on hover.

The bridge sends the engine's arguments — an `--executor` for each `type: wasm`
executor the host started — in its first frame, because there is no argv to
inject them into when the engine runs in a browser. Without them the engine
reaches none of those executors.

In the desktop app the editor also hands the engine the files the sequence
references beside it on disk — the binary of each `type: wasm` executor and each
subsequence called by path — because the loader reads them
(`src/neighbours.mjs`). In a plain browser it cannot, and the engine says which
file it did not find.

## Tests

`npm test` runs the engine through the host and asserts on what it says. The
house rule applies here as everywhere: **a regression test that has not been
seen to fail is not verified**. All four tests in `test/validate.test.mjs` were
verified by mutation — breaking the thing each one asserts, one at a time, and
confirming that test and only that test went red.
