# ADR-0026: The Python executor is a department too, and its steps are addressed by module

- **Status:** Accepted, and **implemented** the same day.
- **Date:** 2026-09-02
- **How it was decided:** in this repo, on a commission from management —
  *"ahora el mismo trabajo para el ejecutor python"* — right after ADR-0025 was
  implemented for the WASM executor. The one decision that was **not** the
  repo's to make was put to management, because the three possible answers
  break different things: it chose to **qualify always**, over qualifying with
  a bare-name alias and over an explicit per-executor mode. Everything asserted
  about today's state is **verified by reading the code in this repo** and cited
  with file and line; the runs quoted in §Consequences were **verified by
  running** them on 2026-09-02.
- **Relates to:** ADR-0003, ADR-0012, ADR-0019, ADR-0021, ADR-0022, ADR-0025
- **Scope:** decides how a step served by the Python executor is **named**, and
  makes its `Registry` hold modules. It does **not** change `paso.proto`, the
  engine, the WASM bridge or the embedded executor; it does **not** make module
  loading lazy (§4 explains why not); and it does **not** put the artifact hash
  in the run's report — that is still ADR-0025 §6, still unpaid, and still
  needing a contract change.

## Context

[ADR-0025](0025-the-executor-is-a-department-modules-by-logical-name.md) decided
that an executor is a department that serves several modules, that a step is
addressed `<module>/<step>`, and that the module is named logically. It
implemented that for the WASM bridge and left one thing open on purpose, in
§Deferred: *"whether the Python executor also moves to module + step, and what
that breaks for sequences that already exist. Today its step names live in one
flat namespace."*

That flat namespace was **not an oversight**. `Registry.add` refuses to start
when two steps share a name, and says why: the name is what a sequence
dispatches on, so it must be unique
(`executors/python/anvil_step/__init__.py`, before this ADR). The executor was
built to serve a folder — `--steps` is repeatable, it reads a path from an
environment variable meant for a service manager, and it can print its catalog
and exit — but everything in that folder landed in one namespace.

And here the starting point is the opposite of the WASM one. For the bridge, a
directory was a **new** thing, so qualifying broke nothing. For Python the
directory is the default and always has been (`./steps`), and it has always
handed out bare names. Qualifying therefore breaks what exists: the seven steps
of `executors/python/steps/instrument.py`, two of the repo's own examples
(`ejemplos/demo_ejecutores.yaml`, `ejemplos/referencia.yaml`), the reference QA
script (`docs/qa/referencia/run.sh`) and any sequence a user has already
written.

## Decision

### 1 — A step is addressed `<module>/<step>`, always

No bare names, no alias, no compatibility mode. The catalog publishes only the
qualified name, and that is what a sequence writes.

The alternative that does not break anything — publish both, while the bare
name is unambiguous — was rejected for the reason ADR-0025 already gave about
guessing: the bare name would stop working the moment somebody drops a second
module serving that step, so the meaning of a sequence would depend on the
contents of a folder. A sequence that breaks when a **different** module is
added is worse than one that never worked, because the failure arrives later
and with a unit on the bench.

**This is a breaking change to the public surface, and it is announced as one.**
Anvil is 0.x and the CHANGELOG records exactly this kind of change. It is also
caught before touching a unit: `--validate --with-executors` lists what the
executor serves, qualified, and the run stops there.

### 2 — The module is the logical name of the `.py`, derived and never declared

`instrument.py` is `instrument`. A package's `__init__.py` takes the package's
directory name, because nobody writes a sequence against a module called
`__init__`. The author of a step writes a function and nothing else, so a module
that is renamed or moved needs no edit inside it — the same rule the WASM bridge
follows for a `.wasm`, and the reason both departments speak one language.

It is derived from the **file the function is written in**, not from what
`discover` happened to import it as, which is what makes it work identically for
a step written in a test, in a package, or in a module imported by another.

### 3 — Uniqueness moves down one level, and module names become the thing that clashes

Two steps with the same name in the **same** module are still a start-up error,
with the message unchanged. Two modules each serving a `medir_voltaje` is now
ordinary, and that is what this buys.

What is new is that two files with the same stem under different `--steps`
paths now clash, and the executor **refuses to start**, naming both. Serving the
wrong module is worse than not starting: the run would measure with a module
nobody asked for and the report would not say so. The check happens **before
importing anything**, because an import runs module-level code and that code can
open a session with an instrument (ADR-0025 §4).

### 4 — Loading stays eager, unlike the WASM bridge, and that asymmetry is deliberate

The bridge loads a directory's modules on demand because it can: a `.wasm`'s
logical name is its file name, so the bridge knows what it serves without
opening anything. Python cannot do that — to know that `instrument.py` serves
`medir_simulador` you have to import it — and `Describe` is asked on every run
and returns the whole catalog, so every module would be imported anyway.
Deferring the import would buy nothing and would move an import error from
start-up to mid-run.

### 5 — The hash of each module is read at discovery, and is not in the report

`Registry.modules` maps each logical name to its file and its SHA-256, computed
by reading the file. It goes on the start-up log and into `--list`.

It is **not** in the run's report, exactly as in ADR-0025 §6 and for the same
reason: there is no field in `paso.proto` to carry it. Do not read a green
report as saying which artifact produced it.

## Discarded alternatives

**Qualified plus a bare-name alias while it is unambiguous.** The tempting one,
because it breaks nothing today. Rejected in §1: it makes a sequence's meaning
depend on what else is in the folder, and it postpones the break to the worst
possible moment.

**An explicit per-executor mode** (`--qualified`), so nothing breaks and nothing
is guessed. Rejected because the YAML would not say which mode is in force: the
same sequence would be valid or not depending on how somebody started the
executor three weeks ago, and that is not something a report can reconstruct.

**Leaving Python flat and qualifying only WASM.** It is the smallest change, and
it is what §Deferred of ADR-0025 left as a live option. Rejected because it
breaks the one thing the department metaphor is for: Anvil talks to every
department the same way. Two addressing schemes would mean the engine's users
have to know which technology serves a step in order to write its name — which
is precisely what naming a module logically was meant to hide.

## Consequences

- **Sequences that name Python steps must be rewritten.** In this repo:
  `ejemplos/demo_ejecutores.yaml`, `ejemplos/referencia.yaml` and
  `docs/qa/referencia/run.sh`. Outside it, whatever users have written.
- Verified by running on 2026-09-02: `demo_ejecutores.yaml` passes with
  `instrument/medir_simulador` and `instrument/conectar_equipo`; the reference
  QA suite is 3/3; two modules each serving `medir_voltaje` are served side by
  side and a sequence measures 4.2 from one and 24.0 from the other — which the
  executor **refused to start** with before this change.
- `Registry` gained `modules` and `add_module`; `StepSpec` gained `module` and
  `qualified`. A step function is still an ordinary function, and the SDK's
  tests still need neither gRPC nor a bench.
- The start-up line and `--list` now show qualified names. A listing that showed
  bare names would be advertising an address that does not work.
- The catalog check does the real work of catching the migration: the bare name
  of an old sequence comes back as "the executor does not serve it", listing
  what it does serve. The executor's own "did you mean `<module>/<step>`?" is
  the belt to that braces, and is only reached when a step is invoked without
  the catalog having been checked.
