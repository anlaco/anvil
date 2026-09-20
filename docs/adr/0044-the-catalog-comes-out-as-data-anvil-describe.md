# ADR-0044: The catalog comes out as data — `anvil describe`

- **Status:** Accepted. **Not implemented** by this ADR.
- **Date:** 2026-09-20
- **How it was decided:** in this repo, with the person who develops here, from
  a question about the editor: TestStand has five Module panels, one per
  adapter, and *"lo que tiene de guay nuestra arquitectura es que el ejecutor
  nos permite que todo sea igual"*. The flow to reproduce was named precisely —
  LabVIEW's *Project Path* and then *pick a VI that belongs to that project* —
  and the question was how to get one panel to serve every language. Four
  questions were put and decided: **where the catalog comes from** (this ADR),
  **how executors are discovered** (only what the sequence declares, for now),
  what *WASM by default* will mean (open), and what a LabVIEW executor is
  (§Context 4). Everything about this repo was verified by reading the code and
  is cited with file and line.
- **Relates to:** ADR-0001, ADR-0019, ADR-0020, ADR-0021, ADR-0022, ADR-0024,
  ADR-0025, ADR-0027, ADR-0028, ADR-0041, ADR-0043,
  [ui-vs-headless.md](../diseno/ui-vs-headless.md),
  [contrato-grpc.md](../contrato-grpc.md)
- **Scope:** decides that a program **outside the engine** can obtain an
  executor's catalog **as data**, through a new subcommand `anvil describe`,
  and fixes the shape of what it prints. It does **not** change `paso.proto`,
  the WIT, `Describe` itself or `contract`; it does **not** make `Describe`
  mandatory; it does **not** execute a step; it does **not** decide how the
  editor draws anything; and it does **not** build an inventory of the
  executors installed on a machine — that stays where ADR-0025 §Deferred left
  it.

## Context

### 1. Everything needed is already on the wire, and nothing can read it

`ParameterSpec` carries `name`, `type`, `required`, `default` and `doc`;
`OutputSpec` carries `name`, `type` and `doc`; `StepSpec` carries a step's
inputs, outputs and a line of documentation
(`crates/modelo/src/proto.rs:211-312`). The three SDKs derive all of it from
the real signature of the function — `#[step]` in Rust
(`executors/rust/anvil-step-macros/src/lib.rs:135-142`), `inspect.signature` in
Python (`executors/python/anvil_step/__init__.py:641-669`), a source generator
in C# (`executors/csharp/src/Anvil.Step.Generator/`) — so the catalog cannot
drift from what will actually run. ADR-0028 already obliges an executor to
serve `Describe` **with no hardware present**.

And one place reads it: `comprueba_firmas` in
`crates/motor/src/bin/anvil.rs:437`, under `--validate --with-executors`. It
compares the catalog against a sequence someone already wrote, prints findings
and counts to stderr, and throws the catalog away.

### 2. `--list` is a door that opens onto text

All three executors have one — `executors/wasm/src/main.rs:881-943`,
`executors/python/server.py:301-330`,
`executors/csharp/src/Anvil.Step/Server/StepHost.cs:122-150` — and the WASM one
says in its own comment what it is for: *the* enumerate *operation of ADR-0025
§4 — what an editor needs*. But it prints to stdout for a person, there is no
machine format, and nothing in this repository consumes any of them.

