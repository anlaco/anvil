# Design: language executors and the `.wasm` loader

> **Priority:** extended MVP. The embedded WASM executor already exists; the
> name→endpoint routing is **implemented in M5-ext.1** (ADR-0013); the
> `.wasm` loader by path is **implemented in M5-ext.2** (ADR-0014, agnostic
> to the `.wasm`'s origin); LID is a deployment pattern **postponed to
> M5-ext.3**.

How Anvil calls steps in **any language** and **its own WASM modules**
without recompiling. Traceable to
[ADR-0015](../adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md),
[ADR-0014](../adr/0014-cargador-wasm-host-side-m5-ext2.md) (superseded on
the `.wasm` contract), [ADR-0013](../adr/0013-cargador-wasm-host-side-y-routing.md),
[ADR-0012](../adr/0012-executores-de-lenguaje-como-modulos.md) (superseded
on the loader and the routing),
[ADR-0003](../adr/0003-pasos-por-grpc-por-nombre.md) and
[ADR-0011](../adr/0011-distribucion-un-binario-hospeda-wasmtime.md).

## The complete model

```
                    ┌──────────────────────────────────────────────┐
                    │  anvil-host (native bin, ADR-0011)           │
                    │  ┌────────────────┐    ┌──────────────────┐  │
Motor (WASM) ─gRPC─▶│  │ ejecutor.wasm  │◀──▶│  .wasm modules   │  │
 name→endpoint      │  │  (embedded)    │    │  loaded by        │  │
                    │  │  · pasos_demo  │    │  path (.vi model) │  │
                    │  │  · built-in    │    │  · own Store      │  │
                    │  └────────────────┘    └──────────────────┘  │
                    └───────────┬──────────────────────────────────┘
                                │ gRPC (same contract)
                    ┌───────────▼──────────────────────────────┐
                    │  executors/  (Apache-2.0 modules)        │
                    │  python/  ·  labview/ (future)  ·  ...    │
                    │  └─ (optional) on an LID: legacy OS (Win7)│
                    │     with declared doors                   │
                    └───────────┬──────────────────────────────┘
                                │ TCP/SCPI/etc.
                                ▼
                        Instrument / simulator
```

- The engine dispatches by **name→endpoint**: it neither knows nor cares
  whether the step is served by the embedded executor, a loaded `.wasm`, or
  a Python executor on another machine.
- They all speak the **same `paso.proto`**. The contract does not change
  (RNF-05).

## The embedded WASM executor (the default one)

- **Zero-install**: it lives inside `anvil-host` (ADR-0011). WASM/Rust is
  the **default language** of a test executor.
- It serves the **built-in** steps: `pasos_demo` compiled in (pass/fail,
  limit test, action, simulated connect/measure/disconnect). Always
  available, on `127.0.0.1:9100`.
- It does **not** load `.wasm` by path: a WASM guest cannot instantiate
  wasmtime inside itself (ADR-0013). That is the **host's** job (see
  below).

### Name→endpoint routing (M5-ext.1, implemented)

The YAML declares `executors:` and each `grpc` step may declare
`executor:`. The engine dispatches by **name→endpoint** (connection table in
`Motor::desde_programa`); without a declaration, everything goes to the
embedded executor (M4b compatibility).

```yaml
executors:
  - name: embebido        # the default WASM executor (127.0.0.1:9100)
    type: embedded
  - name: python          # a separate language executor
    type: grpc
    host: 127.0.0.1         # or 192.168.x.y (future LID) — only if declared
    port: 9101
main:
  - name: verificar_led   # embedded (default)
  - name: medir_simulador
    executor: python
```

CLI override: `--executor python=192.168.1.50:9101` (the `--limits`
pattern). Non-loopback IPs only if declared (ADR-0011's bounded loopback
relaxation); the host flag `--loopback-only` rejects them.

### `.wasm` loader by path (the `.vi` model, M5-ext.2, implemented)

Like TestStand with a `.vi`: you compile the module, save it to a file, and
the sequence references it by path. **Nothing gets recompiled.**

```yaml
executors:
  - name: mi_paso_wasm      # free key for the sequence
    type: wasm                # component loaded by the HOST (ADR-0015)
    path: ./pasos/mi_paso.wasm  # relative to the YAML
```

- **The user's `.wasm` is a WASM component exporting `run` and `describe`**
  (WIT interface `anvil:step`, ADR-0015, ADR-0024). It is not a gRPC server: it
  knows nothing about gRPC, protobuf or Anvil. The step's author annotates a
  plain Rust function with `#[step]` from the `anvil-step` SDK (public,
  crates.io, Apache-2.0) and compiles it with
  `cargo build --target wasm32-wasip2` — no `wasi-grpc`, no `modelo`, no
  `cargo component`, no cloning the repo.
