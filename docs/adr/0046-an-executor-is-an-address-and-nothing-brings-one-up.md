# ADR-0046: An executor is an address, and nothing brings one up for you

- **Status:** Accepted. **Not implemented** by this ADR.
- **Date:** 2026-09-21
- **How it was decided:** in this repo, in a long conversation with the person
  who develops here, starting from a complaint about the editor: *"ahora parece
  que cada proyecto tiene que llevar su ejecutor y eso no me gusta"*. Three
  designs were put up and two were withdrawn — a station configuration file
  read by the engine, and a project file beside the sequence — both by their
  own argument, which is written down in §Alternatives because the reasoning is
  the valuable part. What settled it was theirs: *"en un yseq de producción
  pondrá que hay un ejecutor en esa ip, nada más. No sabrá qué tendrá y no le
  hace falta."* Everything asserted about this repo was verified by reading the
  code in that session and is cited with file and line.
- **Relates to:** ADR-0002, ADR-0003, ADR-0011, ADR-0012, ADR-0013, ADR-0014,
  ADR-0019, ADR-0023, ADR-0025, ADR-0027, ADR-0031, ADR-0038, ADR-0041,
  ADR-0044
- **Amends:** **ADR-0011** (the single binary no longer hosts step executors,
  only the engine guest), **ADR-0013** and **ADR-0014** (the host-side `.wasm`
  loader and its synthetic `--executor` overrides go), **ADR-0023** (the bridge
  stops shipping as a file next to `anvil`), **ADR-0027 §1–§2** (a sequence
  stops naming an executor's binary at all). What each one keeps is in
  §Consequences.
- **Scope:** decides that **every executor is an address**, that a sequence
  therefore declares one kind and not two, that an optional **`dev:`** section
  says how to bring one up on a development machine, and that **nothing starts
  an executor on your behalf during a run**. It does **not** change
  `paso.proto`, the WIT, `Describe` or the step contract; it does **not**
  design the install folder's manifest beyond what a launcher needs; and it
  does **not** implement anything.

## Context

### 1. The engine has never seen a `wasm` executor

`crates/motor/src/lib.rs:166` says it outright: `Wasm` → error, *"el motor
nunca lo ejecuta"*. There is even a variant for it being handed one,
`Error::EjecutorWasmSinHost`, describing a state that cannot legitimately
occur.

What happens instead is that the **host** re-parses the sequence by itself
(`packaging/anvil-host/src/main.rs:435-446`), spawns each `type: wasm`
executor on an ephemeral port, and composes a synthetic
`--executor name=127.0.0.1:port` for the engine — *"which already turns `wasm`
into `grpc` when applying it"*, as the comment says.

So `type: wasm` is not a kind of executor. **It is a launch instruction
wearing a declaration's clothes**, and the engine's model already has one kind:
an address.

### 2. Which makes every sequence carry a deployment detail

`path: departamento/dist/anvil-exec-wasm` is a copy of *our* binary, inside
*your* project, in a file that goes to git. Nine of the twelve examples in this
repo have one. Move the sequence and it breaks; copy it to another bench and it
breaks; and two sequences on one bench each carry their own.

ADR-0027 §4 already named the deeper version of this: *"una secuencia puede
hacer que Anvil ejecute un binario arbitrario — un YAML deja de ser datos puros
en ese campo"*.

### 3. And it teaches a model that does not exist

`./anvil ejemplos/subsecuencia.yseq` works in one command because the host
quietly starts a server. Every real bench has that server started at boot, by
a service manager, possibly on another machine — ADR-0012's LID is a Windows 7
VM, ADR-0038's C# host is the user's own process, and ADR-0025 §Deferred
already wrote the rule: *"remote ones are started by hand or by a service
manager and Anvil only connects"*.

The one-command demo is the only place in Anvil where an executor appears by
itself. Whoever learns there learns something they will have to unlearn.

## Decision

### 1. One kind of executor: an address

```yaml
executors:
  - name: banco
    type: grpc
    host: 192.168.1.50
    port: 9101
```

That is the whole of it, and it is what a production sequence carries: *there
is an executor there*. It does not say what technology serves it, where its
code lives or how it was started, **because it does not need to and must not
depend on it**. Rewriting a department from Python to WASM changes no sequence,
which is what ADR-0025 §3 wanted and what `type: wasm` quietly took back.

`type: wasm` is **removed**. A sequence written before this does not load, and
the loader says what to write instead — the same courtesy ADR-0040 §10 gave
`type: grpc`.

### 2. `dev:` says how to bring one up, and the engine ignores it

```yaml
dev:
  banco:
    runtime: python      # a logical name, resolved in the install folder
    code: ./pasos        # your steps, relative to this file
```

Optional, and absent from a production sequence. Two rules make it safe to
have in git, and both matter:

- **`runtime` is a logical name, never a path.** `python` means the same thing
  on four machines; where it is installed is each machine's business. A path
  would be the one thing in the file that differs per developer, in a file
  everyone commits — a merge conflict generator, and you cannot gitignore half
  a file.
- **`code` is relative to the sequence**, like `path:` and a subsequence by
  path already are.

**The engine never reads it.** The loader parses it with
`deny_unknown_fields` so a typo is still caught — the strictness that produced
DIAG-5's *"¿querías 'main'?"* is not worth a hole — and then nothing uses the
result. `comment` on a step is the precedent for data the engine carries and
never looks at.

### 3. Nothing brings an executor up during a run

The host stops spawning anything. `instanciar_wasm`, `EjecutorWasm`, the
dedup-by-path, the preload and the synthetic overrides all go, and with them
the only code path in Anvil that starts a process on the strength of a line in
a YAML file.

**To try the examples you start the bench first.** Two commands where there was
one, and the second one is the truth:

```sh
anvil-exec-wasm --modules ejemplos/departamento/dist --port 9101 &
anvil ejemplos/subsecuencia.yseq
```

`--executor name=host:port` (RF-36.3) is what re-points a sequence from a
developer's loopback to a factory address without editing the file. It has
existed since M5-ext.1 for exactly this and now has its use.

### 4. The bridge is installed, not carried

`anvil-exec-wasm` moves out of "a file next to `anvil`" (ADR-0023) and into the
install folder with every other executor:

```
~/.anvil/executors/
  wasm/     anvil-exec-wasm
  python/   anvil-exec-python
```

Each folder carries a small manifest: the logical name, and **which flag takes
the code** — `--modules` for the WASM bridge, `--steps` for Python. That one
indirection is what lets a front end launch any runtime without knowing one
from another, and it is the only thing the install folder has to standardise.

The download therefore becomes **the engine, plus a folder of executors to
install**, rather than a binary with a bridge beside it.

### 5. Who reads `dev:`

Only tooling. The Sequence Editor starts what it describes so that a step's
module list and its parameters can be seen while authoring — which is the
`Describe` of ADR-0044 with something to ask. Nothing on the run path reads it,
so a sequence that reaches a bench behaves identically whether the block is
there or not.

## Alternatives discarded

**A station configuration file read by the engine**, with sequences naming only
`banco`. Proposed here and withdrawn on the argument against it: a name
resolved per machine means the same sequence runs different code on two
benches, and both reports look identical. It is LabVIEW's search path, and the
failure is silent. ADR-0025 §6 — the report recording which artifact actually
ran — is **not implemented**, so there would be nothing to reconstruct it from.

**A project file beside the sequence.** Better, because it travels in the same
repo and is versioned with the sequences. Withdrawn because it breaks the
property that makes the rest simple: `anvil secuencia.yseq` must be a complete
command, and a sequence that cannot run without a second file is not one.

**Machine paths in a `dev:` section.** The first shape proposed for it. It is
more machine-specific than what it replaces, and it puts per-developer paths in
a versioned file. The logical `runtime` name exists to avoid exactly this.

**Keeping `type: wasm` for the examples only.** A supported shape that exists
so a demo reads well is a shape somebody ships with, and then it is not just
for the demo.

## Consequences

a. **Nine of the twelve examples stop loading**, and so does every sequence
   anyone has written against a WASM department. This is a breaking change to
   the sequence format and it moves the minor. The loader must name what it
   found and print the two lines that replace it.

b. **The quickstart grows a command**, and the README, the book's chapter 2 and
   the published quickstart all say the old thing. `vision.md` claims
   *"deployment portable"* against TestStand's installer pain: two commands and
   no installer still earns that, but the sentence is a product argument and
   deserves rewriting deliberately rather than in passing.

c. **CI and the QA regression must bring a bench up and take it down** around
   17 cases plus the smokes, on Linux and on Windows. Windows is where this
   will be awkward — starting and reaping a background server cleanly in
   PowerShell is the only part of this with no obvious shape.

d. **A lot of code disappears**, and that is the argument's second half: the
   host's spawn path, the synthetic overrides, `EjecutorWasmSinHost`, the
   loader's two-way branch with its per-type required and forbidden fields, the
   absolute-path diagnosis of DEF-4, and the editor mounting an executor binary
   it only ever checked the existence of (`editor/src/neighbours.mjs:92`). None
   of it was wrong; all of it existed to maintain the pretence that a sequence
   knows how its executors start.

e. **ADR-0011 keeps its thesis and loses a clause.** *"One binary, hosting
   wasmtime and the engine guest in a sandbox"* is unchanged, and so is *"no
   installer, no Rust, no cargo"*. What goes is the binary starting step
   executors for you — which ADR-0041 had already halved by removing the
   embedded one.

f. **ADR-0027 keeps §3 and §4 and loses §1 and §2.** That the bridge is found
   rather than looked up, and that a YAML must not name an arbitrary binary,
   both survive — the second one more completely than before, since now it
   cannot.

g. **The install folder needs a search rule and a failure.** A `runtime` that
   is not installed must fail naming what was looked for and where it looked.
   It is cheap now and it is the exact point where this becomes a search-path
   problem if left silent (ADR-0019, Rule 2).

h. **Not decided here:** the manifest's full shape, whether a machine-level
   configuration file may add search locations, how a runtime is versioned
   against the contract, and whether `dev:` should be able to name a runtime
   the install folder does not have, for a bench that supplies its own.
