# ADR-0042: What ADR-0040 left unsaid

- **Status:** Accepted. **Not implemented** by this ADR.
- **Date:** 2026-09-15
- **How it was decided:** in this repo, while planning ADR-0040's
  implementation with the person who develops here. Reading the code against
  ADR-0040 turned up four places where its text and today's code cannot both
  hold, or where it says nothing. The person's standing rule for this work is to
  **follow TestStand** and depart only for a written reason; the answers below
  apply it. Everything about **this repo** was verified by reading the code,
  cited with file and line. Everything about **TestStand** is **second-hand**,
  and one point is **not contrasted** — each is marked where it appears.
- **Relates to:** ADR-0008, ADR-0009, ADR-0018, ADR-0019, ADR-0040, ADR-0041,
  [#76](https://github.com/anlaco/anvil/issues/76)
- **Scope:** decides, for steps under ADR-0040, **when `result` is visible** to
  `condition` and `value`; **in what order** `assign` and the judgement run;
  **which steps may retry**; and what happens to a **sidecar limit** that lands
  on a step that cannot take one. It does **not** change what ADR-0040 decided
  about types, statuses or the shape of a limit. It does **not** decide whether
  a limit failure is retried: that stays with #76. It does **not** change how a
  sequence's own status is aggregated.

## Context

### 1. `condition` cannot read `result` today, and ADR-0040 needs it to

ADR-0040 §4 lets a `pass_fail` with a module decide on the module's answer —
TestStand's Data Source, "*e.g. `RunState.PreviousStep.Result.Numeric *
Locals.Attenuation > 12`*" — and §5 gives `numeric_limit` a `value` that defaults
to `result.measured_value`. Today both halves refuse that: the loader rejects
`result.*` inside `condition` (`crates/cargador/src/lib.rs:893-901`), and the
engine clears `result` before evaluating a `pass_fail`
(`crates/motor/src/lib.rs:1161-1162`, "*Un `pass_fail` no tiene `resultado.*`
propio*"). That was true when a `pass_fail` could not have a module
(ADR-0018). It is not after ADR-0040.

### 2. The order of `assign` and the judgement is not written anywhere

A step with a module can both copy values out (`assign`) and be judged on them
(`condition`, `value`). ADR-0040 does not say which runs first, and the answer
changes what a `condition` reading `locals.*` sees.

### 3. Retries on `pass_fail` are refused for a reason that a module removes

`crates/cargador/src/lib.rs:2356-2362` refuses `retries > 1` on a `pass_fail`
because it "*evalúa una expresión, el resultado no cambia entre intentos*", and
`:2366-2372` refuses `assign` because "*un pass_fail no produce `result.*`*".
Both reasons are about a step with no module.

### 4. The limits sidecar injects after the checks

`aplicar_limites` sets `paso.limite` on any step whose name matches
(`crates/cargador/src/lib.rs:2016-2033`), after the type-and-field checks have
run. Under ADR-0040 a limit belongs only to a `numeric_limit`, so a sidecar
could build a step the loader would refuse if it were written in the sequence.

### 5. What TestStand does — second-hand

- On NI's forum, a user (not identified as NI): "*In TestStand the
  PostExpression is carried out before the StatusExpression which is what
  populates Step.Result.Status*"
  ([1](https://forums.ni.com/t5/NI-TestStand/Conditional-Operator-not-working-as-expected/td-p/3751612)).
  And a retired NI employee, hedged: "*I believe that the limit expression
  evaluation on 'numeric limit test steps' occurs during the status expression
  evaluation (so, the final part of the step)*"
  ([2](https://forums.ni.com/t5/NI-TestStand/Step-limits-expression-evaluation/td-p/2230048)).
  Neither is NI's documentation.
- The full order is Table 3-4, *Order of Actions that a Step Performs*, in the
  TestStand Reference Manual. **Not contrasted:** the table itself could not be
  read in this session, so where module output parameters are copied relative
  to the Post-Expression is not confirmed here.
- A Pass/Fail Test and a Numeric Limit Test with a module can loop and retry
  like any step that calls a module (ADR-0040 §Context 4 quotes them as step
  types "*You Can Use with Any Module Adapter*").

## Decision

### 1 — With a module, `condition` and `value` read `result`

On a step with a `module`, `result` is the module's answer when `condition` and
`value` are evaluated, and the loader accepts `result.*` in both — with the same
field check `assign` already has, so `result.outputs.<name>` is checked against
the catalog. Without a module there is no `result`, and `result.*` in
`condition` or `value` stays a load error. `precondition` never sees `result`:
it runs before the module (ADR-0009).

### 2 — `assign` runs before the judgement

After the module returns, `assign` copies values out, and then `condition` or
`value` and the limit decide the status. This matches what is reported,
second-hand, of TestStand: a Post-Expression before the Status Expression
(§Context 5). A `condition` that reads a `locals`
variable `assign` just wrote sees the new value.

If the module returned `error`, neither `assign` nor the judgement runs, as
ADR-0040 §4 already says for the condition: a broken bench is not judged
(ADR-0019, Rule 2), and values from a failed call are not copied.

### 3 — A step with a module may retry

`action`, `pass_fail` and `numeric_limit` **with** a module accept `retries`,
and retry on the module's answer exactly as a `grpc` step does today. A
`pass_fail` or `numeric_limit` **without** a module keeps refusing `retries > 1`,
for the reason the loader already gives. Whether a limit failure consumes a
retry is **not** decided here (#76).

A `pass_fail` with a module accepts `assign`; without one, it keeps refusing it.

### 4 — A sidecar limit on a step that is not a `numeric_limit` is a load error

The limits sidecar is checked after it is applied. A limit that lands on an
`action`, `pass_fail`, `statement` or `sequence_call` is refused naming the step
and the sidecar, as if the sequence had written it.

## Alternatives rejected

- **`condition` reads the module's result through a separate name** (for example
  `module.*`). Rejected: `result` already means "*the answer of the call this step
  made*" in `assign`, and two names for one thing is how a reader ends up unsure
  which to use.
- **Judgement before `assign`.** It would let a `condition` see the values from
  before the call, which is never what a test means, and it is not TestStand's
  order.
- **Ignore a misplaced sidecar limit.** A limit that silently does not apply is
  the failure ADR-0040 §Context 3 was written against.

## Consequences

**a. ADR-0018's loader checks become conditional on having no module**, and
`evalua_pass_fail` stops clearing `result` for a step that has one.

**b. A sequence made only of `done` steps aggregates to `pass`** and `anvil`
exits 0 (`crates/modelo/src/lib.rs:398-411`). ADR-0040 §6 leaves the aggregate
unchanged on purpose, and this ADR does not reopen it. It is recorded because it
is the nearest thing left to a green that asserts a check, and the next decision
about the aggregate should start from it.

**c. When Table 3-4 is read**, §2 is checked against it, and this ADR is
annotated if the order differs.
