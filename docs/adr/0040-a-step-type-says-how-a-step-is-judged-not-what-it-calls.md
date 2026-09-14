# ADR-0040: A step type says how a step is judged, not what it calls

- **Status:** Accepted. **Not implemented** by this ADR.
- **Date:** 2026-09-15
- **How it was decided:** in this repo, in a conversation with the person who
  develops here, who writes test sequences in NI TestStand daily. It started
  from a question about the editor ("how do I make a gRPC step that is a
  pass/fail test?") and from noticing that in TestStand a Pass/Fail Test can
  call a code module while in Anvil `pass_fail` cannot. Their decisions, in
  their words: the step types must be TestStand's, starting with the three they
  use almost always — **Action, Pass/Fail Test and Numeric Limit Test** — and
  done very well before adding more; the type is **explicit**; and Action,
  Pass/Fail with a module, Statement and Sequence Call behave **as in
  TestStand**. The four points where "as in TestStand" did not settle it for
  Anvil (§Decision 7–10) were put to them one by one and they accepted each.
  Everything asserted here about **this repo** was verified in that session by
  reading the code, cited with file and line, and the three behaviours in
  §Context 3 were also **run** against the published `anvil 0.6.0`. Everything
  about **TestStand** is **second-hand** — read from NI's current online
  documentation (links at the end), not exercised in TestStand.
- **Relates to:** ADR-0003, ADR-0005, ADR-0008, ADR-0009, ADR-0010, ADR-0018,
  ADR-0019, ADR-0020, ADR-0021, ADR-0033,
  [motor-de-ejecucion.md](../diseno/motor-de-ejecucion.md),
  [modelo-de-pasos.md](../diseno/modelo-de-pasos.md)
- **Scope:** decides **what a step's `type` means** — how the step is judged —
  and that **what it calls** is a separate, optional part of the step; which
  types exist for now (`action`, `pass_fail`, `numeric_limit`, plus `statement`
  and `sequence_call`, which already exist); a new step status, **`done`**; the
  shape of a numeric limit; and that `type: grpc` and today's
  `limit: {type: range|comparison}` stop being accepted. It does **not** change
  `paso.proto`, the WIT, or what an executor may return — `done` is produced by
  the engine only. It does **not** add Multiple Numeric Limit Test, String Value
  Test, TestStand's flow-control step types, custom step types, loops, or
  TestStand's per-step *Step Failure Causes Sequence Failure* switch. It does
  **not** decide the comparison precision (TestStand documents 14 digits), nor
  change how retries interact with a limit (§Consequences i). It does **not**
  change when a whole sequence is `pass` beyond making `done` neutral
  (§Decision 6). It does **not** implement anything, and it does **not** decide
  the editor's interface for these types beyond that it must offer them.

## Context

### 1. Today `type` mixes two questions

A step's `type` accepts `grpc` (the default), `statement`, `sequence_call` and
`pass_fail` (`crates/cargador/src/lib.rs:277-280`, default at `:321-324`;
`crates/modelo/src/lib.rs:473-483`). Those four answer two different questions:

- `grpc` answers **what is called**: an executor, by the step's name.
- `pass_fail` answers **how it is judged**: an expression the engine evaluates
  (ADR-0018).

And `grpc` is not even accurate about what it calls. An executor can be
`embedded`, `wasm` or `grpc` (`crates/cargador/src/lib.rs:86`), and a
`type: grpc` step calls whichever its `executor:` names — the embedded one when
it names none.

How a `grpc` step is judged is **inferred**, not declared. It takes the status
its executor returns, and if the sequence gave it a `limit` and the executor
returned a measurement, the engine may turn a `pass` into a `fail`
(`crates/motor/src/lib.rs:1249-1283`, ADR-0008). Nothing in the sequence says
whether a given step is meant to test something or only to do something.

### 2. `pass_fail` was modelled on half of TestStand's Pass/Fail Test

ADR-0018 took TestStand's Pass/Fail Test as its precedent and says so: its data
source is a Boolean expression the sequencer evaluates — "*Es exactamente esta
decisión*". But that is the Pass/Fail Test **without a code module**. With one,
the module sets the Boolean and the step judges on it. ADR-0018 then made a
module impossible: "*Un `pass_fail` no admite … `ejecutor` (es motor-side).
Todos, error de carga*" (§Recortes). So a step that calls an executor and
passes or fails on what it answers has to be a `grpc` step with no limit, and
reads as nothing in particular.

### 3. Three behaviours this leaves, run against `anvil 0.6.0`

**A limit with no measurement is silently not applied.** `verificar_led`
(embedded executor) returns a status and no measurement. Given an impossible
limit, the step and the sequence pass:

```yaml
name: limit_without_measurement
main:
  - name: verificar_led
    limit: { type: range, min: 1000, max: 2000 }
```
```
=== limit_without_measurement: pass ===
  [pass] verificar_led: led encendido
exit 0
```

That is ADR-0008's own rule — "*Si no hay `valor_medido` (pass/fail, action sin
medida), el límite no aplica*" — implemented at
`crates/motor/src/lib.rs:1255-1259`. It was reasonable when a limit was an
optional extra on a step of unknown intent. Once a step can say it is a
numeric limit test, a declared limit that was never checked is a green that
asserts something it did not check: Rule 1 of ADR-0019.

