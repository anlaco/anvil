# ADR-0041: There is no embedded executor

- **Status:** Accepted. **Not implemented** by this ADR.
- **Date:** 2026-09-15
- **How it was decided:** in this repo, in a conversation with the person who
  develops here, the same day as ADR-0040 and continuing from it. Their
  decisions, in their words: remove the executor inside `anvil`; delete its
  demonstration steps outright — "*la idea es que la instalación te dé una
  especie de prueba con todo listo*" —, built as a WASM component; `executor` is
  required; delete `process_models/sequential.yaml`; delete the SCPI step and
  leave the projects outside this repo that use it to be told, not edited from
  here; and no backwards compatibility before 1.0. Everything asserted here
  about this repo was verified in that session by reading the code, cited with
  file and line; the engine connecting to the embedded executor for a sequence
  that calls no executor was also **run**.
- **Relates to:** ADR-0011, ADR-0012, ADR-0013, ADR-0014, ADR-0016, ADR-0017,
  ADR-0019, ADR-0021, ADR-0025, ADR-0027, ADR-0040
- **Scope:** decides that the `anvil` binary **carries no step executor**: the
  guest `crates/ejecutor_pasos` and the step crates it composes,
  `crates/pasos_demo` and `crates/pasos_scpi`, are removed; that every step
  which calls an executor **names it**; that every entry under `executors:`
  **names its `type`**; that `--port` goes; that the ready-to-run demonstration
  ships as a **WASM component served by `anvil-exec-wasm`**; and that
  `process_models/sequential.yaml` is removed. It does **not** remove the
  `--process-model` mechanism (ADR-0016), which stays and is tested with a
  fixture of its own. It does **not** change `paso.proto`, the WIT, or any
  executor that exists outside the binary. It does **not** replace the SCPI
  step: nothing in this repo reaches an instrument by SCPI after it. It does
  **not** edit the website, Crucible or Anvil-Test, which use
  `ejemplos/scpi.yaml`. It does **not** decide what a default process model
  looks like once there is operator input.

## Context

### 1. What the embedded executor is

`packaging/anvil-host/src/main.rs:60` embeds `ejecutor_pasos.wasm` next to the
engine guest, and `:564-611` runs it on a thread of its own, on an ephemeral
port the engine is handed as `--port`. It dispatches to two crates
(`crates/ejecutor_pasos/src/main.rs:31-36`): `pasos_scpi`, one step that opens
a TCP socket to an instrument (ADR-0017), and `pasos_demo`, the simulated steps
every example is written against — `conectar`, `medir_voltaje`,
`verificar_led`, `abrir_rele`, `desconectar`, and the two plug-ins of
`process_models/sequential.yaml`, `identificar_uut` and `notificar_resultado`
(`crates/pasos_demo/src/lib.rs:28-119`).

It has been the default twice over. A step that names no `executor` goes to it
(`crates/cargador/src/lib.rs:1446-1448`, ADR-0013 §2), and so does an entry in
`executors:` that names no `type` (`crates/cargador/src/lib.rs:91,105-107`).

### 2. What that default costs

**The engine connects to it whether or not anything uses it.**
`Motor::desde_programa_en` calls `Motor::conecta` unconditionally
(`crates/motor/src/lib.rs:147-148`). A sequence with a single `statement`, which
calls no executor, starts it and connects — run with `anvil 0.6.3` built from
this tree:

```
$ anvil packaging/anvil-host/tests/fixtures/paso.yaml
ejecutor de pasos escuchando en 42141
...
conectado a los ejecutores de pasos (embebido en 127.0.0.1:42141)
catálogo pedido
=== exit_paso: pass ===
```

