# ADR-0025: The executor is a department, and a step is addressed by logical module name

- **Status:** Accepted — **decision only. Nothing here is implemented yet**, and
  today's YAML keeps working exactly as it does (see §Scope).
- **Date:** 2026-09-02
- **How it was decided:** in this repo, in a design conversation with
  management, starting from a question about the authoring experience: *"when I
  use a WASM in a sequence I expected to link the file and see the methods
  inside it, the way I pick a `.vi` out of an `.lvproj` in LabVIEW — but here
  the executor seems to be the WASM I just built"*. The reading was correct, and
  the conversation turned into where the module ends and the executor begins.
  Everything asserted about today's state is **verified by reading the code in
  this repo**, and cited with file and line; the `hola.yaml` run and the
  `--validate --with-executors` behaviour quoted in §Context were **verified by
  running** them on 2026-09-02. What is said about NI TestStand is **not
  contrasted**: it is product knowledge, not something measured here, and it is
  marked as such where it appears.
- **Relates to:** ADR-0011, ADR-0012, ADR-0013, ADR-0015, ADR-0019, ADR-0021,
  ADR-0023, ADR-0024
- **Scope:** decides the **addressing model** (module + step, by logical name),
  the **split between describing and enumerating** a catalog, and what the
  **report records** about the artifact that ran. It does **not** change
  `paso.proto`, the WIT of the component, the engine, or the embedded executor.
  It does **not** change how executors are declared in the sequence today —
  `executors:` with its `path` stays as it is until something implements this.
  It does **not** decide Anvil's bench-configuration file, nor whether a
  sequence should declare only the names of the departments it needs: both are
  recorded in §Deferred, deliberately unresolved.

## Context

Anvil already has two executors that serve steps written by the user, and they
do not follow the same model.

The **Python executor is a server with a working directory**. You point it at
one or more step paths and it discovers what is there; the path can also come
from an environment variable meant for a service manager
(`executors/python/server.py:57,61,254-263`). It has its own deployment options
(`--option key=value`, `server.py:268-277,312`), and it can print its catalog
and exit, so that *"which steps does this executor serve?"* can be answered
without starting a bench (`server.py:22,280-298,319`). A comment in the file
puts it plainly: you point the executor at your steps, it does not point at you
(`server.py:56`).

The **WASM executor is the component itself**. `anvil-exec-wasm` takes
`--wasm <path>` as a required argument (`executors/wasm/src/main.rs:13,312`),
loads exactly one component and holds it for the life of the process
(`main.rs:95,213`). One process, one module, one port. The remote case is
already contemplated — `--bind 0.0.0.0` exists for it, with a Raspberry Pi
named in the comment (`main.rs:18-21,324`) — but what you deploy remotely is
still one component per process.

So the WASM side collapses two things that the Python side keeps apart: the
thing that loads modules, and the module. That is what the LabVIEW comparison
exposed. In LabVIEW a `.vi` is both the file and the callable, so picking the
file and picking the method are one act; a `.wasm`, like an `.lvproj`, holds
several steps — `ejemplos/hola-paso` serves three — so the file is a project,
not a VI, and what is missing is the expanding.

The information for that expanding already exists. `describe` has been in the
WIT since `anvil:step@0.4.0` (`executors/wasm/wit/anvil-step.wit:139`), and a
component publishes each step's name, parameters, types, whether they are
required and the first line of its doc comment (ADR-0021, ADR-0024). What does
not exist is any way to *look at it* without having written the sequence first:
the only place the catalog is consulted is `--validate --with-executors`
(`packaging/anvil-host/src/main.rs:178`), which needs a YAML already written.
Verified by running on 2026-09-02: a sequence with `a_quienn:` instead of
`a_quien:` came back with *"step 'hola_mundo' (mis_pasos): it takes no input
called 'a_quienn' (it takes: a_quien)"*. That is a spell-checker where a
drop-down was wanted, and it is correct as far as it goes — it just does not
help you write the sequence in the first place.

*(Not contrasted: in TestStand, a C DLL gives the step's Module tab a drop-down
built from the PE export table — names only, no types, so the prototype is
declared by hand or imported from a header; a .NET assembly or a LabVIEW VI
gives names **and** types, because the artifact carries metadata. A WASM
component is in the second group, not the first: it carries more than a C DLL
does.)*

