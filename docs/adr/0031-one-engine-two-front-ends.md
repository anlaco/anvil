# ADR-0031: One engine, two front ends

- **Status:** Accepted. Partly implemented (see §Consequences).
- **Date:** 2026-09-05
- **How it was decided:** by management, in this repo, while building the
  graphical editor, and stated as the reason the editor hosts the engine at all
  rather than reimplementing any part of it: *"la idea es que esté el IDE
  gráfico y luego servir el host como ejecutor cli. por eso el motor tiene que
  ser el mismo"*. The byte-level claim in §Context was **verified by running
  the comparison** in this session; everything else about today's state is
  **verified by reading the code** and cited with file and line.
- **Relates to:** ADR-0001, ADR-0005, ADR-0011, ADR-0019, ADR-0029, ADR-0030
- **Scope:** decides that the engine is **one artefact serving two front ends**,
  and what that forbids. It does **not** design either front end, does **not**
  change the engine, `paso.proto` or the WIT, and does **not** decide how the
  editor is distributed.

## Context

Anvil now has two ways to run a sequence, and they are not two products.

The command line is `anvil secuencia.yaml`: a native binary hosting wasmtime
that runs the engine guest and the embedded executor guest
(`packaging/anvil-host/src/main.rs:57-58`, ADR-0011). The graphical editor
transpiles the *same* guest to JavaScript and hosts it in a browser tab.

That the two share a component is not a design aspiration; it is a fact of the
build, verified on 2026-09-05 by comparing the artefacts:

```
componente (target/wasm32-wasip2/release/anvil-guest.wasm):  1.232.241 bytes
core extraído (editor/generated/anvil.core.wasm):            1.213.314 bytes
el core entero está contenido en el componente, en offset 5277
```

The transpiler extracts the core module without recompiling it. The ~19 kB of
difference is the component-model wrapper. So the code that loads a sequence,
evaluates an expression and judges a measurement is the same machine code in
both, and only the provider of its WASI imports differs.

The temptation this ADR exists to refuse is small and constant. A browser can
parse YAML; a JavaScript library could check a sequence far faster than
instantiating a 1.2 MB component; a "quick" client-side check of a step's
fields would make the editor feel snappier. Each of those is a second
implementation of a rule that already exists, and a second implementation that
disagrees with the first is exactly how a sequencer tells someone their
sequence is fine and then refuses it on the bench — or worse, runs it
differently.

## Decision

**There is one engine. The graphical IDE and the command line are two front
ends over it, and neither may contain a second implementation of anything the
engine does.**

1. **No second loader, no second validator, no second evaluator.** Whether a
   sequence is valid, what a field means, what an expression evaluates to and
   how a verdict is reached are answered by running the engine, never by code
   that reimplements it. The editor's status bar shows the loader's own
   diagnostic, verbatim, because rewriting it would create a second wording
   that drifts from the first.

2. **The engine is not forked or patched for a front end.** If the editor needs
   something the engine does not do, the engine gains it, for both. A change
   whose only consumer is the editor still ships to the command line, and must
   be acceptable there — which is why ADR-0029 puts execution events on stderr
   rather than behind an import only a JavaScript host could satisfy.

3. **A front end may offer less, never more.** The editor can decline to expose
   a field; it must not accept one the loader rejects, and must not invent
   defaults, coercions or conveniences the engine does not implement. This is
   what makes it impossible to build a sequence in the editor that the loader
   then refuses.

4. **The two must be the same build, and that has to be checkable.** Sharing a
   source artefact is not enough if the two can carry different compilations of
   it — see §Consequences, which makes this the one part of this ADR with work
   attached.

5. **The command line stays the reference.** It is how Anvil is distributed and
   run (ADR-0011), it is what the regression suite exercises, and where the two
   front ends could differ, the command line is right by definition. The editor
   is the newer surface and carries the burden of matching.

## Alternatives discarded

**A JavaScript validator in the editor, with the engine as the authority.**
Fast feedback while typing, engine consulted on save. Rejected: two
implementations of the same rules is the defect, and making one of them
advisory does not remove it — it just decides which one gets to be wrong
quietly. Measured against a real sequence, instantiating the component is
tens of milliseconds, which is not the bottleneck it was assumed to be.

**A shared Rust core compiled twice — once into the engine, once into a
`wasm32-unknown-unknown` library for the editor.** Genuinely one
implementation, and it would drop the WASI shim layer entirely. Rejected for
now because it splits the artefact in two and reintroduces exactly the
drift this ADR is about, at build level instead of source level. Worth
revisiting if the WASI shim proves unmaintainable.

**Let them diverge and reconcile at the report.** Rejected on ADR-0019's Rule
1: what Anvil cannot judge, it does not guess. An editor that disagrees with
the engine is guessing.

## Consequences

- **The editor cannot be made "lighter" by cutting the engine out.** Its cost
  floor is hosting a 1.2 MB component, and that is the price of the guarantee.
- **A guard against profile drift is now required, not merely advisable.**
  This is the concrete work this ADR creates. `packaging/anvil-host/build.rs:37`
  takes the guest from the profile the host is built with, while the editor
  transpiles `release` unconditionally. So `make build` produces a binary
  carrying a different compilation from the one the editor holds, silently.
  Under this ADR that is not a nuisance, it is the promise breaking: the
  symptom is the editor and the CLI disagreeing about the same file, which is
  among the most expensive things to diagnose. The build must refuse or warn.
- **ADR-0029 and ADR-0030 are constrained by this**, and were already written
  to be: events go out by a route all hosts have, and the browser path adds a
  host rather than replacing the native one.
- **The regression suite gains a reason to run twice.** `docs/qa/regresion/`
  asserts correct behaviour of the engine; nothing yet asserts that the two
  front ends answer alike. A case that runs the same sequence through both and
  compares is the natural check and does not exist.
- **The embedded executor is not shared.** The binary embeds two guests
  (`main.rs:57-58`); the editor hosts only the engine. What is a neighbouring
  `Store` in one process becomes a hop across the bridge (ADR-0030) in the
  other. This ADR is about the engine, and claims nothing about the executor.
