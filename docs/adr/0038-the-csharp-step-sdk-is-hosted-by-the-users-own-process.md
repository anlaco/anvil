# ADR-0038: The C# step SDK is hosted by the user's own process, and its catalog is compiled

- **Status:** Accepted
- **Date:** 2026-09-12
- **How it was decided:** in this repo, on a commission from management, taking
  technical note ANLACO-NT-0001 v1.2 (*SDK de C# para pasos de Anvil*) as its
  input. That note states its own code citations are second-hand and asks for
  this ADR to verify them first-hand; **that verification was done**, by
  reading and running the code in this repo, and every claim below is cited
  with file and line. Two of the note's decisions are **departed from** here
  — the module name (§4) and the absence of `--steps` (§7) — because they
  collide with ADR-0026 and with §5 respectively; both departures are named
  where they occur. The claims about OpenTAP and the TestStand .NET adapter
  come from the note and are **second-hand**.
- **Relates to:** ADR-0003, ADR-0004, ADR-0008, ADR-0012, ADR-0015, ADR-0019,
  ADR-0020, ADR-0021, ADR-0022, ADR-0024, ADR-0025, ADR-0026, ADR-0027,
  ADR-0028, ADR-0034
- **Scope:** decides **how a step is written in C# and how it reaches the
  engine**: the integration road (a native gRPC executor, not WASM); that the
  server process belongs to the user and we ship a library; that a step is a
  marked method on an ordinary class with no base type; that the module name
  is derived from the class and `[StepModule]` only overrides it; that the
  catalog is built at **compile time**, so a signature the wire cannot carry
  is an error in the author's editor; and that every number written to the
  wire uses the invariant culture, with the alternative made a build error.
  It does **not** change `paso.proto`, `modelo::proto::CONTRACT`, the engine,
  the WASM bridge, the embedded executor or the Python executor, and it raises
  no contract version: nothing decided here is observable by a peer. It does
  **not** put the C# executor in the release artifact — there is nothing of
  ours to ship when the binary is the user's. It does **not** decide how the
  package is published, versioned or signed, nor whether the same SDK later
  serves F# or VB.NET. It does **not** give a C# step a WASM road: §1 defers
  that and says what would end the deferral, but decides nothing about it.
  It does **not** decide anything about **an executor for compiled DLLs**,
  which is a different product with a different catalog problem and needs its
  own ADR. It does **not** resolve references orphaned by a hard abort, which
  ADR-0022 leaves open, and it does **not** make the object store close slots
  by itself.

## Context

The team writing test code in C# wants to sequence it with Anvil. Two roads
were possible: a native executor speaking `paso.proto` over gRPC, the way the
Python one does, or compiling C# to a WASM component and serving it through
the bridge.

The second road is worth naming correctly, because the repo's own prose has
been getting it wrong. The bridge in `executors/wasm/` is a **WASM executor,
not a Rust one**: it instantiates any component exporting the `anvil:step` WIT
world (`executors/wasm/src/main.rs:299-300`) and ADR-0013 already settled that
what produced the component — "C a mano, Rust, Zig, un editor visual, un
tercero" — is opaque to it. The Rust SDK is the only authoring surface written
on top of it so far, not the contract.

What blocks that road for C# is not the language. It is that the bridge
instantiates every component with an **empty WASI context**
(`executors/wasm/src/main.rs:294`, `WasiCtx::builder().build()`) — no preopens,
no network, no sockets — which ADR-0015 §2 decided deliberately, because the
user's WASM is a function and not a server. A step that talks to VISA, to an
IVI driver or to a socket has nowhere to go. And a component keeps no state
between calls, so an open instrument session has nowhere to live from one step
to the next: the bridge rejects an object reference outright (ADR-0022 §8), and
the Rust SDK says so to whoever reads it
(`executors/rust/anvil-step/src/lib.rs:37-41`).

So the WASM road is deferred, and §1 records what would end the deferral rather
than leaving it as folklore.

There is a second thing the context has to carry, because it is the reason this
SDK needs a rule that the Python one does not. **A measurement crosses the wire
as text.** `StepResult.measured_value` is a `string` (`crates/modelo/paso.proto:74`)
and the engine turns it back into a number with `s.parse::<f64>()`
(`crates/modelo/src/proto.rs:412-418`), which accepts only a decimal point.
Python is safe by accident: `str(result.measured_value)`
(`executors/python/server.py:159-160`) is culture-invariant. C# is not.
`double.ToString()` uses `CultureInfo.CurrentCulture`, and under a Spanish
locale — the locale of the machines this is being written on — it produces
`"0,8"`.

The consequence is not a failed parse that somebody notices. It is this chain:

1. `"0,8"` does not parse, so `de_texto` returns `None` and the result reaches
   the engine **with no measurement**.
2. `aplicar_limite` does nothing when there is no measurement, by documented
   design: *"Si `def` no lleva límite, o el paso no trae `valor_medido`, no
   hace nada"* (`crates/motor/src/lib.rs:1238-1239`).
3. The status the step set therefore stands. The test at
   `crates/motor/src/lib.rs:1372-1378` asserts exactly this: a `"pass"` with a
   range limit declared and no measurement stays `"pass"`, and the limit
   fields are left empty.

A step that measured 0.8 A against `limit: { max: 0.80 }` reports **pass**, and
the report cannot even show the threshold that was not applied. That is a
false green produced by the operator's locale, invisible to a CI running in
English, in a product whose failures are broken units. ADR-0019 exists to make
this impossible; this SDK has to pay for it.

## Decision

### 1 — C# enters through a native gRPC executor; the WASM road stays deferred

The C# executor is a gRPC server speaking `paso.proto`, a sibling of the Python
one. The engine dispatches by name and cannot tell them apart; a sequence may
mix both.

The WASM road is **deferred, which is not the same as rejected**. It would need
two things that do not exist, and neither is C# work: a host interface granting
a component the instrument transport — the `wasi-visa` that this repo's
documentation has been announcing as post-MVP since the MVP
(`docs/arquitectura.md:198`, `docs/diseno/integracion-instrumentos.md:46`) and
which exists nowhere, not even as a WIT — and state across calls, which the
component model can express with `resource`. With both, the WASM road would be
the better one, not the deferred one: real isolation with explicitly granted
access, and an artifact identifiable by its hash. Without them it serves only
pure-computation steps.

Two lesser reasons, both real: .NET's component-WASM toolchain is still
experimental and drags a runtime and a garbage collector, and C# has
first-class gRPC codegen — the opposite of `wasi-grpc` v0.1, which has none and
forces the contract to be mirrored by hand (ADR-0006).

### 2 — The server process is the user's; we ship a library

The SDK is a NuGet package. The user creates their own project, references it,
and their executable brings up the gRPC server. We do not ship a binary that
loads the user's compiled assemblies.

This departs from the shape of the Python executor, where `server.py` is
downloaded, never edited, and pointed at a folder. In Python that works because
every step shares one interpreter and one dependency tree. In .NET each library
comes from a project with its own dependency tree resolved against its own
machine; loading several into one process means isolating them one by one, and
the failures that come from getting that wrong do not appear at start-up but
mid-sequence, with the bench energised.

The apparent counter-example is our own WASM bridge, which does load the user's
artifact inside a process of ours. What makes that admissible there is not the
principle but the sandbox (§Context), and .NET has no sandbox to offer.

ADR-0012's promise — that the user edits no line of our code — is kept, and
more than kept: writing your own entry point is not editing ours, and here the
user owns none of ours to edit. What is lost is the convenience of downloading
and running without compiling, and the possibility of Anvil bringing the
executor up by itself the way it does the bridge (ADR-0023).

### 3 — A step is a marked method on an ordinary class, with no base type

A step is a method carrying `[Step]`. There is no base class to inherit, no
registration file, no name table. The same mark serves a static method — a step
with no state — and an instance method — a step operating on a live object; the
SDK tells them apart, not the person writing the step.

**Inheritance is not incompatible with Anvil; it is redundant.** That
distinction matters, because "it does not fit" would be false and the next
person would find out. The engine sees a catalog and calls to `Invoke`; whether
the SDK builds that catalog by scanning marked methods or by scanning derived
types is invisible to it. What does break is what travels *attached* to
inheritance in OpenTAP: public properties as settings serialised alongside the
plan, which would put the acceptance limit inside the compiled step instead of
in the sequence, against ADR-0008 and ADR-0002, and out of reach of anyone
auditing a change of criterion without recompiling. And there is a third
mismatch that decides on its own: OpenTAP's tooling injects the instrument into
the property before calling `Run()`, and in Anvil nothing injects anything —
the reference arrives inside the request's inputs (ADR-0022) — so an inherited
class would have to receive it as an input anyway. Same destination, more
ceremony.

One authoring form, not two. Offering marked functions *and* inheriting classes
would mean two sets of documentation, two places to look for a bug and a
permanent argument about which to use; the case that motivated a second form,
exposing stateless functions, is already covered by static methods.

### 4 — The module name is derived from the class; `[StepModule]` only overrides it

A step is addressed `module/step`, qualified always, never bare
(ADR-0025, ADR-0026, ADR-0027). **Any class holding a `[Step]` is a module**,
and its name is the class name in `snake_case`. `[StepModule("psu")]` is an
optional override, for when the type name is not the address you want.

**This departs from ANLACO-NT-0001 §5**, which presents `[StepModule]` as the
way to *declare* the module. Declaring it contradicts ADR-0026: the module name
is derived and never declared, so that *"a module that is renamed or moved does
not need its steps edited"*
(`executors/python/anvil_step/__init__.py:309-313`). Making the attribute
mandatory would import the duplication ADR-0026 rejected.

Where C# does diverge from Python, deliberately: Python derives the module from
the **file** stem, and C# derives it from the **type**. In C# the file is not
the unit of grouping — partial classes exist, and several classes per file are
ordinary — so deriving from the filename would be the arbitrary choice here.
The rule that survives is the one ADR-0026 actually made: derived by default,
declared only as an exception.

### 5 — The catalog is built at compile time, not by reflection at start-up

The SDK discovers steps with a Roslyn source generator. At start-up the server
already holds its catalog; no reflection runs.

The motive is a promise this repo has already made. ADR-0021 and ADR-0028 hold
that a sequence can be validated without a bench and that an editor can draw
the catalog from a laptop with nothing powered; ADR-0024 holds that the
signature *is* the catalog and is not written twice. With reflection, a step
whose parameter type cannot cross the contract does not show itself until the
server starts, and at worst simply fails to appear in the catalog with nobody
knowing why. With a generator the author gets a squiggle in their editor.

The second motive is concrete: .NET has no API for reading documentation
comments at run time, so a reflection-based SDK ends up demanding a
`[Description]` attribute above every element — writing twice what was already
written, which is precisely what ADR-0024 refuses. A generator reads them,
including each parameter's.

The price is accepted: the generator is another piece, it compiles separately,
it is harder to debug and it needs its own tests.

### 6 — Every number written to the wire uses the invariant culture

One `internal` helper in the SDK turns a `double` into text, and nothing else
in the package does. The public surface offers no overload taking a
pre-formatted string for a measurement, so the user cannot hand us one.

That alone would be a convention. It is made a **build error** instead:
`CA1305` ("Specify IFormatProvider") is raised to `error` for the SDK, and the
package ships a props file that raises it in the **consumer's** project too, so
a step composing a SCPI command under `es-ES` gets the same red squiggle. That
second case — a malformed command reaching an instrument — is worse than a lost
measurement and the technical note did not cover it.

The exact spelling of a number is not contractual, only that
`s.parse::<f64>()` accepts it: Rust writes integral values without a fraction
(`crates/modelo/src/proto.rs:404-410`) and Python writes `str(5.0)` as `"5.0"`,
so the two existing executors already differ. The decimal separator is the part
that is not negotiable.

### 7 — There is no `--steps`, and that is a visible difference between siblings

The CLI is `--port`, `--bind`, `--option KEY=VALUE` and `--list`. Python's
`--steps` cannot exist here: with a catalog compiled into the user's own
executable there is no folder to scan. This is a real difference between two
sibling executors and is written here so a user meets it in the documentation
rather than in an error message.

## Alternatives rejected

**A binary of ours that loads the user's assemblies**, the shape of the Python
executor. Rejected in §2: the dependency trees, the native instrument libraries
that pin architecture and platform version — if we ship the executable we
choose those for everyone and break whoever has a 32-bit driver — and
debugging, since a user who owns the project sets a breakpoint in their own
step instead of attaching to a process of ours already running.

**Reflection for the catalog.** Rejected in §5: it moves the author's error
from their editor to the bench, and it forces a `[Description]` attribute that
duplicates the documentation comment, against ADR-0024.

**Inheriting from a base class, OpenTAP-style.** Rejected in §3 — and rejected
for cost and redundancy, not because it could not work. Saying otherwise would
be false, and the next person to read this would discover it.

**Two authoring forms at once.** Rejected in §3: two mechanisms doing one job.

**Adding `option csharp_namespace` to `paso.proto`.** `paso.proto` has no
`package` and no `csharp_namespace`, so protoc would emit `Value`, `Reference`,
`StepRequest` and `Catalog` into the global namespace of a package that goes
inside the user's code — a collision waiting to happen. Adding the option would
be additive: an option carries no tag, changes no byte on the wire, and would
not raise `CONTRACT`, whose rule is to rise only when *"an old peer's silence
could alter a verdict"* (`crates/modelo/src/proto.rs:28-33`). It is rejected
anyway, because the same problem is solved without touching public surface: the
generated types are emitted `internal` to the SDK assembly, where they can
collide with nothing. That also keeps the user's project free of any
compile-time coupling to protobuf.

> One argument against touching the `.proto` was considered and **does not
> hold**: that it would leave `executors/python/paso_pb2.py` stale. That file
> is ignored, not committed (`executors/python/.gitignore:2`). The reason not
> to touch `paso.proto` is the one above.

**Declaring the module with a mandatory attribute.** Rejected in §4, against
ADR-0026.

## Consequences

- A new Apache-2.0 directory, `executors/csharp/`, under the licence border of
  ADR-0004: what you *use* is AGPL, what you *link* is Apache. The SDK goes
  inside the user's code the moment they write `using Anvil.Step`, so copyleft
  there would be copyleft over their test steps.
- **No change to the contract.** `paso.proto`, `crates/modelo/src/proto.rs`,
  the engine, the bridge and the Python executor are untouched, and `CONTRACT`
  stays at 4 (`crates/modelo/src/proto.rs:33`).
- **The release does not carry it.** `packaging/package.sh:43` ships the WASM
  bridge and not the Python executor, and the C# one could not be shipped even
  in principle: the binary is the user's.
- The `.csproj` compiles `crates/modelo/paso.proto` from an Apache directory
  inside an AGPL tree. This is the contract, not the sequencer, and the Python
  executor already generates its own copies of it under Apache; noted so nobody
  has to re-derive it.
- **A divergence from the Python executor in how a reference is used**, which
  ANLACO-NT-0001 §6 also records: there the step receives the `Reference` and
  resolves it by hand with `ctx.objects.get(...)`, while here the SDK resolves
  it before calling the method, so an instance step never sees a handle. What
  the contract requires is identical in both; only the author's convenience
  differs.
- The two duties of ADR-0022 §7 that no contract can check are discharged by
  the object store: a monotonic key that is never recycled within a lifetime,
  even after closing, and a fresh lifetime minted on every start and published
  in `Catalog.lifetime`.
- `make test` grows `test-executors-csharp`, degrading with a warning when
  `dotnet` is absent, like `test-executors` and `test-editor` already do
  (`Makefile:106-111`). In CI it does **not** degrade: a silently retired suite
  is worse than a red one.
- The generator must target `netstandard2.0`. A generator built for a modern
  target framework **is not loaded by the compiler and says nothing about it**,
  which would present as an empty catalog — legal on the wire, since
  `describes = true` with zero steps is a valid answer, and therefore silent.
  The same hazard applies to steps living in a lazily-loaded referenced
  assembly, so the generator also emits an explicit registration hook.
- **Exercised end to end**, against `anvil 0.4.0` with
  `ejemplos/csharp.yaml` and the example executor in
  `executors/csharp/examples/HelloBench`. The engine read the catalog
  (`6 paso(s) comprobados contra el catálogo de su ejecutor`), ran setup, main
  and cleanup, minted and spent an object reference, and applied the
  sequence's limit: with the supply set to 20 V the run answered
  `[fail] psu/measure_current: 1.25 fuera de rango [0, 0.8]`.
- **The false green of §Context was reproduced, not argued.** With the executor
  started under `LC_ALL=es_ES.utf8`, that same sequence answers `fail`
  correctly. Replacing the invariant culture with the current one in the SDK's
  one formatting site and running it again, unchanged in every other respect,
  the run answers **`=== csharp: pass ===`** — no warning, no measurement on the
  report, nothing to notice. This is why §6 is a build error and not a
  convention.
- **Two defects found by running it and not by reading it**, both silent from
  outside: the gRPC service was being resolved by the dependency container,
  which needs a public constructor while ours is internal, and failed as HTTP
  200 with an empty stream — read by the engine as "it does not describe its
  catalog"; and the executor had no way to log, so that failure had nowhere to
  show itself (`ANVIL_STEP_LOG`).
- **Still not verified:** start-up time and binary size are unmeasured; the
  behaviour of a hard abort against an external executor is untested, as is the
  fate of the references orphaned by one, which ADR-0022 leaves open; the
  package has not been published, and `ci-windows` does not run this suite.