**Doing something reads as having checked something.** A `statement` that only
assigns a variable reports `[pass] set_x: statement ok`
(`crates/motor/src/lib.rs:1135`), and so does an executor step that only
powers a supply. The report cannot tell "checked, and it is good" from "did
it".

**A limit failure is not retried.** With `retries: 3` and a limit the
measurement misses, the executor is called once (`paso pedido: medir_voltaje
intento=1`) and the step fails. The retry loop looks at the executor's status
and the limit is applied after it (`crates/motor/src/lib.rs:369-384`), while
[motor-de-ejecucion.md](../diseno/motor-de-ejecucion.md) says the engine
retries "*mientras el paso no pase*". Noted here because numeric limits are in
scope; **not decided here** (§Consequences i).

### 4. What TestStand does — second-hand, from NI's documentation

Not exercised in TestStand. Quotes are from NI's current online help.

- **Two independent choices per step.** The *module adapter* decides what is
  called (LabVIEW, C/C++ DLL, .NET, Python, or `<None>`). The *step type*
  decides how the result is judged. Pass/Fail, Numeric Limit and Action are
  "*Step Types You Can Use with Any Module Adapter*".
- **Action.** "*Calls code modules that do not perform tests but instead
  perform actions necessary for testing, such as initializing an instrument. By
  default, Action steps do not pass or fail. … the status for an Action step is
  Done or Error unless the code module specifically sets another status for the
  step or the step calls a subsequence that fails.*"
- **Pass/Fail Test.** "*Calls a code module that makes its own pass/fail
  determination*": `Step.Result.PassFail` True → Passed, False → Failed. Its
  *Data Source* is a Boolean expression defaulting to `Step.Result.PassFail`,
  customisable "*when you do not want to set the value … in the code module*",
  e.g. `RunState.PreviousStep.Result.Numeric * Locals.Attenuation > 12`.
