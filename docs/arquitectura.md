# Architecture

Anvil's architecture in **C4 levels 1–3** (Context, Container, Component).
It replaces the IEEE 1016 SDD (12 viewpoints) — overkill here — keeping what
matters: what gets built, how things are isolated, where state lives and
where the license boundary sits.

The underlying decisions live in the [ADRs](adr/); this doc is the *how*.

## Level 1 — System context

```
                  ┌───────────────┐
   Test           │   Anvil       │      ResultSinks
   engineer ─────▶│  sequencer    │─────▶ (console/JSON/CSV/
  (authors YAML)  │  (WASM host)  │      SQLite/STDF post-MVP)
                  └───────┬───────┘
                          │ gRPC (by name)
                          ▼
                  ┌───────────────┐         ┌──────────────────┐
   Operator ─────▶│  Step         │────────▶│  Steps / code     │
  (runs on the    │  executor     │ gRPC    │  modules (any     │
   shop floor)    │  (WASM)       │         │  language)        │
                  └───────────────┘         └────────┬─────────┘
                                                      │ SCPI/Visa (post-MVP)
                                                      ▼
                                               ┌──────────────┐
                                               │ Instruments  │
                                               │  (hardware)  │
                                               └──────────────┘
```

- **Test engineer** authors the sequence as YAML (data) and versions it in
  Git.
- **Operator** runs the sequence on the bench, headless/CLI in the MVP.
- **Anvil (sequencer)** = the WASM engine. Walks the sequence and asks for
  each step over gRPC.
- **Step executor** = a gRPC server that dispatches steps by name. In the
  MVP it hosts the steps in the same `.wasm`; the goal is for steps to be
  **gRPC services in any language**.
- **Steps / code modules** = the measurement logic, in any language. They
  touch the **instruments** (SCPI/VISA post-MVP).
- **ResultSinks** receive results as open data (today `println!`).
- **wasi-grpc** (external lib, Apache-2.0, `../wasi-grpc`) is the transport
  stack: not Anvil's, but its foundation ([ADR-0006](adr/0006-wasi-grpc-propio.md)).

## Level 2 — Containers

Things that deploy or exist independently:

```
┌─────────────────────────────────────────────────────────────┐
│  Motor  (crates/motor → motor.wasm, wasmtime)                 │
│  gRPC client. Walks DefinicionSecuencia, applies semantics.   │
│  Holds ALL the run's state in memory.                         │
└─────────────────────────────────────────────────────────────┘
        │ gRPC  /EjecutorPasos/Invoca   (wasi-grpc, one stream/call)
        ▼
┌─────────────────────────────────────────────────────────────┐
│  Step executor  (crates/ejecutor_pasos → .wasm, wasmtime)     │
│  gRPC server on 127.0.0.1:9100. Dispatches by name.           │
│  MVP: hosts pasos_demo in the same .wasm. Stateless across    │
│  calls.                                                       │
└─────────────────────────────────────────────────────────────┘
        │ in-process direct call (MVP)  ──── future: gRPC to
        ▼                                      step servers
┌─────────────────────────────────────────────────────────────┐
│  Steps  (crates/pasos_demo today; any language tomorrow)     │
│  Code modules: medir_voltaje, verificar_led, conectar…        │
└─────────────────────────────────────────────────────────────┘

┌──────────────┐   ┌──────────────────────┐   ┌─────────────────┐
│ Sequence     │   │ ResultSink (post-MVP) │   │ wasi-grpc (lib)  │
│ YAML (data)  │   │ console/JSON/CSV/      │   │ Apache-2.0,      │
│ (RF-20)      │   │ SQLite/STDF (RF-21+)   │   │ separate repo    │
└──────────────┘   └──────────────────────┘   └─────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  modelo  (crates/modelo, lib)  — shared, does NOT deploy alone │
│  DefinicionSecuencia, ResultadoStep, proto.rs messages.       │
│  License: AGPL (part of the product).                         │
└─────────────────────────────────────────────────────────────┘
```

**Current MVP vs. goal (honest):**

| Container | Today (prototype) | Goal |
|---|---|---|
| Engine | `motor.wasm` builds the sequence in code (`basica_datos.rs`) | Reads sequence YAML (RF-20) |
| Executor + Steps | Same `.wasm`; `pasos_demo` linked in-process | Steps as gRPC services in any language |
| Report | Frozen textual `println!` | Decoupled ResultSink (RF-21) |
| UI | No UI (CLI) | Operator web UI post-MVP |
| Test process | Implicit Sequential (one sequence) | Sequential process model + plug-ins |

The **gRPC engine↔executor border already exists and is real**
(engine-side isolation). The executor↔step border is in-process today; the
goal is gRPC for steps in any language (ADR-0003).

**M5-ext.1 (ADR-0013):** the embedded WASM executor stays as the **default**
(zero-install, ADR-0011); the engine dispatches by **name→endpoint**
(`executors:` in the YAML + the `--executor` override), with non-loopback
IPs only if declared. Beside it, **language executors** (`executors/`,
Apache-2.0) serve steps with their ecosystem's native gRPC.

