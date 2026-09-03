# ADR-0027: A sequence names the executor, and the executor finds its own modules

- **Status:** Accepted, and **implemented** the same day.
- **Date:** 2026-09-02
- **How it was decided:** in this repo, by management, on seeing the result of
  ADR-0025 written out in a guide. The complaint was concrete and correct:
  *"el ejecutor no debe apuntar al .wasm sino al ejecutor genérico, y lo primero
  que veo en la ruta de ejecutores es `hola-wasm/target`"* — a Cargo build path
  had ended up inside a sequence. The shape was given the same way: *"la ruta al
  ejecutor puede ser una ruta local o una conexión… después le das la ruta
  relativa respecto al ejecutor en el paso"*, and, asked about the two open
  details, *"apunta al binario el ejecutor"* and *"por ahora y temporal son
  rutas relativas a donde esté el binario; el ejecutor debe saber
  encontrarlos"*. Everything asserted about today's state is **verified by
  reading the code** and cited with file and line; the runs in §Consequences
  were **verified by running** them on 2026-09-02.
- **Relates to:** ADR-0011, ADR-0013, ADR-0015, ADR-0023 (amended), ADR-0025
  (amended), ADR-0026
- **Scope:** decides what `path:` means on a `type: wasm` executor and who
  resolves where the modules are. It does **not** change `paso.proto`, the WIT,
  the engine, the embedded executor or the Python executor; it does **not**
  introduce a bench-configuration file (still deferred, see ADR-0025); and it
  does **not** decide how a module directory is laid out beyond "next to the
  binary", which management marked as **temporary**.

## Context

ADR-0025 made the WASM executor serve a directory of modules and named them
logically, which took the extension and the module path out of the step's name.
But it left the *executor's* declaration pointing at the modules:

```yaml
executors:
  - name: mis_pasos
    type: wasm
    path: hola-wasm/target/wasm32-wasip2/debug   # ← the modules
```

Written out in a guide, what that produces is a **Cargo build path inside a
sequence**: `target/wasm32-wasip2/debug` is a detail of how the modules were
built, on one machine, and it has no business surviving in a document that
outlives the project. Worse, it is the thing a sequence should be most
indifferent to — ADR-0025 §3 argued exactly this about the step's name, and
then left the same leak one line above.

There was a second, quieter constraint. `anvil` could not be told *where the
executor is*: it looked `anvil-exec-wasm` up beside its own binary and spawned
that one, always (ADR-0023). So a department could not be a thing you copy: the
modules lived wherever the YAML said, and the executor serving them was always
Anvil's own.

## Decision

### 1 — `path:` on a `type: wasm` executor is **the executor's own binary**

Not a `.wasm`, not the folder of modules:

```yaml
executors:
  - name: instrumentos
    type: wasm
    path: departamento/dist/anvil-exec-wasm    # where the executor is
  - name: banco_remoto
    type: grpc
    host: 192.168.1.40
    port: 9101                                  # or a connection
```

An executor is a thing that exists on its own, with or without any sequence.
What a sequence says is **which one it is addressing**: a local path to its
binary, or a connection to a running one. Those are the two, and they were
already the two — `type: wasm` and `type: grpc` — except that the first one was
naming the wrong object.

`anvil` spawns exactly that file, with `--port <ephemeral>` and nothing else
(`packaging/anvil-host/src/main.rs`, `instanciar_wasm`).

### 2 — Where the modules are is the executor's business, not the sequence's

Told nothing, `anvil-exec-wasm` serves the directory **its own binary is in**
(`executors/wasm/src/main.rs`, `own_directory`). Resolved from the executable
and not from the working directory, because it is spawned by `anvil`, which has
a cwd of its own, and a department has to mean the same thing wherever it is
launched from.

So a department is a **copyable folder**: the executor's binary with its
`.wasm` beside it. Copy it to a Raspberry Pi, start it there, and the same
sequence reaches it by changing `type: wasm, path: …` for `type: grpc,
host: …` — nothing else moves.

