# ADR-0033: Identity on the event stream is the execution, not the step

- **Status:** Accepted. **Not implemented** by this ADR. `--events` does not
  exist: `grep -n events crates/motor/src/bin/anvil.rs` is empty, and the
  argument `match` at `crates/motor/src/bin/anvil.rs:110-152` is the exhaustive
  list of flags that do.
- **Date:** 2026-09-09
- **How it was decided:** in this repo, implementing ADR-0029. That ADR settled
  the transport — NDJSON on stderr, behind `--events` — and left open what a
  line says about *which* step it refers to. This ADR went through two review
  rounds before being accepted; the second found that a prerequisite the first
  draft leaned on is not achievable (§4a) and that a cut it made was unsafe
  (§3, `plan`). Everything asserted here about today's state is **verified by
  reading the code or by running the binary in this session** and cited with
  file and line. Claims about other products are **second-hand** and marked.
- **Relates to:** ADR-0011, ADR-0019, ADR-0021, ADR-0029, ADR-0030, ADR-0031,
  [#60](https://github.com/anlaco/anvil/issues/60) (implementation notes),
  [motor-de-ejecucion.md](../diseno/motor-de-ejecucion.md)
- **Scope:** decides **what identifies a step on the event stream, and the
  minimum an event line must carry to be honest on a lossy channel**. It does
  **not** change the execution semantics, the verdict, `paso.proto`, the WIT, or
  the YAML sequence format; it does **not** make the channel two-way; and it does
  **not** decide the persistent step id — it reserves its slot and its meaning.
  It **does** change `ResultSink` hook signatures, **does** add one import to the
  engine guest's world (`wasi:random/random`) — both contradicting ADR-0029's
  Scope, §5 — and **does** change the key order of the `--json` report (§3).

## Context

ADR-0029 said each line carries an `event` key and "the fields that hook has".
It did not say what identifies the step. The obvious candidate — the name — does
not survive contact with this code base.

**A name is not unique, and nothing makes it so.** `DefinicionPaso`
(`crates/modelo/src/lib.rs:657-659`) and `ResultadoStep`
(`crates/modelo/src/lib.rs:227-229`) carry `nombre: String` and no other
identity: no id, no index, no path. The loader checks only that a name is not
empty (`crates/cargador/src/lib.rs:2139-2141`) — no uniqueness, no charset, no
length. Exercised: a sequence with two steps both named `verificar_led` in
`main` loads, invokes both, and exits 0.

**The name is already load-bearing where it should not be.** The limits sidecar
matches by step name across the whole program — "*el sidecar casa por nombre de
paso en cualquier secuencia del programa*"
(`crates/cargador/src/lib.rs:2052-2062`). With two steps of one name, one entry
rewrites both. *(Inferred from the recursive by-name match; not exercised.)*
That is a wrong limit reaching a step that runs against hardware, today.

**Position is not identity either.** YAML order maps to engine order 1:1 through
`pasos.into_iter().map(PasoYaml::a_definicion).collect()`
(`crates/cargador/src/lib.rs:821-823`) — true today, asserted by no test as a
contract. `--process-model` then re-roots the tree
(`crates/cargador/src/lib.rs:1700-1705`): the root becomes the process model and
the operator's sequence moves into `archivos`. The same act has a different
position depending on how it was launched.

**The stream is interleaved.** Sub-steps fire the step hooks like any other step;
only the root fires the sequence hooks (`es_raiz` at
`crates/motor/src/lib.rs:689-691`, `758-760`). Nesting reaches
`PROFUNDIDAD_MAX = 64` (`crates/motor/src/lib.rs:667`).

**The hooks fire from exactly one place, and unreached steps are silent.**
`corre_un_paso` (`crates/motor/src/lib.rs:782-863`) is the only caller;
`on_inicio_paso` is its first statement (`:789`), before every early return, so a
step skipped by `disable` (`:795-800`) or by a false precondition (`:804-815`)
emits a full lifecycle with status `skipped`. A step never reached emits nothing
at all: Main breaks at the first failure (`:728-730`), Main is gated entirely
when Setup failed (`:720`), and a transport `Error` propagates with `?` (`:833`)
so that **neither `on_fin_paso` nor `on_fin_secuencia` fires** — the engine's own
rustdoc says so (`crates/motor/src/lib.rs:405-409`).

**Delivery is lossy, and the channel is shared.** ADR-0029 §5 lets a reader skip
lines it does not understand; §6 makes writing best-effort. Fd 2 is not the
engine's alone, and a step name is sanitised nowhere. Exercised: a step whose
name carries a newline **before and after** an embedded JSON object puts that
object alone on its own line on stderr — a syntactically perfect event that never
passed through `serde_json`. (With only a leading newline the loader's diagnostic
appends `': executor '…' does not serve it (…)` after it and the line does not
parse.) **The path that reproduced is the loader's catalogue diagnostic.**
Forging an event is possible today.

**Prior art converges on splitting identity in two.** *(Second-hand.)* OpenTAP —
whose `ResultListener` this repo's `ResultSink` was modelled on — separates
`ITestStep.Id`, persisted in the plan, from `TestStepRun.Id`/`.Parent`, minted per
execution, and keeps the human-readable path out of the listener API. Node's test
runner ships `testId` and `parentId` on every event, citing interleaved siblings.
NI's forums state TestStand's `UniqueStepID` is a GUID in the sequence file, and
that copying a file duplicates it, which `CreateNewUniqueStepIds()` repairs.

## Decision

**Identity on the wire is the execution, minted by the engine. Everything else is
description.** The field set is the minimum that follows from that, plus what a
lossy channel needs to avoid asserting something false. Anything else is left
out: ADR-0029 §5's skip-unknown rule makes *adding* keys backward-compatible.

### 1. Three fields are identity, and no consumer may join on anything else

`run_id`, `step_run_id`, `parent_run_id`.

The reason is not that random ids are more unique. It is that **identity must be
minted by the party that knows the answer, at the moment the answer is certain.**
When the engine decides to invoke a step, it knows which invocation this is. Name,
position and stored id are *claims about the world*, reconstructed from something
another party owns and can change underneath the reader.

**Why now rather than later**, given everything else here was weighed for
deferral: the alternative is not "no identity", it is third-party readers joining
on `name` in the interim — and once dashboards key on the name, its ambiguity
becomes a compatibility problem instead of a bug.

**The `motor` mints them, unconditionally**, whether or not `--events` is on. A
sink cannot: `on_inicio_paso` receives only `&DefinicionPaso`
(`crates/modelo/src/result_sink.rs:53`) and would have to keep its own depth
stack to know the parent, which is the reader-side state §2 exists to avoid. One
16-byte draw per step is noise beside a gRPC round trip.

### 2. The parent link is asserted, not inferred, so that recovery is local

`parent_run_id` carries the `step_run_id` of the enclosing `sequence_call`
**step**, and is `null` at the root.

A reader *can* rebuild nesting without it — the hooks are strictly bracketed and
`depth` moves by one. The field exists so that **a line's meaning does not depend
on reader-side state**, which keeps a failure local: a lost line costs one node,
not a re-derivation.

That is a preference, not a law, and this ADR breaks it four times on purpose:
`run_id`, `events_version` and `seq_total` are each carried once, and **`seq`
only works if the reader remembers the previous line's**. Stated plainly so
nobody quotes the preference back as a rule.

### 3. The field set

Envelope, on every line:

| field | meaning |
|---|---|
| `event` | as ADR-0029 §4 |
| `run_id` | 128-bit random, hex string. **Scopes the run**, so `seq` is unambiguous when two engines write to one fd |
| `seq` | integer from 0, +1 **per emitted line, allocated before the write is attempted**, so a dropped line leaves a gap |
| `elapsed_ms` | integer milliseconds since `sequence_start` |

`elapsed_ms` and not a wall clock: the guest already imports
`wasi:clocks/monotonic-clock`, so a duration costs no dependency and no new
import, whereas RFC-3339 formatting needs a crate this workspace does not have.
It survives a `run.log` read afterwards, which "the consumer stamps on receipt"
does not — and for a tool driving instruments, *"the step that measured 4.9 V
took 41 seconds"* is a diagnosis where *"it passed"* is not.

`step_start` / `step_result` / `step_end` add:

| field | meaning |
|---|---|
| `step_run_id` | 128-bit random, hex string. Opaque: compare for equality only, never order, never parse. Identical on **every** line of one step |
| `parent_run_id` | the enclosing `sequence_call` step's `step_run_id`, or `null` |
| `depth` | integer, 0 at root |
| `name` | descriptive. **Explicitly not identity** |
| `phase` | `"setup"` / `"main"` / `"cleanup"` of the containing sequence |
| `locator` | `{"source": …, "sequence": …, "path": ["main", 3, "setup", 0]}` — where it sits in the program **as loaded**. A hint, not a key (§4c) |
| `step_id` | **reserved** (§6). Emitted only when the sequence declares one |

`step_result` additionally carries the JSON sink's step object, built from
`paso_a_json` (`crates/result_sink/src/json.rs:77`) — **not retyped**, which is
the no-drift requirement ADR-0029 left to implementation time. That object
already contains `name` and `phase` (`:79`, `:81`); they are not emitted twice.
Two changes:

- **`sub_steps` is omitted, and `has_children` says so.** `paso_a_json` nests the
  whole subtree recursively (`:95-104`); on the wire the children have already
  streamed as their own lines, and re-embedding them ships every nested step
  twice, to depth 64. But the object is otherwise identical to the report's, so a
  reader reusing a report parser would silently see a leaf.
  **`has_children` is `sub_pasos` present and non-empty — not "the step is a
  `sequence_call`".** A call skipped by `disable` or by a precondition, and the
  two early returns in `ejecuta_sequence_call`, all leave `sub_pasos: None` on a
  step whose type is `SequenceCall`; a subsequence with no steps leaves
  `Some(vec![])`. A builder keying on the type would assert "no children" about a
  call with forty declared children it never entered.
- **`final` (boolean).** `true` marks the result the verdict was taken from.
  **Today it is a constant `true`** — every exit path of `corre_un_paso` fires
  `on_resultado` exactly once, and retries loop below the sink
  (`crates/motor/src/lib.rs:363-384`) — and it is emitted anyway so that a later
  design may add `final: false` lines for superseded attempts without breaking a
  reader that already filters on it. It replaces a prose rule ("do not assume one
  `step_result` per `step_start`"), which is unenforceable and untestable.

`sequence_start` carries `sequence`, `source`, `process_model`,
`engine_version`, **`events_version`** and **`plan`**.

- **`events_version`**: an integer, `1` as of this ADR, incremented **only** when
  the meaning of an existing event or field changes in a way a conforming reader
  cannot absorb. Not `engine_version`, which moves for unrelated reasons.
  ADR-0029 §5 covers additive change; this covers the rest.
- **`plan`**: the declared step names, phases and locators, per sequence,
  including nested subsequences. It is the **denominator** — which steps the
  program declares — delivered before anything can be lost. §4b says why a
  stream cannot answer that question on its own.

`sequence_end` carries the aggregate as the JSON sink renders it, plus
`seq_total`, **which counts its own line**. A reader that has `seq_total` and no
gap has the whole stream.

**Key order on the wire is fixed and is part of the format:** `event`, `name`,
`status`, `phase`, `message`, the measurement fields, then the identity envelope,
then `locator`, `inputs`, `outputs`. A person scanning a wrapped terminal must
reach the verdict without reading to the end of the line — today's `--json` is
alphabetical, because `serde_json::Map` is a `BTreeMap` here, and `status` is the
**last** of a step object's eleven keys (verified by running `--json` on
`ejemplos/basica.yaml`).

It needs `serde_json`'s `preserve_order`, which **reorders the whole `--json`
document** into insertion order — a visible change to a public artefact, so it
goes in the `CHANGELOG`. And because the mandated order interleaves the envelope
into the middle of `paso_a_json`'s object, it cannot be produced by inserting
into one map; the mechanism is in #60.

### 4. What a lossy, shared channel needs

#### (a) "Never block" is not achievable in the guest, and the exposure is not new

The first draft made it a prerequisite that the events sink "must never block on
a write". **It cannot be built there.** In the `wasmtime-wasi` this repo pins
(47.0.3, `packaging/anvil-host/Cargo.toml:9-10`), the stdio stream's
`check_write` returns a **constant** `Ok(1024 * 1024)` regardless of the state of
fd 2, and `write` then does a blocking `write_all` on the host thread
(`wasmtime-wasi-47.0.3/src/cli/stdout.rs`). `check-write` carries no
backpressure for inherited stdio, and the pollable is always ready. The guest
has no threads either — `crates/result_sink/src/reintento.rs:5-8` says so, and it
is why that helper has no `sleep`. A bounded buffer inside the guest is not a
fallback: with no way to observe fullness, draining it is the same blocking
write.

So this ADR does **not** require it. What it requires instead:

1. **The exposure is stated, not invented.** Blocking on stdio is not new:
   `SinkConsola` writes stdout and the engine `eprintln!`s throughout, both
   blocking, on every non-`--quiet` run. `--events` raises the volume; it does
   not open the door.
2. **`--events <path>` exists as the control.** A regular file does not stall the
   way a stopped pipe reader does, and it is also the answer to the sensitivity
   problem below.
3. **The bound is documented**: a cap on line length, so one pathological
   `outputs` cannot produce a line that takes unbounded time to drain.
4. **The native host may fix it properly, later.** `inherit_stdio()`
   (`packaging/anvil-host/src/main.rs:91`) can be replaced by a custom
   `OutputStream` over a bounded queue drained by a host thread — the host is
   native and has threads. That would make drops real, and only there. It is not
   required by this ADR, and if it is ever done, this ADR's drop semantics must
   be revisited.

Consequently **drops are not routine**: with blocking writes the engine stalls
rather than dropping. `seq` gaps stay exceptional, and a consumer that meets one
is right to treat it as a fault.

**The events sink must still swallow its own IO errors.** Every sink today
reports a write failure with `eprintln!` (`crates/result_sink/src/consola.rs:28`,
`csv.rs:68`, `json.rs:67`), which panics if that write also fails — latent for the
file sinks, fatal for a sink whose destination *is* stderr. That one is real and
is a prerequisite.

#### (b) Loss must be detectable, and `plan` is what covers the tail

`seq` turns silent corruption into "I lost three lines here". But **a gap needs a
line on both sides**: loss after the last line you hold produces no gap, and the
terminator that would close that hole, `seq_total`, rides on `sequence_end` —
exactly the line that does not exist when a run aborts on a transport error
(`crates/motor/src/lib.rs:405-409`). A reader that lost the last forty lines sees
a stream shaped identically to a clean abort.

`seq` is therefore a numerator and cannot answer *"which declared steps never
ran?"* — the question asked after an abort, before touching the unit. `plan` is
the denominator, and it arrives on line one. It is also the only field here that
is **not cleanly additive later**: a reader that has already adopted "no lines
means never ran" as its coverage rule must be rewritten when `plan` lands, not
extended.

Two further facts a reader needs, stated so the format is not read as promising
more than it does. A **dangling `step_start` with no gap and no `sequence_end`
means the engine stopped**, not that lines were lost — the step that hit a
transport error emits `step_start` and nothing else
(`crates/motor/src/lib.rs:833`). And **an aborted run has no `sequence_end` at
all**.

#### (c) The forgery is fixed at the source, and the source is wider than one line

A first draft required `run_id` to be an unguessable nonce every reader must
check. Rejected: it puts a permanent MUST on every third-party reader — a check
that is pure cost until something goes wrong, and therefore the first thing an
implementer skips — to mitigate an injection this repo can fix. It would not have
closed it anyway: the claim that executors "inherit the write end of fd 2 and
never read it" is false when stderr is a terminal, since a tty is a bidirectional
character device. *(Reasoned from POSIX semantics; not exercised.)*

The fix is **that no user-controlled string reaches fd 2 unescaped, from any
writer this repo controls**. That is wider than the first draft's wording, which
named only the spawned executors:

- **The loader's diagnostics** — the path that reproduced. Closed by sanitising
  names at load.
- **The embedded step executor**, `crates/ejecutor_pasos/src/main.rs:138`, prints
  `pet.name` raw, once per invocation per attempt. It is **not a child process**:
  it is a WASI guest run in-process on a thread whose ctx comes from a builder
  calling `inherit_stdio()` (`packaging/anvil-host/src/main.rs:91`). Closing it
  means giving *that* builder a non-inheriting stderr, which is a different
  change from piping a child — and must not be applied to the engine's own
  builder, which is where the events go.
- **The WASM bridge's diagnostics** embed the module path. Closed by piping the
  one child that exists.
- **`grpc` executors declared by address** are never spawned by the host at all
  (`std::process::Command::new` appears once in non-test code, at
  `packaging/anvil-host/src/main.rs:357`). If the operator started one in the
  same terminal it shares the tty, and no host-side change reaches it.

**Neither is a one-liner, and calling them prerequisites does not make them
small.** Sanitising names is a **breaking load-time change** needing its own
`CHANGELOG` line. Piping the one child **moves a hazard** rather than removing
it: an undrained 64 KiB pipe blocks the process holding the instrument, nothing
today calls `wait()` on that `Child`, and the relay must escape what it forwards
or an executor forges an event through the host's own hand. Each needs its own
design; both are in #60.

The character rule, because it is a decision and not a detail: **reject C0 and
U+007F** (the injection vector), **C1**, **U+2028/U+2029** (line terminators to
JavaScript readers, and the editor is an in-repo consumer), and the **bidi
overrides and isolates U+202A–U+202E, U+2066–U+2069** — that last group not as an
injection defence but because a name carrying U+202E renders the rest of the line
reversed in a terminal, which defeats §3's whole reason for fixing key order.
Reject nothing else: `/` is load-bearing for executor namespacing, spaces occur,
and non-ASCII must stay — an operator writes `medir_tensión`. A multi-line name
has nowhere to render and needs no escape hatch; an escape hatch is the vector.

`run_id` stays, for the smaller and honest job named in §3: scoping `seq`.

#### (d) A locator is an attribute, never a key

Two different questions: *correlation* ("is this `step_result` about the same act
as that `step_start`?"), answered by ids and meaningless outside the run; and
*location* ("which row do I highlight?"), answered by `locator` and meaningless
across edits. One field for both would claim they coincide, and they do not.

Two honest limits, because this repo will hit both. The editor addresses a step
as `{ phase, index }` (`editor/src/app.mjs:49`, `#stepNode(phase, index)` at
`editor/src/document.mjs:313`), so on day one `locator` is the only field it can
use. And the control channel ADR-0029 §7 still owes must name a step **before it
runs**, when no execution id exists. So the rule is: **do not join events to each
other by locator.** Addressing a step in a document by its position is a
different act and is legitimate.

**`locator.source` does not exist in the model and is real work.**
`DefinicionSecuencia` (`crates/modelo/src/lib.rs:757-777`) has no source path,
and neither the root nor an inline subsequence has a key in `Programa.archivos`
to recover one from — so `source` has to be threaded through the engine and
injected from the CLI for the root. The decision this ADR owes it: **an inline
subsequence takes the `source` of the file that declares it**, with `sequence`
naming the inline key. The threading is in #60.

**Detecting a stale locator is left open.** A first draft put a `program_digest`
on `sequence_start`; it was specified wrongly — under `--process-model` the loaded
program is the process model with the operator's sequence below it, so its digest
can never equal one computed from the file the editor holds, and the signal would
fire on every production run. The corrected form is per file, not per program,
and belongs with the `source` work above. Until then a locator is advisory.

### 5. What this changes that ADR-0029 said it would not

> **Qualifies [ADR-0029](0029-the-engine-streams-its-execution-as-ndjson.md)
> §Scope (2026-09-05) in two places.**
>
> **"does not change the `ResultSink` lifecycle".** The hook **order** and the
> lifecycle's shape are unchanged; the **signatures** are not. `on_inicio_paso`
> receives only `&DefinicionPaso` (`crates/modelo/src/result_sink.rs:53`), and
> the phase is stamped onto the result after it — the hook fires at
> `crates/motor/src/lib.rs:789`, the `sella` closure is defined at `790-793` — so
> a sink cannot know the phase, the ordinal or the parent at `step_start`.
>
> **"it adds no import to the engine guest's world".** It adds one:
> `wasi:random/random`. The guest today imports `wasi:clocks/monotonic-clock`,
> `wasi:clocks/wall-clock` and `wasi:random/insecure-seed`, and **not**
> `wasi:random/random` — verified in `editor/generated/interfaces/` and by
> scanning `target/wasm32-wasip2/release/anvil-guest.wasm`. Zero *host* lines is
> not zero import: `wasmtime_wasi::p2::add_to_linker_sync`
> (`packaging/anvil-host/src/main.rs:271`) links it unconditionally, `wasmtime
> run` supplies it, and the editor's transpile map resolves `wasi:random/*` by
> wildcard (`editor/scripts/transpile.mjs:109`) — so nothing needs wiring, but
> the guest's world does grow, which is what ADR-0029 promised it would not.
> Unlike the private `anvil:editor/events` that ADR rejected, this is a
> `wasi:cli`-world standard every generic host already provides, so the property
> that rejection protected — the guest stays runnable by any host — is intact.

The engine is closer to this than it looks: `Contexto`
(`crates/motor/src/lib.rs:768-773`) already carries `fase` and `profundidad`;
only the ordinal needs `.enumerate()` on the three phase loops
(`crates/motor/src/lib.rs:703`, `:721`, `:735`).

**The randomness costs no new crate**: `wasip2 1.0.4+wasi-0.2.12` is already in
the root `Cargo.lock` via `wasi-grpc` and exposes `random::random`. In the
browser the shim resolves to `crypto.getRandomValues`, so the ids are real
randomness in all three hosts. The trade-offs between that and `getrandom` are in
#60.

**Nine `impl ResultSink` exist, all in-repo; five are test fixtures, and five
compile unchanged on the trait's default bodies**
(`crates/modelo/src/result_sink.rs:47-66`). The identity travels as one struct in
`modelo` carrying `&str`s, a `usize` and `Fase`, so **`modelo` gains no
dependency**, which its module doc (`result_sink.rs:7-11`) is explicit about. The
call sites are enumerated in #60.

### 6. The persistent step id is reserved, not decided

`step_id` has a slot and a fixed meaning: **absent means *unknown*, never *the
same* and never *different*.** Emitted only when the sequence declares one.

A stored id is coming — the sidecar defect above is it leaking already, and
per-step trending, breakpoints that outlive a session, and requirement
traceability all need a handle that survives a rename. But it is a **file format**
decision: `PasoYaml` is `deny_unknown_fields`
(`crates/cargador/src/lib.rs:256-257`), so an `id:` key fails the load today —
exercised, `campo desconocido 'id' en main[0]`, exit 1 — and the hard questions
are who mints it, what a copy-paste means, and what a duplicate means at load.
Its own ADR. It replaces nothing here: one stored step produces many executions.

## Alternatives discarded

**The name, or a path of names.** Names are not unique (exercised), so a UI
keying on one highlights the wrong row with no signal. Kept on every line as a
human field.

**An ordinal path as the join key** — `main.1/main.0`. Attractive because it is
already the editor's handle. Rejected because it makes position identity: it
rests on a YAML-to-engine ordering no test asserts, and couples a public wire
format to one consumer's current internal structure. It survives as `locator`.

**An unguessable `run_id` that readers MUST check.** §4c.

**A per-guest bounded buffer to make writes non-blocking.** §4a — with no way to
observe fullness it never drops, so it is the same blocking write with more code.

**`attempt`/`attempts_total` on the wire.** See §Consequences: putting them only
on the wire creates the drift this ADR forbids.

**A persistent id in the YAML as the answer.** Deferred, §6.

## Consequences

- **A live view can be built that cannot silently point at the wrong step**, and
  whose failure modes are visible: a gap in `seq`, or a `plan` entry with no
  lines.
- **Retries stay invisible, and that is a defect this ADR records without
  fixing.** `ejecuta_con_reintentos` (`crates/motor/src/lib.rs:363-384`) loops
  below the sink, `ResultadoStep` has no attempt field, and no sink renders one —
  the "(intento 2)" a person sees is free text the demo step writes into its own
  message (`crates/pasos_demo/src/lib.rs:39`), not something Anvil records. So a
  unit that passed on the fourth attempt and one that passed first try produce
  identical reports: Rule 3 of ADR-0019 broken **today**. Putting `attempt` only
  on the wire would make the live view carry a fact the archived report cannot.
  When fixed it belongs in `ResultadoStep`, so `paso_a_json` renders it and both
  channels agree — a change to the report's shape, which is public surface.
  **Its own change, and it should not wait long.**
- **The `--json` report's key order changes** (§3), and that goes in the
  `CHANGELOG`.
- **`--events` carries the same data as `--json` and is as sensitive**: evaluated
  parameters, so instrument addresses and channels, and object references naming
  an executor and its payload (`crates/result_sink/src/json.rs:145-150`). Unlike
  `--json` it goes wherever stderr goes, and ADR-0029 §3 keeps `--quiet` from
  silencing it. That is Rule 3 of ADR-0019 working as intended; `--events <path>`
  (§4a) is what turns the warning into a control.
- **The design must stay testable**, which a lossy, randomly-identified stream is
  not by default: the id source and the writer both have to be injectable, or
  this repo's "seen it fail" rule cannot be honoured for the one property that
  matters most — that a dropped line leaves a gap. The seams, and the rest of the
  implementation notes this ADR deliberately does not carry, are in **#60**.
- **A reference reader belongs in this repo.** Gap detection, parent resolution
  and a `final` filter, published to people who will write dashboards, gets six
  divergent implementations unless the safe path is the easy one — the same
  reasoning ADR-0012 and ADR-0021 applied to executors.
- **The browser host cannot stream anything yet.** `capture()`
  (`editor/src/engine.mjs:61-80`) accumulates every chunk and decodes only after
  the guest returns (`:152`). Live progress needs incremental decoding and line
  reassembly across chunk boundaries. That cost is identical for every scheme
  considered and is not small.
- **The bridge must not grow an events frame kind.** In `--bridge` mode the
  engine runs in the tab, so its stderr is already there; ADR-0030's component
  holds a network boundary and needs no second job.
- **The events channel is not the record.** `--json` is. No chaining, no MAC; a
  saved `run.log` can be edited afterwards by anyone.
