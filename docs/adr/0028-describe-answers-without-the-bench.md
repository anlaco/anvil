# ADR-0028: `Describe` answers without the bench

- **Status:** Accepted. **Not implemented** by this ADR.
- **Date:** 2026-09-05
- **How it was decided:** in this repo, while designing the graphical editor.
  The editor needs a catalog to draw its centre panel, and the case that broke
  the assumption was named by management: writing a sequence on a laptop, with
  no bench, no instrument powered and nothing running. Everything asserted
  about today's state is **verified by reading the code** in this session and
  cited with file and line. The claim about TestStand's type files is **not
  contrasted with primary sources** and is marked as such where it appears.
- **Relates to:** ADR-0003, ADR-0019, ADR-0020, ADR-0021, ADR-0022, ADR-0026,
  ADR-0027
- **Scope:** decides **when** an executor must be able to answer `Describe`.
  It does **not** change `paso.proto`, does **not** make `Describe` mandatory,
  does **not** raise `contract`, and does **not** say anything about `Invoke`,
  which may need the bench all it likes. It does **not** design the editor.

## Context

ADR-0021 gave the executor a way to describe itself, and the engine already
handles a refusal well. `crates/motor/src/catalogo.rs:31-40` models the two
answers as `Descripcion::Describe(Catalog)` and `Descripcion::NoDescribe(String)`,
and its header states the doctrine plainly:

> *"An executor may decline, and it shows. A third party need not implement
> `Describe`. Then those steps are reported as unchecked — never an error,
> which would shut the door on third parties, and never silence, which is the
> false green of ADR-0019."*

That covers **whether** an executor answers. It says nothing about **when** it
can, and the editor is what makes the difference matter.

To draw the parameters of `medir_voltaje` — name, type, whether it is required,
its default, its one line of doc — the editor asks `Describe`. Which means a
process has to be up and answering. And that is a different demand from the one
the engine makes: the engine asks once at start-up, on a bench that is by then
already powered, because it is about to run the sequence. The editor asks on a
laptop on a train.

Today it happens to work. Verified on 2026-09-05:

- `crates/pasos_scpi/src/lib.rs:42-43` — `medir_voltaje_scpi_en` opens the
  `TcpStream` **inside the step**, not at start-up. The process serves without
  an instrument and only reaches for one when invoked.
- `executors/wasm/src/main.rs:765-777` — the bridge answers from
  `modules.catalog()`, read off the loaded components. No device involved.
- `executors/python/server.py:256-272` — always `describes=True`, built from
  `self.registry.catalog()`.

But that is a property of the three executors that happen to exist here, not a
promise the contract makes. And there is a reason to expect it to break: the
whole point of ADR-0022's references is executors that hold things open. Its
own words for what a reference carries are *"open sockets and vendor driver
locks"*. The natural shape for an executor built on a real VISA driver is to
open the session once at start-up and reuse it — and that executor does not
start without the bench.

The failure is worse than it looks, because it is **indistinguishable**. An
executor that refuses to describe itself is a legitimate, visible state the
engine already reports as unchecked. An executor whose process will not start
is not refusing — it is absent, and from the editor's side that looks exactly
like a bench that is switched off. There is no answer to report as unchecked,
because nobody answered.

*(Not contrasted: TestStand does not have this problem because its step types
live in type files on the developer's machine rather than being asked of the
instrument. Stated from working knowledge, not from primary sources.)*

## Decision

**An executor that implements `Describe` must be able to serve it with no
hardware present.**

Concretely, and this is the whole of it:

1. **Starting the process must not require the bench.** Binding the port and
   serving `Describe` may not depend on opening an instrument, a driver session
   or a device file. What an executor does on `Invoke` is its own business and
   this ADR says nothing about it.

2. **Declining stays free.** An executor may still not implement `Describe`, or
   answer `UNIMPLEMENTED`, and the engine reports its steps as unchecked
   exactly as it does today (`crates/motor/src/catalogo.rs:36-40`). This ADR
   does not make the RPC mandatory; it constrains only those that do implement
   it.

3. **The catalog is knowledge about the code, not about the world.** What steps
   an executor serves and what parameters they take is fixed by what was
   written and loaded, not by what is plugged in. An executor that cannot name
   its own steps without asking an instrument has put the description in the
   wrong place.

4. **`contract` does not move.** Silence here cannot change a verdict — the
   rule of ADR-0020 §4c — so this raises no version. It is a promise to whoever
   writes an executor, and its enforcement is social and documentary, not on
   the wire.

5. **`lifetime` still means what it meant.** An executor started without the
   bench mints a lifetime like any other (ADR-0022 §6). Nothing here lets a
   reference minted in one process be honoured by another.

## Alternatives discarded

**Let the editor cache and never ask.** The cache is coming anyway — an editor
that only works with executors running is not the editor anyone asked for. But
a cache has to be filled the first time by someone who did talk to the
executor, and a cache that can never be refreshed without the bench is a cache
that silently rots. Caching is the answer to *"not right now"*, not to *"never
without hardware"*.

**Inspect the artifact instead of asking.** Read the `.wasm`, the `.py`, the
DLL, and derive the signature. This is what TestStand does with a VI's
connector pane, and `crates/motor/src/catalogo.rs:11-16` already argues against
it: asking works the same for WASM, Python and a box in another room, while
inspecting runs out of road the moment the artifact carries no metadata.
Rejected there, rejected here.

**A separate catalog file next to the executor.** A manifest the editor reads
without any process at all. It removes the problem and introduces a worse one:
two sources of truth for the same signature, drifting, with nothing to detect
the drift. ADR-0021 chose asking precisely so the answer comes from the thing
that will actually run.

**Say nothing and let it be discovered.** The status quo. It works until the
first executor with a real driver, and then it fails on the machine of whoever
is trying to write a sequence at home — the person least equipped to diagnose
it, and furthest from anyone who can.

## Consequences

- **A new promise to whoever writes an executor**, and it belongs in the
  executor guides — `executors/python/` and `executors/rust/` document what an
  executor owes, and this joins the two obligations ADR-0022 §7 already lists
  that the contract cannot verify.
- **The three executors in this repo comply today**, so nothing needs fixing
  now. That is exactly why it is worth writing down before one of them stops
  complying for a good local reason.
- **It is not machine-checkable.** Like ADR-0022's "never recycle a payload
  within one lifetime", this is an obligation the wire cannot enforce. An
  executor that breaks it is broken, and the symptom will be an editor that
  cannot draw a panel.
- **`--validate --with-executors` gets easier to justify off the bench**
  (`crates/motor/src/bin/anvil.rs:113-114`): checking a sequence's step names
  and parameters against the catalog stops being something that only works
  where the instruments are.
- **The editor may still find nobody home**, and must say so rather than
  assume — regla 2 of ADR-0019. This ADR narrows why that happens; it does not
  promise it never will.