So when a *declared* executor cannot be reached, the error names the one that
answered (#78).

**A step that forgets its `executor` is not an error.** It is sent to a demo
executor that may well serve a step by that name — `medir_voltaje` exists there
and measures 4.2 — and the sequence runs. On a bench that is a measurement from
a simulation reported as one from the instrument: Rule 1 of ADR-0019.

**It is the wrong place for a demonstration.** Its steps are compiled into the
engine's own binary, so a demo cannot be changed without a release, and it
teaches the one way of writing steps no user will ever use — inside `anvil`.

**ADR-0040 already separates what a step calls from how it is judged**, and
makes `type` explicit rather than inferred. A default executor is the one
inferred part left in *what it calls*.

### 3. What already exists to replace it

`anvil-exec-wasm` already ships next to `anvil` with an assembled department
(`packaging/package.sh:42-54`), serves every `*.wasm` beside its own binary
(ADR-0025, ADR-0027) and needs nothing installed. The Rust step SDK
(`executors/rust/anvil-step`) expresses everything `pasos_demo` does — pass,
fail, error, a measurement, named outputs, optional typed inputs, the attempt
number — except object references (the WIT has no reference type; ADR-0022),
which `ejemplos/referencia.yaml` already takes from the Python executor.

What it cannot host is the SCPI step: a component runs with **no network**
(`executors/wasm/src/main.rs:291-292`).

## Decision

**The `anvil` binary carries the engine and nothing else. Steps are served by
executors the sequence declares.**

### 1 — No executor inside `anvil`

`crates/ejecutor_pasos`, `crates/pasos_demo` and `crates/pasos_scpi` are
removed, with `crates/motor/src/bin/basica_datos.rs`, which is built on them.
The host embeds one guest, the engine. The engine opens a connection only to
the executors the program declares and uses.

### 2 — A step that calls an executor names it

`executor` is **required** on every step that calls an executor — today a
`grpc` step, after ADR-0040 every step with a `module`. Its absence is a load
error. It is checked **per program**, where the executor table is known, and
not per file: a subsequence file declares no executors and inherits its root's.

### 3 — An executor names its `type`

Every entry under `executors:` names its `type` (`wasm` or `grpc`). There is no
`embedded` type and no reserved executor name.

### 4 — `--port` goes

It existed only to place the embedded executor (`crates/motor/src/bin/anvil.rs:87`,
`packaging/anvil-host/src/main.rs:62-68`). It is removed from both the host and
the engine's command line.

### 5 — The demonstration is a WASM component, and it ships

The simulated steps become a component, `demo`, in the example department
(`ejemplos/departamento/`), written with the Rust step SDK and served by
`anvil-exec-wasm`. The example sequences declare it
(`type: wasm, path: departamento/dist/anvil-exec-wasm`) and call
`demo/<step>`. It ships in the package as the department already does, so a
freshly unpacked `anvil` runs its examples with nothing else installed. CI, the
regression suite and the host tests use the same component.

A `path:` naming an executor binary is resolved **with its platform suffix**:
on Windows `departamento/dist/anvil-exec-wasm` finds `anvil-exec-wasm.exe`, so
one sequence runs on both.

### 6 — `process_models/sequential.yaml` is removed

Its two plug-ins stand in for an operator prompt that does not exist: one
returns a made-up serial number, the other notifies nobody. Keeping them means
keeping an executor to serve them. The `--process-model` mechanism stays; a
process model worth shipping waits for operator input.

### 7 — The SCPI step is removed, and not replaced here

No step in this repo talks SCPI after this. `ejemplos/scpi.yaml` goes. The
website, Crucible's guide and Anvil-Test run it, and are told; they can rebuild
it on the Python or C# SDK, which have the network a component does not.

## Alternatives rejected

- **Keep the embedded executor, only stop defaulting to it.** Fixes the silent
  simulation but keeps demonstration steps in the engine's release cycle, the
  unconditional connection, and a second way of serving steps nobody else uses.
- **Default to the only declared executor.** Convenient until a second executor
  is added and every step that relied on it moves silently. Rejected for the
  reason ADR-0040 rejected an inferred `type`.
- **Build the demo on the Python or C# executor.** Easier to read, but it would
  need Python and `grpcio`, or .NET, installed: not ready on unpacking, and the
  release check in a clean container (`.github/workflows/verifica-release.yml`)
  runs without a toolchain.
- **Keep `sequential.yaml` with its plug-ins as engine-side placeholders.**
  Rejected by the person deciding: a placeholder that does nothing is not worth
  maintaining, and the real one needs operator input.
- **Move the SCPI step into the demo component.** Not possible: a component has
  no network.

## Consequences

**a. It is a breaking change to the sequence format and the command line**, in
the same minor release as ADR-0040 and before 1.0, when this repo keeps no
backwards compatibility. Sequences with steps that name no executor stop
loading, with an error that says to declare one.

**b. ADR-0011, ADR-0012 §2, ADR-0013 §2, ADR-0014 §2, ADR-0016 (its canonical
file), ADR-0017, ADR-0021 §6 and ADR-0040 §1 are overruled on the points above,
and are annotated there.**

**c. Every example, fixture, regression case, CI smoke and book listing that uses
a demo step is rewritten against the component**, and the book's recorded
sessions, which contain the embedded executor's own output, are recorded again.
Cases that only exercised the embedded executor's port handling are deleted.

**d. Startup changes shape, not cost from nothing.** The embedded guest was
itself compiled at every start; now a sequence that uses the demo spawns
`anvil-exec-wasm` once per run. The editor's wait for the bridge covers that
start and is measured again.

**e. Projects outside this repo break**: the website and Crucible's guide
(`ejemplos/scpi.yaml`), and Anvil-Test's copies of the examples. Recorded for
whoever coordinates them; not edited from here.

**f. #78 loses its cause.** With no embedded connection, a declared executor that
cannot be reached is the only one that can be named. Whether its message is
then right is checked, not assumed.