- **The host spawns the bridge `anvil-exec-wasm`** (a file next to the
  `anvil` binary — ADR-0023; it used to be embedded) with
  `--wasm <path> --port <ephemeral>`.
  The bridge (native: wasmtime + tonic + wit-bindgen) loads the component
  into a Store with an **empty** WASI sandbox (no files, no network: the
  component is a pure function — real isolation) and translates
  gRPC↔function: for each `Invoca` of the engine it calls
  `run(nombre, intento)` and returns the result as `ResultadoPasoProto`.
  `paso.proto` does not change (RNF-05); the translation lives inside the
  executor.
- **One bridge per path** (deduplicated: two executors with the same
  `.wasm` → one bridge). Preload at startup, readiness by polling, ephemeral
  port (`bind 127.0.0.1:0`).
- **The engine never runs `Wasm`** (ADR-0014/0015): the host composes a
  synthetic `--executor name=127.0.0.1:<port>` override (M5-ext.1, which
  already turns `wasm` into `grpc`), so the engine only sees
  `embebido`/`grpc`, as always. Running `anvil.wasm` loose with the wasmtime
  CLI (no host) against a `wasm` executor gives `Error::EjecutorWasmSinHost`
  with a clear message.
- **Remote case (Raspberry Pi, ADR-0023)**: the bridge ships as a file next
  to `anvil` and is run with `--bind 0.0.0.0`; the YAML declares
  `tipo: grpc, host: 192.168.x.y`. Anvil cannot tell: the local bridge and
  the Pi's are the same binary.
- **Performance (50+ modules)**: wasmtime compiles **JIT to native** (it
  does not interpret). AOT precompile to `.cwasm` + `StoreLimitsBuilder`
  are **post-M5-ext.2** (once RSS is measured). Detail in
  `docs/planes/m5-ext.md`.

> **Pattern supported since M5-ext.1** (no milestone of its own): a **single
> `.wasm` dispatching by name** (a module that serves N names internally) is
> just another `grpc` executor — 1 Store, N calls. Anvil cannot tell whether
> behind it there is one loose `.wasm` by path (M5-ext.2) or a module that
> fuses several steps. It is the analogue of TestStand's Run-Time Engine: if
> a generator produces that format, it works with nothing special.

## Language executors (`executors/`)

Separate modules, one per system, distributed with Anvil, **Apache-2.0**
(adoptable, ADR-0012):

```
executors/
  python/    # gRPC server in Python (M5)
  labview/   # future
  matlab/    # future
```

- They are **alternatives**: you start the one you need; they can run at the
  same time and mix in the same sequence.
- They speak the same `paso.proto` with **their ecosystem's native gRPC**
  (`grpcio`, `tonic`, …), not `wasi-grpc` (that one is only for WASM,
  ADR-0006).
- The engine needs no such runtime installed (ADR-0003); whoever runs the
  executor does install it on their machine — their choice, not a requirement
  of Anvil.
- Each module is self-contained and versionable → **downloadable from the
  UI** once it exists (post-MVP).

### Naming: `anvil-exec-<language>`

Every executor a user launches is called `anvil-exec-<language>`, and the
language is the one the steps are written in, not the transport:

```
anvil-exec-wasm      # the bridge: serves a user's .wasm step component
anvil-exec-python    # the Python executor's launcher (server.py underneath)
anvil-exec-labview   # future
anvil-exec-native    # future: an external executable, TestStand's Call Executable
```

The names group on purpose: they sort together in the release directory and
`anvil-e<TAB>` lists the executors installed on a machine without opening the
documentation. The core binary, `anvil`, stays outside the family — it is not
an executor.

Three things the scheme is **not**:

- It is **not** the `type:` of the sequence. The YAML types by transport —
  `embedded`, `wasm`, `grpc` — because the engine does not know what language
  sits behind an endpoint and must not (ADR-0013). `anvil-exec-python` and
  `anvil-exec-labview` are both `type: grpc`. Only `wasm` names a runtime,
  because it is the only one Anvil loads itself.
- It is **not** a rename of the embedded executor. That one is inside the
  `anvil` binary and is never launched by hand, so it has no file name to
  carry (ADR-0011).
- It does **not** replace `server.py`, which stays runnable exactly as before.
  `anvil-exec-python` is a launcher over it: it puts the executor's own
  directory on `sys.path` and hands the command line to `server.main()`.

The word is `executor`, fixed in [`GLOSSARY.md`](../../GLOSSARY.md) — not
`runner`, not `adapter`. `exec` is its abbreviation and not a second word for
the same thing.

> The WASM bridge shipped as `anvil-puente-wasm` until 2026-09-01, a name half
> in Spanish from before the English-only rule. ADR-0023 §Alcance left the
> rename open; this is it. ADR-0014, ADR-0015 and ADR-0023 still use the old
> name and are immutable: they mean this binary.

### Objects that stay in the executor (ADR-0022)

A bench session, an instrument connection, a driver handle: a thing with open
sockets and vendor locks that **cannot cross the wire and must not be reopened
per step**. It stays in the executor's process, and the sequence carries a
`Reference` to it — which is the one thing a language executor can offer that
the embedded WASM one cannot, since `anvil:step` is a function with no state
between calls.