**M5-ext.2 (ADR-0014/0015):** the **`.wasm` module loader by path** (the
`.vi` model of TestStand) is the **host's** job: for every `tipo: wasm` in
the YAML it spawns the **bridge** `anvil-puente-wasm` (a file next to the
`anvil` binary since ADR-0023 — no longer embedded), which loads the user's
`.wasm` component (WIT interface `anvil:step`: a `run` function, no gRPC, no
protobuf) and translates gRPC↔function with tonic. The bridge runs with an
empty WASI sandbox (the component is a pure function). The engine only sees
synthetic `--executor` overrides — never a `Wasm`. The **LID** pattern for
legacy OSes is postponed to post-M5-ext. See
[diseno/executores-lenguaje.md](diseno/executores-lenguaje.md),
[ADR-0013](adr/0013-cargador-wasm-host-side-y-routing.md),
[ADR-0014](adr/0014-cargador-wasm-host-side-m5-ext2.md) and
[ADR-0015](adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md).

## Level 3 — Components

### Inside the engine (`crates/motor/src/lib.rs`)

```
Motor
 ├─ desde_programa(programa)     → connection table per executor (M5-ext.1)
 ├─ conecta(host, puerto)        → wasi-grpc client (legacy, embedded)
 ├─ ejecuta_paso(def, programa)  → resolves the endpoint by def.ejecutor, encodes
 │                                 PeticionPaso, calls RUTA_INVOCA, decodes
 ├─ ejecuta_con_reintentos(def)  → retries while !paso() && intento<max
 └─ ejecuta_secuencia(def)       → Setup / Main(stops at 1st fail) / Cleanup(always)
                                   + aggregates into ResultadoSecuencia
```

- **State:** the whole run lives in `ResultadoSecuencia` **in memory**; no
  persistence in the MVP. The ResultSink (post-MVP) will pour it out.
- **Errors:** `Error::Red` (communication) / `Error::Protobuf` (unreadable
  response). A step that *fails* is **not** an engine error (RF-11).

### Inside the executor (`crates/ejecutor_pasos/src/main.rs`)

```
Executor
 ├─ Servidor::escuchar(127.0.0.1:9100) → accept() one connection
 ├─ loop: siguiente_peticion()         → validates path == RUTA_INVOCA
 │   ├─ decodes PeticionPaso
 │   ├─ pasos_demo::despacha(nombre, intento)   ← the only name→function spot
 │   └─ responds(stream, ResultadoPasoProto)
 └─ Stateless across calls
```

- **Dispatch by name:** `pasos_demo::despacha` is the **only** place where
  the wire's name gets tied to a function. Unknown name → `error` (RF-12).
- Each call spends a **new HTTP/2 stream** (handled by wasi-grpc).

## Why WASM

See [ADR-0001](adr/0001-rust-wasm.md). In short: **isolation** (the
sequencer's sandbox; each step's interior is opaque to the engine) +
**portability** (a `.wasm` runs on any OS with wasmtime, no installer) +
**determinism** (the basis for reproducible retries). The cost (no
`tonic`/`tokio` → own stack, no codegen → hand-written structs) is paid in
[ADR-0006](adr/0006-wasi-grpc-propio.md) and `crates/modelo/src/proto.rs`.

**Performance (ADR-0012):** wasmtime compiles WASM **JIT to native code**
(it does not interpret it): ~1.5–2× of native C/Rust and far ahead of plain
Python; versus a native DLL it pays ~10–30% for the sandbox, negligible
next to the time of a real instrument (RNF-04). There is no reason for
"fast stuff in DLLs, slow stuff in WASM".

## Where state lives

- **The run** (the result in progress): in memory, in the engine, as
  `ResultadoSecuencia`. It does not persist in the MVP.
- **The definition** (what to run): data (`DefinicionSecuencia`), today
  built in code, tomorrow YAML. It is **inert**: it does not mutate while
  running.
- **The executor**: stateless across calls. It stores nothing about the
  sequence.
- **Sequence variables** (Locals/Parameters/FileGlobals, post-MVP): they
  will live in the engine, bound to the run's scope
  ([diseno/variables-y-alcances.md](diseno/variables-y-alcances.md)).

## Concurrency model

- **MVP: sequential.** One connection, one stream per call, one sequence at
  a time. No threads in the engine; retry determinism depends on that
  (RNF-03).
- **Post-MVP: parallelism with hierarchical cancellation** (Parallel/Batch),
  with a *CancellationToken* (OpenTAP TapThreads style) to abort in cascade.
  See [diseno/proceso-de-test.md](diseno/proceso-de-test.md).

## License border (Apache / AGPL)

```
AGPL-3.0-or-later                     Apache-2.0
─────────────────────                ─────────────────────
anvil (the product):                  wasi-grpc   (lib, linkable)
  motor, ejecutor_pasos,              wasi-visa   (lib, linkable, post-MVP)
  modelo, pasos_demo                  WIT interfaces
   (they are USED, not linked)
```

- Whoever **uses** Anvil (runs it) catches no AGPL.
- Whoever **links** the libs (writes a step that links
  `wasi-grpc`/`wasi-visa`) is under Apache: their code belongs to them.
- **Sequences** are data: not a derivative work, they infect nothing
  ([ADR-0004](adr/0004-licencia-dual-agpl-apache.md), [licencia.md](licencia.md)).

## Determinism and performance

- **Determinism:** for the same sequence and the same steps, the number of
  attempts and their order are reproducible because there is no implicit
  concurrency in the MVP (RNF-03). Verified in CI with `pasos_demo`'s
  simulated steps (e.g. `conectar` fails attempt 1 and passes 2).
- **Performance:** the overhead of a local gRPC call is negligible next to
  the time of a real instrument (RNF-04). It is not the bottleneck.