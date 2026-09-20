# Anvil documentation

Anvil is an **open-source** production test sequencer that competes with
NI TestStand: it runs sequences of steps against real equipment (instruments),
retries the ones that fail and reports. The sequence is **data, not code**, and
every step is invoked **over gRPC by its name** — never through a direct
call —, which isolates steps from each other and leaves the door open to
writing them in any language.

> This documentation was born **pre-development** —to pin down *what* Anvil is,
> *why* and *how* before building it— and it is still the product's living
> specification. It lives alongside usage documentation —
> [guia-inicio-rapido.md](guia-inicio-rapido.md), the
> [root README](../README.md) and each executor's own README under
> [`executors/`](../executors/README.md)— and the ADRs, which are the record of
> what is already built. Where a design doc and the code disagree, the code
> wins and the doc is corrected.

## To start right away

**[The Anvil Book](book/README.md)** — learn to use Anvil from nothing: install
it, write steps in C#, write and run sequences, read the reports. Every command
in it is checked against the release it names.

**[guia-inicio-rapido.md](guia-inicio-rapido.md)** — download or build Anvil,
run a sequence, and write your own step. For the steps themselves, by
language: [Python](../executors/python/README.md),
[Rust (compiled to WebAssembly)](../executors/rust/README.md) and
[C#](../executors/csharp/README.md).

## How to read this documentation

A test engineer coming from TestStand can understand what Anvil is, what it
does, what it does not do and why by reading only three files, in this order:

1. **[vision.md](vision.md)** — what Anvil is, what it competes against, who it
   is for, what goes into v1 and what does not.
2. **[requisitos.md](requisitos.md)** — what it must do, verifiable and traceable
   to the contract (`crates/modelo/paso.proto`).
3. **[arquitectura.md](arquitectura.md)** — how it is built (C4 levels 1–3).

The rest goes deeper by area. If a term is not clear, it is in the
[glossary](glosario.md).

## Map of the tree

```
docs/
├─ README.md                  this index
├─ book/                      The Anvil Book: learning to use Anvil, checked by check.sh
├─ glosario.md                domain vocabulary (TestStand + Anvil)
├─ vision.md                   vision, value proposition, competition, MVP scope
├─ requisitos.md              functional and non-functional requirements (light SRS)
├─ arquitectura.md            C4 architecture, levels 1–3
├─ contrato-grpc.md           semantics of the step contract over paso.proto
├─ licencia.md                dual AGPL / Apache licensing strategy
├─ paridad-teststand.md       what Anvil does and does not do vs. TestStand (generated)
├─ roadmap.md                 milestones M0 → M4+ with MVP vs. post-MVP
├─ adr/                       decisions already made (immutable), 0001–0044
│                             see "The ADRs, by area" below
├─ planes/                    milestone plans (m4-nucleo, m4b, m5-ext)
├─ qa/                        campaign reports + executable checks
│  ├─ informe-beta-2026-08.md the August 2026 external beta
│  ├─ regresion/run.sh        the defects from that beta
│  └─ referencia/run.sh       ADR-0022 end to end (needs grpcio)
├─ investigacion/             research the decisions lean on
│  ├─ TestStand-y-competencia.md
│  └─ aislamiento-lid.md
└─ diseno/                    domain design, one doc per functional area
   ├─ motor-de-ejecucion.md
   ├─ limites-y-estados.md
   ├─ modelo-de-pasos.md
   ├─ formato-de-secuencia.md
   ├─ executores-lenguaje.md
   ├─ reportes.md
   ├─ variables-y-alcances.md
   ├─ integracion-instrumentos.md
   ├─ motor-de-expresiones.md
   ├─ proceso-de-test.md
   ├─ ui-vs-headless.md
   └─ principios-del-editor.md
```

### The ADRs, by area

Three to read first, because the rest cite them:
[0005](adr/0005-motor-generico-dirigido-por-datos.md) (the engine does not know
the domain), [0019](adr/0019-que-hace-anvil-cuando-no-puede-juzgar.md) (what
Anvil does when it cannot judge) and
[0020](adr/0020-parametros-del-paso-en-la-peticion.md) (the step's contract).

- **Foundations** — 0001–0006: Rust and WASM, the sequence as data, steps over
  gRPC by name, the dual licence, the data-driven engine, wasi-grpc.
- **The engine and the sequence** — 0008–0010, 0016, 0018, 0019, 0029, 0033:
  limits, expressions, sequence calls, the process model, the `pass_fail`
  verdict, what Anvil does when it cannot judge, the NDJSON event stream.
- **Reports** — 0007: the SQLite sink, postponed.
- **Steps and executors** — 0011–0015, 0017, 0020–0028: one binary hosting
  wasmtime, executors as modules and departments, the WASM bridge, the real
  SCPI instrument adapter, the step's typed contract, the catalog
  (`Describe`), object references, the Rust SDK.
- **Front ends** — 0030, 0031, 0034–0037: the engine in a browser, one engine
  for two front ends, the engine as a service, the Sequence Editor's shell.
  [0043](adr/0043-the-editor-is-laid-out-as-teststand-and-declares-what-it-does-not-do.md):
  the editor's information architecture is TestStand's, and what Anvil does not
  do is greyed in place with one of three verdicts — `todo`, `elsewhere` or
  `never`. The engine unlocks a cell, never the editor. Accepted, not
  implemented. The editor's own principles (the **AP**) are written down in
  [principios-del-editor.md](diseno/principios-del-editor.md).
- **0.5.0** — [0038](adr/0038-the-csharp-step-sdk-is-hosted-by-the-users-own-process.md)
  (steps in C#) and [0039](adr/0039-a-sequence-file-may-end-in-yseq.md) (the
  `.yseq` extension).
- **Step types** — [0040](adr/0040-a-step-type-says-how-a-step-is-judged-not-what-it-calls.md):
  a step's `type` says how it is judged (`action`, `pass_fail`,
  `numeric_limit`, TestStand's), and what it calls is its `module`.
  [0041](adr/0041-there-is-no-embedded-executor.md): `anvil` carries no step
  executor, a step names its executor, and the demo is a WASM component.
  [0042](adr/0042-what-adr-0040-left-unsaid.md): what 0040 left unsaid —
  `result` in `condition`, the order of `assign`, retries. Accepted, not
  implemented.
- **The catalog** — [0044](adr/0044-the-catalog-comes-out-as-data-anvil-describe.md):
  `anvil describe <sequence>` prints what each declared executor serves, and
  with what signature, as JSON. It is the *enumerate* operation ADR-0025 §4
  named and nothing implemented — the one an editor draws a parameter table
  from instead of guessing.
- **Governance** — 0032: contributions and the reversion clause (a direction,
  not yet in force).

ADRs up to 0022 are in Spanish; from 0023 they are in English.

At the root of the repo, the community files:
[`CONTRIBUTING.md`](../CONTRIBUTING.md),
[`GOVERNANCE.md`](../GOVERNANCE.md),
[`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md),
[`SECURITY.md`](../SECURITY.md); and the change history by version in
[`CHANGELOG.md`](../CHANGELOG.md).

## Conventions

- **English.** Since 2026-08-28 everything that enters the repo is written in
  English. Older docs are still in Spanish and are translated when they are
  next opened to be changed, in a commit of their own; a Spanish file is the
  starting point, not an anomaly. [`GLOSSARY.md`](../GLOSSARY.md) fixes the
  translation of each domain term.
- Every requirement and decision is **anchored to its real source** in the repo
  (`crates/modelo/paso.proto`, `crates/modelo/src/lib.rs`, `crates/motor/src/lib.rs`…).
- ADRs are **immutable**: if a decision changes, a new ADR is added that
  replaces it (Status: *Superseded by ADR-00NN*).
- Each design area marks its scope: **MVP** / **MVP-partial** /
  **post-MVP** / **out-of-scope**.
- Data and quotes about TestStand and competitors are referenced to
  [`investigacion/TestStand-y-competencia.md`](investigacion/TestStand-y-competencia.md),
  which has the sources; the URLs are not repeated in each doc.

## State of the product today

**Anvil 0.5.0** (2026-09-13, see [`CHANGELOG.md`](../CHANGELOG.md)). The engine
is **one binary** that hosts wasmtime and the two WASM guests (ADR-0011),
downloadable for Linux (statically linked against musl) and Windows (static
CRT). It runs YAML sequences —`.yaml` or `.yseq`— with limits, variables,
expressions, preconditions and subsequences; reports to console, JSON and CSV;
wraps a sequence in the Sequential process model; and routes each step by name
to its executor. Steps are written in **Python**, in **Rust** compiled to a
WebAssembly component, or in **C#**, and every executor publishes its catalog,
so a sequence is checked before it touches a unit. The **Sequence Editor** is
downloadable too, as an AppImage, a `.deb` and a Windows installer.

The first external beta campaign —more than 600 runs of 180 sequences against
`anvil 0.1.0`— is written up with its findings and reproductions in
[`qa/informe-beta-2026-08.md`](qa/informe-beta-2026-08.md). No beta round has
been run against 0.5.0 yet.

What comes after the MVP (parallelism, Operator UI, sector-specific sinks…)
follows the same criterion as always: what is decided is **formalised** in these
docs; what is not is **proposed** as a design decision marked as a proposal.