Two duties fall on whoever writes an executor, and **Anvil cannot check either
from outside**. An executor that breaks one is a broken executor:

1. **Never recycle a payload within one lifetime.** If a closed bench's key
   came back for the next open, an old reference would resolve cleanly to a
   live, *different* object: same executor, same lifetime, everything green,
   measuring against the wrong bench. A monotonic counter is what makes this
   impossible; a free list is what makes it happen.
2. **Mint a different lifetime on every start**, and publish it in
   `Catalog.lifetime`. A process that came back on the same lifetime would make
   its own restart undetectable, for Anvil and for itself.

And one it should do because it is the only one that can: **reject a reference
whose lifetime is not its own**. Anvil knows this only by comparison; the
executor knows it with certainty.

The Python executor is the worked example — `ctx.objects` is the store, and
`executors/python/steps/instrument.py` ships the shape any object steps take:
one opens and mints, several use, one closes. What it does *not* do is mint a
new handle when a step merely changes the bench: the reference names a slot,
and answering a new one would break retries.

### LID: deployment on legacy OSes (a pattern, not a component — postponed to post-M5-ext)

When a step needs DLLs/drivers of an OS Anvil does not offer (Windows 7/10,
old Ubuntu), **any** language executor can deploy on that legacy OS with
**declared isolation** — a *Legacy Isolation Domain*:

- Only the **declared doors** go out (network instruments, agreed files);
  the rest is isolated.
- Anvil sees one more gRPC endpoint: `192.168.x.y:9100` (a networked PC) or
  a local VM/container. It neither knows nor cares about the OS.
- **Postponed to post-M5-ext** (modern first, legacy later): the pattern is
  fixed, but the **isolation mechanism is defined when building it**
  (container / VM / OS firewall). The exhaustive option survey
  (QEMU/KVM, Hyper-V, Sandboxie-Plus, Docker, systemd-nspawn,
  namespaces, Windows Sandbox, Firecracker, gVisor, WSL2, …) with verified
  sources and a recommendation per topology lives in
  [investigacion/aislamiento-lid.md](../investigacion/aislamiento-lid.md).

## Routing configuration

Embedded first, sidecar later (same as the limits, RF-30):

1. **Embedded in the sequence's YAML** (MVP): the `executors:` section,
   versionable with the sequence.

   ```yaml
   executors:
     - name: embebido        # the default WASM executor
       type: embedded
     - name: mi_paso_wasm    # .wasm module loaded by path
       type: wasm
       path: ./pasos/mi_paso.wasm
     - name: python          # a separate language executor
       type: grpc              # same contract, other process/host
       host: 127.0.0.1         # or 192.168.x.y (LID) — only if declared
       port: 9101
   ```

   And each step references its executor: `executor: python` in
   `DefinicionPaso` (or a default executor if none is declared).

2. **CLI flag override** (MVP): `--executor python=192.168.1.50:9100` to
   point an executor at another endpoint without touching the YAML (R&D vs.
   factory), like `--limits` already does.

3. **Reusable sidecar** (post-MVP): a config file shared by several
   sequences.

With no `executors:` declared, everything goes to the embedded executor on
loopback — identical behavior to M4b (ADR-0011 compatibility).

## Demo M5-ext.1 (done, no Docker)

The real demo is `ejemplos/demo_ejecutores.yaml`: **embedded + Python on
loopback** (no Docker, no LID).

```yaml
name: demo_ejecutores
executors:
  - { name: embebido, type: embedded }
  - { name: python, type: grpc, host: 127.0.0.1, port: 9101 }
main:
  - name: verificar_led        # embedded (default)
  - name: medir_simulador, executor: python
  - name: conectar_equipo, executor: python
```

Verification: the sequence passes/fails per step, and the report shows steps
served by two different executors without the engine knowing anything about
the language. The demo with an own `.wasm` step (`tipo: wasm`) is
`ejemplos/demo_wasm.yaml` (M5-ext.2, ADR-0015): the host spawns the bridge,
which loads the `ejemplos/hola-paso` component (the "hello world") and calls
its `run`; the engine dispatches the three steps (embedded + component) with
the limit and retries evaluated by the engine. See
[ADR-0015](../adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md).

## Extended-MVP cuts

- AOT cache of `.wasm` modules (post-M5-ext.2, once RSS/threads are measured
  with 50+ modules).
- Sidecar for `executors:` (post-MVP).
- Auto-discovery / balancing / reconnect per endpoint (post-MVP; only the
  per-step retry exists, RF-07).
- Downloadable from the UI (post-MVP; the structure allows it).
- LID: pattern documented, postponed to M5-ext.3; technology defined when
  building it.

## Out-of-scope

- Language executors other than Python in the MVP (LabVIEW/MATLAB: future).
- WASM inside the LID (impossible with native DLLs; their isolation is by
  declared network/FS).
- Changes to `paso.proto` (RNF-05).