ADR-0025 §4 separated the two operations deliberately: **describe what the
sequence is going to use** (once at start-up, bounded, obligatory) and
**enumerate everything a department holds** (*"a tooling operation… for an
editor"*, never during a run). The first exists in code. The second exists as
three `--list` flags and a paragraph.

### 3. The editor cannot get there from where it stands

`editor/src/app.mjs:1678-1687` states the gap in the code that has the hole:

> *"Anvil's equivalent exists already on the wire and is not plugged in here
> yet… Asking for it needs the bridge, so the shape is here and the content
> says what is missing rather than looking empty."*

The bridge is the wrong instrument. It is started per sequence with
`anvil <seq> --bridge`, it needs the bench up, and the case ADR-0028 was
written for is *"writing a sequence on a laptop, with no bench, no instrument
powered and nothing running"* — **the editor asks on a train**. A catalog that
only arrives when a bench is running is not the catalog the editor needs.

### 4. What is actually being built, and why one panel is enough

The executors planned are .NET, Python and LabVIEW, over a WASM default. The
LabVIEW one is a **VI Server** department: it starts LabVIEW, opens a project,
enumerates the public VIs and reads their terminals.

That is the shape that makes this decision worth making. TestStand needs a
LabVIEW panel **inside the test executive**, because the executive is what
inspects the connector pane. Anvil's LabVIEW executor inspects too — but it
does so **inside the department**, which is allowed to know LabVIEW, and it
answers the same `Describe` as a `.wasm`. The engine never learns what a VI is.
One panel serves every language because the executor did the normalising.

## Decision

### 1. `anvil describe <sequence>` prints the catalog as JSON

A subcommand, not a flag, because it is a different mode rather than a modifier
of a run: it loads, connects, asks, prints and exits, and it produces a
document on stdout rather than a report.

It is **scoped to a sequence** — the sequence names the executors
(`executors:`), the loader resolves their paths inside its sandbox, and the
host starts the `type: wasm` ones on ephemeral ports. That is the whole of the
machinery this needs, and all of it already exists. Describing an executor that
no sequence declares is a different feature and it waits for the inventory of
ADR-0025 §Deferred.

### 2. It does not run a step

Like `--validate --with-executors`: *"`Describe` asks, it does not measure"*
(`crates/motor/src/bin/anvil.rs:300-303`). Same promise, same reason, and the
host's gate says so in the same words
(`packaging/anvil-host/src/main.rs:113-117`).

### 3. An executor that declines appears in the output, saying so

Every declared executor gets an entry. One that does not describe itself has
`describes: false` and the reason the engine already models — no connection,
`UNIMPLEMENTED`, an unreadable catalog, never asked
(`crates/motor/src/catalogo.rs:279-297`). It is **never omitted**: an absent
key would read as "this executor has no steps", which is exactly the false
green of ADR-0019 Rule 2 and exactly the distinction ADR-0028 was written to
protect.

### 4. Nothing about the contract moves

No new message, no new field, no `contract` bump. The subcommand is a second
reader of what `Describe` already answers. What does get a version of its own
is **the JSON**, with a `describe_version`, because it becomes a public surface
the way the event stream did (ADR-0029 §5): adding keys is compatible, changing
what one means is not.

### 5. Types cross as their names, not their numbers

`ValueType` goes out as `number`, `text`, `boolean`, `reference` or
`unspecified` — the vocabulary the YAML and the report already use — rather
than as the enum's integers. A reader of this JSON should never have to know
the wire encoding, and `unspecified` has to stay legible as *unchecked* rather
than looking like a missing field.

## Alternatives discarded

**A `--catalog` flag on the existing path.** Cheaper: no change to how the
positional is parsed. But it reads as a modifier of *"run this"* when it is a
different mode, `--help` would have to explain that one flag suppresses the
run, and the flag set is already at thirteen — past the point where
`ui-vs-headless.md:94-97` says to stop and write this down.

**Teach the editor to speak gRPC over the bridge.** The bridge relays raw TCP,
so it is possible. It would mean HTTP/2 framing, protobuf and the `Describe`
route reimplemented in JavaScript — **a second implementation of the contract
outside the engine**, which is the thing ADR-0031 and AP-04 exist to prevent.
It would also still need a bench.

**Parse `--list`.** The output exists and is aimed at exactly this reader. But
it is formatted for a person, it differs between the three executors, and
parsing it would make its layout a compatibility surface by accident.

**A catalog file beside the executor.** Rejected by ADR-0028 and not reopened:
*"two sources of truth for the same signature, drifting, with nothing to detect
the drift"*.

## Consequences

a. **The editor's Module panel becomes possible**, and with it the flow that
   prompted this: choose the executor, then choose a module from what it
   actually serves, then see the parameters it actually declares. ADR-0025
   predicted exactly this and built none of it: *"An editor becomes possible for
   the first time… with the data that already travels. Nothing in this ADR
   builds it."*

b. **The command line gains it too**, which is the point of doing it here
   rather than in the editor (AP-13). `anvil describe seq.yseq | jq` answers
   *"what does this bench actually serve?"* on a machine with no editor, and it
   is checkable in CI.

c. **Caching is still not solved.** ADR-0028 says the cache is coming anyway,
   and ADR-0025 §5 says it must be invalidated **by hash, not by re-describing**
   — but no hash crosses `Describe` today; the executors compute one and only
   `--list` shows it (ADR-0025 §6 is marked not implemented for the same
   reason). Until that exists, this is asked on demand. Saying so here is the
   point: a cache invalidated by anything else silently rots.

d. **`describe` becomes a reserved first argument.** A sequence file called
   `describe` on the command line would now be read as the subcommand. It needs
   an explicit error, not a surprise — and it is the first time this CLI has a
   word that is not a path or a flag.

e. **It does not make the editor work in a browser.** Running it needs a
   process, and a page has none. The desktop shell gets the catalog; a plain
   browser keeps saying what it cannot know, which is true of what the page can
   see — the same line `neighbours.mjs` already holds.

f. **Not decided here:** the inventory of executors installed on a machine
   (ADR-0025 §Deferred), the hash that would let a cache be invalidated, and
   whether the catalog should be checked on every run as ADR-0025 §7 says and
   the code does not.