- **Numeric Limit Test.** "*Calls a code module that returns a single
  measurement value*" (`Step.Result.Numeric`) and compares it with limits. It
  has *Units* (display only, "*do not scale the measured value or affect the
  limit comparison*") and a numeric *Data Source*. "*You can use a Numeric Limit
  Test step without a code module, which is useful when you want to limit-check
  a value you have already acquired.*" Comparison types: EQ, NE, GT, LT, GE, LE
  (one limit); GTLT, GELE, GELT, GTLE (inside two limits); LTGT, LEGE, LEGT,
  LTGE (outside two limits); EQT (a nominal value ± a threshold given as a
  percentage, parts per million or a delta); and *No Comparison*, where
  "*TestStand makes no Pass/Fail determination, and sets the status to Passed
  automatically*".
- **Statement.** Executes expressions, no module. "*By default, Statement steps
  do not pass or fail*": Error if the expression fails, otherwise Done.
- **Sequence Call.** "*Always uses the Sequence Adapter.*" A failing subsequence
  fails the call step, and the failure propagates up the chain.
- **Statuses.** Passed, Failed, Error, Done ("*the step completed without
  setting the status*"), Terminated, Skipped, Running, Looping. A sequence fails
  when a step with *Step Failure Causes Sequence Failure* fails, enabled by
  default for most step types. Done fails nothing.

## Decision

**A step's `type` says how the step is judged. What the step calls — its
module — is a separate part of the step, and for some types optional.**

### 1 — The module is `module` and `executor`, and nothing else

A step that calls something names it with **`module`**, the step name its
executor serves, and optionally **`executor`**, as today (default: the embedded
one). `module` is what travels as `StepRequest.name`
(`crates/modelo/paso.proto:54-55`) and what an executor's catalog lists
(ADR-0021); dispatch by name (ADR-0003) is unchanged.

**`name` becomes only the step's name** — what the report, the editor and the
events show. Two steps can call the same module with different `inputs` and say
so: `name: Measure 5V rail` and `name: Measure 12V rail`, both
`module: dmm/measure_voltage`.

`inputs` and `assign` work on any step that has a `module`, as they do today on
a `grpc` step.

### 2 — `type` is explicit and required

There is no default type. A step without `type` is a load error. The error
names the choice, because "*which type?*" is exactly the question this ADR wants
asked.

### 3 — `action`

- **Requires** a `module`.
- The executor's `pass` becomes **`done`**. Its `fail` stays `fail` and its
  `error` stays `error` — TestStand's "*unless the code module specifically sets
  another status*". `skipped` stays `skipped`.
- A `limit` or a `condition` is a load error: an action judges nothing.

### 4 — `pass_fail`

- `module` is **optional**; `condition` is **optional**; **at least one** of the
  two is required.
- **Module, no condition:** the executor's status is the verdict — `pass` →
  `pass`, `fail` → `fail`, `error` → `error`.
- **Condition, no module:** today's `pass_fail`, unchanged (ADR-0018).
- **Both:** the module runs first; then `condition` — TestStand's *Data Source*
  — decides, and can read the module's `result`. If the executor returned
  `error`, the step is `error` and the condition is not evaluated: a broken bench
  is not judged (ADR-0019, Rule 2).
- A `limit` is a load error.

### 5 — `numeric_limit`

- `module` is **optional**; **`value`**, a numeric expression — TestStand's
  numeric *Data Source* — is **optional** and defaults to
  `result.measured_value`; at least one of the two is required. Without a
  module, `value` is required: it limit-checks something already acquired.
- `limit` is **required**, in the shape of §7.
- **No measurement is `error`,** not a pass (§Context 3). This narrows ADR-0008:
  its "*si no hay `valor_medido` … el límite no aplica*" no longer holds for a
  step that declares itself a numeric limit test.
- The executor's `fail` and `error` stand, and the limit can only turn a `pass`
  into a `fail` — ADR-0008's rule, unchanged.

### 6 — A new status, `done`, produced by the engine only

- `done` means **the step completed and judged nothing**. It is what `action`
  gives on success (§3), what `statement` now gives on success instead of `pass`,
  and what a numeric limit with `comparison: none` gives (§8).
- It is **not** added to what an executor may return
  (`crates/modelo/src/lib.rs:116`, `crates/modelo/paso.proto:67-72`): like
  `inconclusive`, only the engine writes it. An executor that returns `"done"`
  is `error`, as any unrecognised status already is (ADR-0019, Rule 2).
  **`paso.proto` does not change.**
- **It is neutral.** It does not stop `main`, does not stop `setup` from
  succeeding, does not make a sequence fail, and is not retried. In the severity
  scale it sits where `skipped` sits (`crates/modelo/src/lib.rs:163-173`).
- **It is not `pass`.** Counts of passed steps do not include it.
- When a whole sequence is `pass` does not change beyond this: the rules of
  ADR-0019 stand as they are.

### 7 — The shape of a numeric limit is TestStand's

```yaml
limit: { comparison: GELE, low: 4.75, high: 5.25, units: V }
```

- `comparison` takes TestStand's codes: `EQ`, `NE`, `GT`, `LT`, `GE`, `LE` (with
  `low` only); `GTLT`, `GELE`, `GELT`, `GTLE`, `LTGT`, `LEGE`, `LEGT`, `LTGE`
  (with `low` and `high`); `EQT` (with `nominal`, `lower`, `upper` and
  `threshold: percent | ppm | delta`, computed as TestStand documents); and
  `none` (§8).
- A field a comparison does not use is a load error, as today's limits already
  refuse `op` on a range (`crates/cargador/src/lib.rs:370-374`).
- `units` is text for the report. It does not scale or affect the comparison.
- This replaces `limit: {type: range, min, max}` and
  `limit: {type: comparison, op, expected}` everywhere they appear, the limits
  sidecar included (ADR-0008, RF-30).

### 8 — `comparison: none` is `done`, not `pass`

TestStand's *No Comparison* sets **Passed**. Anvil records the measurement, its
units and the absence of a comparison, and gives **`done`**. A step that
compared nothing does not report that it passed: that is Rule 1 of ADR-0019,
which exists because the August beta shipped sequences that went green on
checks they never made. The difference from TestStand is the label, not the
behaviour — it fails nothing and the sequence goes on.

### 9 — `statement` and `sequence_call` keep their meaning

- `statement`: no module; `done` on success, `error` otherwise (TestStand's
  Statement). The change is `pass` → `done`.
- `sequence_call`: no module, as TestStand's always-Sequence-Adapter step; its
  status is its subsequence's aggregate, as today (ADR-0010).

### 10 — `type: grpc` and the old limits are refused, with the way out

Existing sequences are not translated silently, because the translation is not
knowable when loading: a `grpc` step without a limit may be an action or a
pass/fail test depending on what its executor returns. Loading one is a load
error that says what to write — `numeric_limit` for a `grpc` step with a
`limit`, `action` or `pass_fail` otherwise, `name` → `module` — and an old
`limit` is an error that shows its TestStand form (`range` →
`comparison: GELE, low, high`; `comparison, op: le, expected` →
`comparison: LE, low`).

This is a change to the public surface, so it ships in a **minor** release
(the versioning rule in `CHANGELOG.md`).

## Alternatives rejected

- **Leave `type` as it is and add `action`.** Cheapest, and it keeps every
  sequence loading. Rejected because it keeps `grpc` as a type that means "what
  it calls" next to types that mean "how it is judged", which is the confusion
  this started from, and it leaves the judgement of every executor step
  inferred.
- **Infer the type when it is missing, from `limit` and `module`.** Rejected by
  the person deciding ("*explícito*"), and on the facts: whether a step without
  a limit is an action or a pass/fail test is decided by its executor at run
  time, so the inference would be a guess that changes the verdict.
- **Keep `grpc` as a deprecated alias.** Rejected for the same reason: there is
  no single type it is an alias for.
- **Copy *No Comparison* as `pass`.** Rejected in §8.
- **Keep `name` as both the step's name and its module.** Cheaper, and ADR-0003
  dispatches by name. Rejected because it makes two steps calling one module
  indistinguishable in the report, and because the separation of what is called
  from how it is shown is the axis this ADR is about. ADR-0033 already notes
  that names are not unique and should not be load-bearing.
- **Put `done` on the wire so an executor can return it.** Rejected: an action's
  "nothing to judge" is a property of the step's type, which the sequence
  declares; the executor does not know whether it was called by an action or by
  a pass/fail test, and should not need to.

## Consequences

**a. It is a breaking change to the sequence format, and it is meant to be.**
Every sequence in `ejemplos/`, `process_models/` and `docs/book/listings/`, and
the sequences users already wrote, stop loading
until they are rewritten. The load errors of §10 are what makes that a
five-minute job rather than a guessing game, so they are part of the change,
not a nicety.

**b. ADR-0018 is narrowed, and is annotated there.** `pass_fail` may now call a
module; its §Recortes on `ejecutor` no longer holds.

**c. ADR-0008 is narrowed, and is annotated there.** A limit on a
`numeric_limit` step with no measurement is `error` (§5); and the limit's shape
is §7's.

**d. The report gains a status and fields.** `done` appears in the console, JSON,
CSV and event outputs, and results carry `module`, the comparison and `units`.
The console format is part of the spec (RNF-08), so that change is deliberate
and goes in the CHANGELOG.

**e. The limits sidecar changes shape with the limits (§7).** It matches by step
name (`limites_sin_aplicar`, `crates/cargador/src/lib.rs:2041-2051`;
ADR-0033); after §1 that is the step's `name`, not its `module`.

**f. The editor has to offer the types and the module.** Today its palette
offers `grpc`, `statement`, `sequence_call` and `pass_fail`
(`editor/src/document.mjs:33`) and its step settings cannot add a limit. It must
offer `action`, `pass_fail` and `numeric_limit` with a module, a condition, a
value and the comparisons of §7.

**g. The Anvil Book is rewritten where it teaches steps.** Chapters 5, 7 and 9
use `grpc` steps, `limit: {type: …}` and `pass_fail`; `docs/book/check.sh` will
fail on them until they are, which is the point of it.

**h. More TestStand types have a place to go.** Multiple Numeric Limit Test and
String Value Test are further values of `type`, each with its own judgement,
and no longer need to be squeezed into `limit`. Each is its own decision.

**i. Retries and limits are left as they are, and that is written down.** A
limit failure is not retried today (§Context 3), contrary to
`motor-de-ejecucion.md`. Whether a numeric limit test should retry on a limit
failure is a question about execution semantics of its own; it is filed as an
issue, not decided here.

## Sources (second-hand, not exercised)

- [Action Step](https://www.ni.com/docs/en-US/bundle/teststand-api-reference/page/tsref/action-step.html)
- [Pass/Fail Test Step](https://www.ni.com/docs/en-US/bundle/teststand-api-reference/page/tsref/pass-fail-test-step.html)
- [Data Source Edit Tab (Pass/Fail Test)](https://www.ni.com/docs/en-US/bundle/teststand-api-reference/page/tsref/data-source-edit-tab.html)
- [Numeric Limit Test Step](https://www.ni.com/docs/en-US/bundle/teststand-api-reference/page/tsref/numeric-limit-test-step.html)
- [Limits Tab – Numeric Limit Test](https://www.ni.com/docs/en-US/bundle/teststand-api-reference/page/tsref/limits-tab-numeric-limit-test-edit-tabs.html)
- [Statement Step](https://www.ni.com/docs/en-US/bundle/teststand-api-reference/page/tsref/statement-step.html)
- [Sequence Call Step](https://www.ni.com/docs/en-US/bundle/teststand-api-reference/page/tsref/sequence-call-step.html)
- [Step Status](https://www.ni.com/docs/en-US/bundle/teststand/page/step-status.html)