## Decision

### 1 — The executor is a department, not a module

An executor is a persistent process, possibly on another machine, that loads
**several** modules and knows how to talk to its own technology. Anvil talks to
every department in the same language; each department knows how to talk to its
own team. The Python executor is already this; the WASM one is the piece that
does not follow the model, and the unit it is given changes from a file to a
set of modules.

### 2 — A step is addressed by module **and** name

`multimetro/medir_voltaje` and `plc/medir_voltaje` are different steps and
coexist. This is what makes a department able to hold several projects without a
name collision, and it is the same shape TestStand uses (module + function,
*not contrasted*).

### 3 — The module is named **logically**: no extension, no path

The sequence says `multimetro`. It never says `multimetro.wasm`,
`multimetro.py`, or `instruments/dc/multimetro.wasm`. Translating the logical
name to a file is the department's job, exactly like loading it.

Two reasons, and the second is the one that bites in production:

- An extension leaks the department's technology into the sequence. Rewriting
  the multimeter from Python to WASM because the Python is too slow would then
  edit every sequence that used it, without a single step or parameter having
  changed.
- A relative path couples the sequence to the directory tree of another
  machine, so the department can no longer reorganise itself without breaking
  sequences — which is the opposite of the autonomy that makes it a department.

### 4 — Asking for a catalog is two operations, not one

**Describing what the sequence will use** happens once at the start of a run,
bounded to the modules that run mentions. It stays mandatory even when an editor
has already validated (§7 below), and it keeps the timing ADR-0021 §3 already
fixed: at start-up, never before each step, because finding out at step 47
leaves the unit half tested.

**Enumerating everything a department holds** is a tooling operation — a lazy,
per-module, cached expand for an editor — and it should never happen during an
execution.

They are split because enumerating is neither cheap nor side-effect free.
Importing a Python module executes module-level code, so a `multimetro.py` that
opens its VISA session on import has just touched the instrument because someone
asked a question; an `.lvlib` starts LabVIEW; a DLL runs its `DllMain`. Multiply
by the number of modules in the folder. Only a WASM component is genuinely inert
to ask (*not contrasted for LabVIEW and DLL specifics*).

### 5 — An editor's catalog cache is invalidated by hash, not by re-describing

Asking a department for the hashes of its modules is cheap and imports nothing;
describing them is expensive. A cache that says `medir_voltaje` takes `canal`
while this morning it was renamed to `canal_dc` is worse than no cache, because
the sequence is then designed against a ghost.

### 6 — The report always records what actually ran

For every module a run touched: its **logical name**, the **executor** that
served it, and the **hash of the artifact** that was loaded. There is no flag to
turn this off.

This is the mandatory counterpart of §3. The moment the sequence stops naming a
file, the report becomes the only place in the world where it is written down
which code ran that night. ADR-0019 Rule 3 — what alters the criterion is
written in the report — is not satisfied by a sequence that says `multimetro`
and a bench that decides what that means.

### 7 — The contract is checked on every run, always

The catalog check at start-up stays, and stays mandatory, even when the sequence
was validated in an editor. Between editing and executing there is time, and
into that time fit: someone editing the YAML by hand or resolving a git conflict
without opening the editor; someone recompiling a module and renaming a
parameter; and the same sequence running on another bench whose department holds
a different version of the same module. Validation at edit time is a
convenience, never a guarantee.

This covers the case that matters most in practice — the module changed and the
sequence no longer fits — without depending on two binaries being identical.

### 8 — Pinning a hash is optional, and lives outside the sequence

A pin file, passed by flag, following the pattern `--limits` already
establishes (`packaging/anvil-host/src/main.rs:113`): the thresholds can be
lifted out of the YAML into their own file so the sequence stays portable. The
sequence itself never carries a hash.

Nailing the hash inside the YAML was rejected (§Discarded alternatives). What
the pin covers is the residual case §7 cannot: a module whose contract is
unchanged but whose behaviour is not. That case is real and is the most
dangerous of all, but it is rare, and it does not justify blocking daily
operation.