`--modules <dir>` still exists for pointing it elsewhere by hand, and `--wasm
<file>` for running a single component. **Anvil uses neither**, so a step
served through Anvil is always qualified `<module>/<step>`.

> **This is temporary and management said so.** "Modules sit next to the
> binary" is the rule for now, not a considered layout. A department that grows
> subdirectories, or that wants its modules somewhere else entirely, is a later
> decision — and `--modules` is already the seam where it will be made.

### 3 — This amends ADR-0023, and the amendment is the point

ADR-0023 decided the bridge ships as a file next to `anvil` and is looked up
there. **Shipping** next to `anvil` is untouched: that is still how it arrives,
and `make release` still leaves the pair together. What is gone is the
**lookup**: `anvil` no longer searches beside itself, because the sequence names
the binary. The function that did it is deleted, and the test that covered it
(`packaging/anvil-host/tests/bridge_lookup.rs`) is rewritten as
`executor_binary.rs`.

That is a real loss and it is worth naming: before, there was exactly one bridge
binary on a machine and it was the one Anvil shipped with. Now there can be
several, of different versions, and Anvil runs whichever one the YAML points at.
The contract echo (ADR-0020 §4b) is what still catches a stale one, and it
catches it before a step runs.

### 4 — A sequence can now cause Anvil to execute an arbitrary binary

This follows from §1 and cannot be avoided while a sequence names the executor:
`anvil una-secuencia.yaml` will spawn whatever `path:` names. A YAML stops
being pure data and becomes, in this one field, a thing that runs code.

It is the same bargain TestStand makes with a DLL, and the same one already made
by `type: grpc` pointing at a host that answers with whatever it likes. But it
is new for `type: wasm`, where the executed binary used to be Anvil's own, so it
is written here rather than left implicit: **treat a sequence from an untrusted
source as you would a script from one.**

### 5 — Pointing `path:` at a `.wasm` is diagnosed, not executed

The mistake anyone coming from before this ADR will make. A `.wasm` is a file,
so it passes every existence check, and `exec` fails with "Exec format error" —
which sends the reader to look at their toolchain instead of at the line of YAML
that is wrong. The host checks the extension and says what a `path` is now
(`instanciar_wasm`), with a test that was seen to fail without it.

## Discarded alternatives

**Leave `path:` pointing at the module directory and just document a tidier
folder.** The smallest change, and it does not fix anything: the sequence still
carries a path into someone else's filesystem, and the executor still cannot be
a copyable thing.

**A bench-configuration file** mapping department names to technology and
location, with the sequence naming only departments. It is the right end state,
it is what ADR-0025 §Deferred describes, and it was explicitly deferred again
here: management asked for the simple version — *"es sencillo un ejecutor"* —
and this decision is a step towards that file, not away from it.

**`path:` pointing at the department's folder**, with Anvil finding the binary
inside. Rejected by management in favour of the binary, and it is the more
honest of the two: the folder convention would put Anvil in the business of
knowing how a department is laid out inside, which is exactly what §2 gives to
the executor.

## Consequences

- **Every sequence with a `type: wasm` executor must be rewritten**, and the
  repo's two were: `ejemplos/demo_wasm.yaml` and
  `ejemplos/demo_departamento.yaml` now point at
  `departamento/dist/anvil-exec-wasm`. `demo_wasm.yaml`'s steps are qualified
  too (`hola_paso/medir_voltaje`), because Anvil never spawns the bridge with
  `--wasm` any more and therefore never serves bare names.
- **A department has to be assembled**, and `make build`/`make release` do it
  (`dept` target): the bridge binary plus every example module copied into
  `ejemplos/departamento/dist/`. Verified by running on 2026-09-02: the two
  sequences pass, and `dist/anvil-exec-wasm --list` shows the three modules with
  their hashes, having been told nothing.
- The `--wasm` single-file mode of ADR-0025 §D2 survives only as a
  by-hand facility. The "a file means bare step names" half of that decision is
  therefore unreachable from Anvil, and no sequence can use it.
- `ruta_puente` is gone from the host, and with it the only place that knew
  where a bridge "should" be.
