# Anvil documentation

Anvil is an **open-source** production test sequencer that competes with
NI TestStand: it runs sequences of steps against real equipment (instruments),
retries the ones that fail and reports. The sequence is **data, not code**, and
every step is invoked **over gRPC by its name** — never through a direct
call —, which isolates steps from each other and leaves the door open to
writing them in any language.

> This documentation was born **pre-development** —to pin down *what* Anvil is,
> *why* and *how* before building it— and it is still the product's living
> specification. With the MVP closed (M0→M5, see [roadmap.md](roadmap.md)), it
> lives alongside usage documentation: [guia-inicio-rapido.md](guia-inicio-rapido.md)
> and the ADRs, which are the record of what is already built.

## To start right away

**[guia-inicio-rapido.md](guia-inicio-rapido.md)** — from zero to running a
sequence with subsequences in 5 minutes (build, tests, end-to-end smoke).

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
├─ glosario.md                domain vocabulary (TestStand + Anvil)
├─ vision.md                   vision, value proposition, competition, MVP scope
├─ requisitos.md              functional and non-functional requirements (light SRS)
├─ arquitectura.md            C4 architecture, levels 1–3
├─ contrato-grpc.md           semantics of the step contract over paso.proto
├─ licencia.md                dual AGPL / Apache licensing strategy
├─ roadmap.md                 milestones M0 → M4+ with MVP vs. post-MVP
├─ adr/                       decisions already made (immutable, Nygard template)
│  ├─ 0001-rust-wasm.md
│  ├─ 0002-secuencia-como-datos.md
│  ├─ 0003-pasos-por-grpc-por-nombre.md
│  ├─ 0004-licencia-dual-agpl-apache.md
│  ├─ 0005-motor-generico-dirigido-por-datos.md
│  └─ 0006-wasi-grpc-propio.md
│  (0007-0018 in the same folder; 0013 replaces 0012 in the loader and routing)
├─ planes/                    milestone plans (m4-nucleo, m4b, m5-ext)
├─ qa/                        campaign reports + executable checks
│  ├─ regresion/run.sh        the defects from the August 2026 beta
│  └─ referencia/run.sh       ADR-0022 end to end (needs grpcio)
└─ diseno/                    domain design, one doc per functional area
   ├─ motor-de-ejecucion.md
   ├─ limites-y-estados.md
   ├─ modelo-de-pasos.md
   ├─ formato-de-secuencia.md
   ├─ reportes.md
   ├─ variables-y-alcances.md
   ├─ integracion-instrumentos.md
   ├─ motor-de-expresiones.md
   ├─ proceso-de-test.md
   └─ ui-vs-headless.md
```

At the root of the repo, the community files:
[`CONTRIBUTING.md`](../CONTRIBUTING.md),
[`GOVERNANCE.md`](../GOVERNANCE.md),
[`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md),
[`SECURITY.md`](../SECURITY.md); and the change history by version in
[`CHANGELOG.md`](../CHANGELOG.md).

## Conventions

- **Markdown in Spanish**, consistent with the repo's `README.md`.
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

**The MVP is closed** (M0→M5 + M5-ext.1/2, see [roadmap.md](roadmap.md)).
Anvil is distributed as **one binary** that hosts wasmtime and the two WASM
guests (ADR-0011): YAML sequences with limits, variables, expressions,
preconditions and subsequences; report to console/JSON/CSV; Sequential process
model; multi-executor routing; user steps as WASM components loaded by path.
On top of that base there is a first external beta campaign —600+ runs— with
its findings and reproductions in
[`qa/informe-beta-2026-08.md`](qa/informe-beta-2026-08.md).

What comes after the MVP (parallelism, Operator UI, sector-specific sinks…)
follows the same criterion as always: what is decided is **formalised** in these
docs; what is not is **proposed** as a design decision marked as a proposal.