### 9 — A pin mismatch is `error`, never `fail`, and it stops at start-up

It is information about the bench, not about the unit, so ADR-0019 Rule 2
settles the status. And it is checked with the rest of the start-up validation,
not mid-run, for the reason ADR-0021 §3 already gave.

### 10 — The limit of the mechanism, written down on purpose

The hash is computed and reported **by the executor itself**, so Anvil trusts
what it is told. This detects the accident — the rebuild nobody announced, the
out-of-date bench — which is what actually happens. It does not detect someone
who wants to lie to you. Artifact signing and a chain of custody would be a
different and much more expensive decision; this ADR does not promise it, and
nothing built on it should claim it does.

## Discarded alternatives

**Pinning the hash inside the sequence, mandatory.** Rejected. In production
there is not one bench but eight, and they are not recompiled at the same time.
A sequence carrying a nailed hash stops running on the bench next door, and what
happens then is not that the benches get synchronised: someone adds an
ignore-the-hash option to the start-up script and it never comes off. A bolt
everybody disables is worse than no bolt, because it also lies to the audit. On
top of that, a `.wasm` hash changes on a rebuild with nothing semantic having
changed — different compiler, an embedded absolute path, a new SDK version — and
false positives are what teach people to ignore alarms.

**Keeping the path and the extension in the step, as TestStand does with a
module.** Rejected for the two reasons in §3. TestStand can afford it because
its adapter has no working directory of its own; a department does.

**Enumerating the whole department at the start of every run**, so that the
engine always knows everything. Rejected: it is the expensive, side-effecting
operation of §4, paid on every run, to answer a question only an editor asks.

**Leaving the WASM executor as one component per process** and telling users to
declare one executor per file. It works — it is what happens today — but it
makes the sequence name a deployment unit, and it is what forces the path and
the extension back into the YAML.

## Deferred

Recorded so that a later reader knows these were **seen and left open**, not
missed:

- **Whether a sequence should declare only the names of the departments it
  needs**, with where they live and how they start up moving to a separate
  layer. It is the natural consequence of §3, and the `--executor
  name=host:port` flag (`main.rs:114`) is already a miniature of it. Deliberately
  not resolved: for now executors keep being declared in the YAML as they are
  today, and opening a configuration layer was not asked for.
- **Anvil's bench-configuration file** — the list of which departments exist in
  this installation and where to find them. It is what a sequence that named
  only departments would need, and it is also where the technology of each
  department would legitimately reappear (Anvil must know that `instrumentos`
  is served by the WASM bridge and not the Python one in order to bring it up
  locally). Whether it is read by convention or passed per run is open.
- **Who starts a department.** The working rule from the conversation: local
  ones Anvil brings up itself, as it does today; remote ones are started by hand
  or by a service manager and Anvil only connects. The tension with ADR-0011
  ("download one binary and run") has not been worked through.
- **What a department's own logs owe the report.** A department has its own logs
  and its own problems; if it is autonomous, what only it knows stays on its
  machine while Anvil's report says `error`. Reconstruction split across two
  files on two machines is a real cost and has not been addressed.
- **Whether the Python executor also moves to module + step**, and what that
  breaks for sequences that already exist. Today its step names live in one flat
  namespace.
- **Whether what gets pinned is the artifact hash, the contract hash, or both.**
  A contract hash is stable across cosmetic rebuilds and is what actually
  protects against a renamed parameter; the artifact hash is identity. §6 records
  the artifact hash; §8 does not settle which one a pin compares.

## Consequences

- The WASM bridge stops being *the* step and becomes a loader: one process, many
  modules. `--wasm` as a required single-component argument
  (`main.rs:13,312`) is what has to give.
- The report gains a section that did not exist, and it is not optional. Any
  change that makes a measurement impossible to reconstruct afterwards is a
  broken change even if every test passes.
- `describe` in the WIT does not change. What changes is who is asked and about
  what: a department, about one of its modules.
- An editor becomes possible for the first time — expand a department, expand a
  module, pick a step, see its parameters typed — with the data that already
  travels. Nothing in this ADR builds it.
- The Python executor is closer to the target than the WASM one, which is the
  opposite of how it looked from the outside.
