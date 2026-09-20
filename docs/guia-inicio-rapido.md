# Quick-start guide

Anvil is **one binary**: you download it and run it. Inside, it hosts
`wasmtime` and the engine's WASM guest in a sandbox, which speaks gRPC to the
step executors. You need no `wasmtime` install nor any runtime — it is
embedded. The binary carries no step executor of its own
([ADR-0041](adr/0041-there-is-no-embedded-executor.md)): every step names the
executor that serves it. The download also carries `anvil-exec-wasm`, the
executor that serves `.wasm` steps, as a file next to it (ADR-0023), and a
demo bench built with it in `ejemplos/departamento/dist/`, which the examples
run against. You copy that file
into a folder together with your `.wasm` modules — that folder is a
*department* — and a sequence names its binary; `anvil` brings it up itself
(ADR-0027). See
[ADR-0011](adr/0011-distribucion-un-binario-hospeda-wasmtime.md) for the
why.

## For the end user

To learn Anvil step by step rather than look up a command, follow
[The Anvil Book](book/README.md).

Download the package for your platform from the
[release page](https://github.com/anlaco/anvil/releases/latest):
`anvil-vX.Y.Z-x86_64-linux-musl.tar.gz` for Linux (statically linked, any
libc) or `anvil-vX.Y.Z-x86_64-windows.zip` for Windows (static CRT, no
Visual C++ redistributable). Both carry `anvil`, `anvil-exec-wasm` and the
example sequences; on Windows the binaries end in `.exe` and the commands
below are the same. To *write* sequences rather than only run them, the same
page has the Sequence Editor as an AppImage, a `.deb` and a Windows
installer.

Then run:

```sh
./anvil <sequence.yaml> [--process-model <pm.yaml>] [--json <path>] \
  [--csv <path>] [--limits <path>] [--executor name=host:port] \
  [--validate [--with-executors]] [--quiet]
```

Examples (the repo's own are in `ejemplos/`; they call the demo bench's steps,
`demo/<step>`, on the executor they declare as `demo`):

```sh
./anvil ejemplos/subsecuencia.yseq --json ./out.json --csv ./out.csv
./anvil ejemplos/basica.yseq
./anvil ejemplos/limites.yseq
./anvil ejemplos/variables.yseq
./anvil ejemplos/basica.yseq --limits ejemplos/limites.limits.yaml
./anvil ejemplos/demo_ejecutores.yseq      # routing: demo bench + Python on loopback
./anvil ejemplos/demo_ejecutores.yseq --executor python=127.0.0.1:9200
# Validate without executing or touching hardware (CI):
./anvil ejemplos/subsecuencia.yseq --validate
# And with the executors up, also check step names, parameter names and
# outputs against what each executor says it serves (ADR-0021):
./anvil ejemplos/demo_ejecutores.yseq --validate --with-executors
```

A sequence file may end in `.yseq` as well as `.yaml` or `.yml`: it is the
same YAML, with an extension that says what the file is for, and a
`sequence_call` finds `child.yseq` without needing `./`
([ADR-0039](adr/0039-a-sequence-file-may-end-in-yseq.md)).

The console prints the textual report to **stdout** (diagnostics go to
stderr, so they do not pollute it — `anvil --version` included, for now:
[#75](https://github.com/anlaco/anvil/issues/75)). `--json`/`--csv` dump to a file.
`--process-model` wraps the sequence in a process model file (RF-38, ADR-0016;
none ships since ADR-0041 removed `process_models/sequential.yaml`);
`--validate` loads and validates without executing; `--quiet` silences the
console. There are no dependencies to install.

> **Executor routing (M5-ext.1, ADR-0013):** `ejemplos/demo_ejecutores.yseq`
> demonstrates the name→endpoint dispatch: `demo/check_led` is served by the
> demo bench (`type: wasm`, which the host starts) and `instrument/medir_simulador` /
> `instrument/conectar_equipo` by a Python executor on `127.0.0.1:9101` (start
> `simulador_tcp.py` and `server.py` from `executors/python/` in two other
> terminals). Those two carry a module prefix because a Python step is named
> `<module>/<step>` — the module being the `.py` it lives in
> ([ADR-0026](adr/0026-the-python-executor-is-a-department-too.md)). The flag
> `--executor name=host:port` re-points an executor without touching the YAML
> (the `--limits` pattern). There is no default executor: a step that calls one
> and names none is a load error.
>
> **Writing your own Python step** does not require touching the executor:
> you decorate a function with `@step` and drop the file where `--steps`
> points ([ADR-0021](adr/0021-el-ejecutor-describe-su-catalogo.md); the how,
> in [`executors/python/README.md`](../executors/python/README.md)).
>
> **Writing your steps in C#**: mark a method with `[Step]` in your own
> project, reference the SDK, and your executable listens on gRPC — the
> catalog is compiled from the signatures
> ([ADR-0038](adr/0038-the-csharp-step-sdk-is-hosted-by-the-users-own-process.md);
> the how, in [`executors/csharp/README.md`](../executors/csharp/README.md),
> with [`ejemplos/csharp.yseq`](../ejemplos/csharp.yseq) as the worked
> example).

## For developers (build from source)

### Prerequisites

- A Rust toolchain with the `wasm32-wasip2` target (`rust-toolchain.toml`
  pins it).
- No `wasmtime` needed: the host embeds it as a library. (The `wasmtime` CLI
  is only needed if you want to run the guests loose for debugging — see
  below.)
- Nothing else to clone. [`wasi-grpc`](https://github.com/anlaco/wasi-grpc),
  the gRPC stack the guests use, is a git dependency pinned to a tag
  (`Cargo.toml`, `wasi-grpc = { git = …, tag = "v0.1.1" }`); cargo fetches it.
  To develop both at once against a local checkout, override it with a
  `[patch."https://github.com/anlaco/wasi-grpc"]` section instead of editing
  the dependency. (Older instructions asked for a sibling clone; that stopped
  being true when the dependency moved to a tag, #25 — CI still carries the
  leftover, #71.)

### Building

The host embeds the engine's `.wasm` and ships **the bridge** next to it, and
the example department is assembled from the bridge and the example
components, so they are built in order. The root `Makefile` does it:

```sh
make build      # debug   → packaging/anvil-host/target/debug/anvil
make release    # release → packaging/anvil-host/target/release/anvil
```

What it does inside, if you prefer it by hand (add `--release` to all three
for the distribution binary):

```sh
# 1. WASM guest (motor) — core workspace
cargo build --target wasm32-wasip2 -p motor

# 2. gRPC↔component bridge (M5-ext.2, ADR-0015) — its own workspace
cargo build --manifest-path executors/wasm/Cargo.toml

# 3. Native host (its own workspace; wasmtime compiles here, not in the core)
cargo build --manifest-path packaging/anvil-host/Cargo.toml
```

> **Debug starts slow, and that is normal.** The debug binary takes tens of
> seconds to start because wasmtime compiles the guest unoptimized on every
> start (measured: ~26 s in debug, ~1.2 s in release).
> That is why the host's startup timeout is 60 s (`SONDEOS_ARRANQUE`). For
> anything other than debugging the host itself, use `make release`.

> The host lives in `packaging/anvil-host`, **outside** the core workspace,
> so that `cargo build` / `cargo test` on the core do not drag in wasmtime
> (ADR-0011 decision). That is why it builds with `--manifest-path` (or `cd
> packaging/anvil-host && cargo build`), not with `-p anvil-host`.

The host's `build.rs` copies the already-compiled engine `.wasm` (from the
core's `target/`) into `OUT_DIR`; if they are missing, it fails naming the
step-1 command.

### Tests (no network)

```sh
make test                  # core, bridge, host, the Python, Rust and C# step SDKs, the editor
cargo test                 # core only: modelo, cargador, expr, motor, sinks
cargo test -p motor        # sequence call with a mock (no gRPC)
```

### Trying the binary

```sh
./packaging/anvil-host/target/debug/anvil ejemplos/subsecuencia.yseq --json ./out.json --csv ./out.csv
```

Same nested/JSON/CSV report as the smoke test. The executors' logs go to
stderr; stdout stays clean for the report.

## What to look at

**On the console** (nested textual report, M4b):

```
=== basica: pass ===
  [pass] preparar: sequence call 'init_comun' → pass
    [done] preparar_canal: statement ok
  [pass] test_fuentes: sequence call 'ejemplos/medir_fuentes.yseq' → pass
    [done] ajustar_canal: statement ok
    [pass] demo/measure_voltage: measured: 4.2 V (channel 1)
    [done] demo/disconnect: instrument disconnected
```

`[done]` is a step that finished and judged nothing — here an `action`
(ADR-0040). It is neutral: it neither passes nor fails the sequence.

**`out.json`**: nested `sub_steps`; `demo/measure_voltage` with
`measured_value: 4.2`, `limit_min`/`limit_max` and its `outputs`
(`channel_used`, `temperature`).

**`out.csv`**: one row per step; flattening adds no columns of its own, and
the header ends in `phase,inputs,outputs,module,comparison,units` — the last
three added by ADR-0040, so a measurement can be reconstructed afterwards
(Rule 3 of ADR-0019): what was called, with what comparison code, in what
units.

## M4b variations (subsequences)

Edit `ejemplos/subsecuencia.yseq` and run again (no rebuild needed: the YAML
is read at runtime):

- **Signature mismatch**: add one extra parameter to the outer call →
  `secuencia inválida: el sequence call 'test_fuentes' no encaja con la
  firma…` (fail-fast at load; nothing executes).
- **Undeclared lvalue**: `parametros: { canal: locals.inventado }` →
  `…usa 'locals.inventado', no declarado en locals de su secuencia`.
- **Cycle**: `a.yaml` → `./b.yaml`, `b.yaml` → `./a.yaml` → `ciclo de
  subsecuencias: A → B → A`.

The **by-reference** case (the child mutates `parameters.canal` and the
parent picks it up) does not show up in the report — the sinks do not expose
`locals`. It is covered by the unit test:
`cargo test -p motor sequence_call_by_reference`.

## CLI usage

```
anvil <sequence.yaml> [--process-model <pm.yaml>] [--json <path>] [--csv <path>]
      [--limits <path>] [--executor name=host:port]
      [--validate [--with-executors]] [--quiet] [--help] [--version]
```

- The sequence is the first positional argument (required).
- Console unless `--quiet`; `--json`/`--csv` optional (file).
- `--process-model` wraps the sequence in a process model file (RF-38).
- `--validate` loads and validates without executing or connecting (CI with
  no hardware). It opens no ports: it does not even bring up the bridge of a
  `type: wasm` executor, though it does check the declared `.wasm` exists.
  Beyond the schema, the cycles and the subsequence signatures, it validates
  the **expressions**: reading an undeclared name, writing `file_globals`, or
  writing `parameters` from the root, are load errors. The **types** are not:
  they are undecidable without evaluating.
- `--with-executors` (only with `--validate`) adds what **does** require
  connecting: it asks each executor which steps it serves and with what
  signature ([ADR-0021](adr/0021-el-ejecutor-describe-su-catalogo.md)), and
  checks that the step exists, that its `inputs` are accepted parameters,
  that no required one is missing, that a literal matches the declared type,
  and that `assign: result.outputs.<name>` reads an output it returns. It is
  opt-in because it would break the promise of plain `--validate`, which is
  running in CI with no hardware. **On a real run, this is always checked**,
  once per executor and before the first step: a finding stops the run
  without touching the unit. An executor that cannot describe itself
  leaves its steps *unchecked*, and says so on stderr: neither error nor
  silence. WASM steps used to be that case and no longer are — since
  `anvil:step@0.4.0` a component publishes its catalog like anyone else
  ([ADR-0024](adr/0024-the-signature-is-the-catalog-in-rust-too.md)).
- `--limits` injects a limits sidecar keyed by step name (RF-30),
  overriding the embedded ones. It matches **any** sequence of the program
  —the root, the external and inline subsequences, and the operator's
  sequence under `--process-model`—, which is what makes the mechanism usable
  in production. If any sidecar name matches no step, it **warns on stderr
  and names them** — even under `--quiet`—: a limit that is not applied
  leaves the embedded one standing and produces a verdict that is not the one
  you asked for.
- `--executor name=host:port` re-points an executor declared under
  `ejecutores:` to another endpoint without touching the YAML (R&D vs.
  factory, RF-36.3); it can be repeated. If the name is not declared, a load
  error.
- Host flag: `--loopback-only` rejects any declared non-loopback `grpc`
  (CI/paranoia).
- Diagnostics go to **stderr**; stdout stays clean for the report.

## Debugging with the wasmtime CLI (advanced)

To run the engine guest **loose** (without the host), you need the `wasmtime`
CLI and two terminals. The host is what would start the demo bench's executor
and hand the engine its address, so here you do both by hand:

```sh
make release
# Terminal 1 — the demo bench's executor
ejemplos/departamento/dist/anvil-exec-wasm --port 9300
# Terminal 2 — engine, pointed at it
wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
  target/wasm32-wasip2/release/anvil-guest.wasm ejemplos/basica.yseq \
  --executor demo=127.0.0.1:9300
```

The executor in this mode **does not exit by itself**; Ctrl-C when done. It is
only for debugging the guest separately; for normal use, the `anvil` binary
(host) is the recommended path.

## Measuring against an instrument over SCPI

The SCPI step that used to be built into `anvil` was removed with the executor
that served it ([ADR-0041](adr/0041-there-is-no-embedded-executor.md) §7), and
nothing in this repo replaces it. A step that talks to an instrument needs
network access, which a WASM component does not have: write it on the
[Python](../executors/python/README.md) or [C#](../executors/csharp/README.md)
SDK.

## Writing your own step in Rust (ADR-0015, ADR-0024)

The full "hello world": write a step in Rust, compile it to `.wasm` and run it
with Anvil. **No cloning the repo, no `wasi-grpc`, no `modelo`, and nothing to
install beyond the Rust toolchain.** Official reference: `ejemplos/hola-paso/`.

1. A library crate with one dependency:

   ```toml
   # hola/Cargo.toml
   [package]
   name = "hola"
   version = "0.1.0"
   edition = "2021"

   [dependencies]
   anvil-step = "0.4"

   [lib]
   crate-type = ["cdylib"]
   ```

2. The steps. A step is an ordinary function:

   ```rust
   // hola/src/lib.rs
   use anvil_step::{step, Ctx, Outcome};

   /// Measures the voltage on a channel.
   #[step(outputs(channel_used: f64))]
   fn measure_voltage(channel: f64, scale: Option<String>) -> Outcome {
       Outcome::measured(read_instrument(channel, scale))
           .output("channel_used", channel)
   }

   /// Checks the LED is lit.
   #[step]
   fn check_led() -> Outcome {
       Outcome::passed("led lit")
   }

   anvil_step::export!();
   ```

   **The signature is the catalog.** The name of each parameter, its type and
   whether it is required come from the function itself: they are not written
   twice, so they cannot drift. That is what lets Anvil check a sequence
   **without running it** (`--validate --with-executors`) and tell you that you
   wrote `channell` instead of `channel` before the unit is on the bench.

   A parameter is `f64`, `String` or `bool`, or an `Option` of one of them —
   which is how it is declared optional. Anything else does not compile: a
   parameter that needs structure is a badly cut step (ADR-0020 §2). What the
   signature cannot say, the attribute takes: `outputs(name: Type)` for the
   named outputs a sequence reads as `result.outputs.<name>`, and
   `name = "..."` when the step's name in the sequence is not a valid Rust
   identifier.

   A step that wants the attempt number takes a `ctx: Ctx` first, and only if
   it asks for one. `ctx` is never part of the described signature: it is the
   executor talking to the step, not a value out of the sequence.

3. What a step gives back. `Outcome::measured(v)` for a measurement,
   `Outcome::passed(…)` / `Outcome::failed(…)` for a pass/fail,
   `Outcome::error(…)` when it could not judge. Shortcuts also work: returning
   a number is a measurement, a `bool` is pass/fail, `()` is a pass with no
   measurement, and a `Result` whose `Err` is a bench problem comes out as
   `error`.

   **The threshold is not the step's business**: return the measurement and let
   the engine judge it against the sequence's `limit` (ADR-0008). And the
   distinction that matters most is between `fail` and `error`: **`fail` is the
   unit's** ("I measured and it does not comply"), **`error` is the bench's or
   the step's** ("I could not measure"). A step that blew up is never a failed
   unit (ADR-0019, Rule 2).

   Prefer `Result` over `panic!` or `unwrap()`. A panic no longer takes the
   run with it (#58), but the step comes back as `error` with a WebAssembly
   backtrace for a message, where a `Result` says what actually went wrong —
   see *Known limitations* below.

4. Compile it to a component:

   ```sh
   cargo build --target wasm32-wasip2
   # → target/wasm32-wasip2/debug/hola.wasm
   ```

   The SDK carries the WIT and generates the bindings, so there is no `wit/`
   directory in your project, no generated `bindings.rs` to keep, and no
   `cargo component` to install.

5. **Assemble a department**: a folder with the `anvil-exec-wasm` binary and
   your `.wasm` beside it. That is what an executor is — it serves the modules
   it finds next to itself
   ([ADR-0027](adr/0027-a-sequence-names-the-executor-not-the-module.md)).

   ```sh
   mkdir -p mi-departamento
   cp anvil-exec-wasm mi-departamento/
   cp target/wasm32-wasip2/debug/hola.wasm mi-departamento/
   ```

6. Declare **the executor** in the YAML — its binary, not your module — and
   name the step `<module>/<step>`:

   ```yaml
   executors:
     - { name: instrumentos, type: wasm, path: mi-departamento/anvil-exec-wasm }
   main:
     - name: hola/medir_voltaje
       executor: instrumentos
   ```

   Then `./anvil sequence.yaml`. The host spawns that binary, which loads your
   component (empty WASI sandbox: no files, no network) and translates
   gRPC↔function. If the executor is on another machine, declare it as
   `type: grpc` with its host and port instead — the steps do not change.

### One executor, several modules (ADR-0025)

One executor serves every `*.wasm` it finds beside its own binary, and each is
a module named after its file. What a step calls is then `<module>/<step>`,
and it goes in `module:` — `name:` is just the label in the report
([ADR-0040](adr/0040-a-step-type-says-how-a-step-is-judged-not-what-it-calls.md)):

```yaml
executors:
  - name: instrumentos
    type: wasm
    path: departamento/dist/anvil-exec-wasm
main:
  - name: Rail voltage
    type: numeric_limit
    module: multimetro/medir_voltaje
    executor: instrumentos
    limit: { comparison: GELE, low: 4.5, high: 5.5, units: V }
  - name: PLC rail voltage         # same step name, different instrument
    type: numeric_limit
    module: plc/medir_voltaje
    executor: instrumentos
    limit: { comparison: GELE, low: 23.0, high: 25.0, units: V }
```

The extension and the module's location never appear in the sequence, so a
department can reorganise itself — or rewrite a module in another language —
without editing any YAML. `ejemplos/demo_departamento.yseq` and
`ejemplos/demo_wasm.yseq` are the worked examples, and `make build` assembles
the department they use.

To see what a department serves without writing a sequence first, ask its
executor directly — it needs no arguments, because it knows where its modules
are:

```sh
./mi-departamento/anvil-exec-wasm --list
```

Your steps are ordinary functions, so their own unit tests call them directly:
`cargo test` needs no WASM and no Anvil — outside `wasm32`, `export!()` expands
to nothing.

### Known limitations

- **A `panic!` in a step ends the phase, not the run.** It used to cut the
  whole run ([#58](https://github.com/anlaco/anvil/issues/58), fixed after
  0.4.0). Run against 0.5.0 on 2026-09-13, a step that panics comes back as
  `error` with the WebAssembly trap in its message, the rest of `main` is
  skipped as after any error, and `cleanup` still runs. The message is a
  twenty-line backtrace, though, so return an `Outcome::error` or a `Result`
  that says why.
- **No object references** (ADR-0022 §8): a component is a function with no
  state between calls, so it cannot hold an open instrument session. A step
  that needs one is served from a `grpc` executor of its own process, such as
  the Python one.

See [ADR-0015](adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md)
and [ADR-0024](adr/0024-the-signature-is-the-catalog-in-rust-too.md).

## Continuous integration

`.github/workflows/ci.yml` runs on every push to `main` and on every PR. Its
`ci` job (Linux) does `make check` (fmt + clippy for the Rust workspaces), the
core tests, the Rust step SDK tests, the C# step SDK tests and
`dotnet format`, `make release`, the host tests and the beta regression
(informational while defects remain open). A second job, `ci-windows`,
verifies the same build and the Sequence Editor on Windows — see below.

**A leftover worth knowing about.** Both jobs still check `wasi-grpc` out next
to this repo with a read-only deploy key (`WASI_GRPC_DEPLOY_KEY`), from when
the dependency was a relative path and the repository was private. Neither is
true any more: `wasi-grpc` is public, and the workspace takes it by git tag,
so cargo fetches it and the sibling checkout is not used at all — a `ref:` on
that checkout changes nothing. Removing it is
[#71](https://github.com/anlaco/anvil/issues/71).

**Releases** are built by `.github/workflows/release.yml`, each download on
the platform it is for: the Linux engine (`packaging/package.sh`, musl), the
Windows engine (`packaging/package.ps1`, static CRT) and the Sequence Editor
as an AppImage, a `.deb` and an NSIS installer. Every engine package is
smoke-run before upload. Pushing a `release/**` branch builds the artifacts
only; running the workflow by hand with a version that matches the manifests
creates a **draft** Release, with one `SHA256SUMS` and that version's
CHANGELOG section as notes. Nothing publishes by itself: a person reads the
draft and publishes it, which triggers `verifica-release.yml` — it downloads
the Linux tarball and runs it in clean Debian and Alpine containers.

**`ci-windows`** (ADR-0036) runs the same idea on `windows-latest`, compiled
natively rather than cross-compiled — the runner already carries MSVC Build
Tools, which resolves the same `zstd-sys` dependency Linux needs `musl-gcc`
for. Besides `make release` and the host/bridge tests, it smoke-tests things
the Linux job has no reason to: that `anvil.exe --bridge` actually opens its
loopback relay (a real WebSocket handshake against the printed port, not
just "the process is alive"), that the editor's Vite dev server starts under
Git Bash, and that the Sequence Editor's Electron installer builds and its
packaged binary starts (ADR-0037). It skips `make check` (same source the Linux job
already lints) and the beta regression script (Linux-only shell, and it
regresses named defects already covered there).

## Troubleshooting

- **`Missing artifact '…'`** while building the host → `make build` (or
  `make release`) from the root, which chains the three steps in order. The
  host's build script looks for the bridge both under
  `executors/wasm/target/<profile>/` and under
  `executors/wasm/target/<target-triple>/<profile>/`, so a `--target` build
  of the bridge is found too.
- **`no se pudo cargar la secuencia`** → the YAML path does not exist or is
  not accessible. Absolute paths work — for the sequence, `--json` and
  `--csv` — since #40 (checked against 0.5.0: a sequence and both reports
  outside the current directory); an `os error 44` points at a directory
  that does not exist.
- **`usa 'resultado.valor_medido' en 'precondicion', donde no está
  disponible`** → `resultado.*` only lives inside the step's own `asigna`:
  a precondition is evaluated *before* invoking it, so there is no result to
  read. Dump the measurement into a local with `asigna` and read it from
  there (see [variables-y-alcances.md](diseno/variables-y-alcances.md)).
- **`el ejecutor '…' (…) no empezó a escuchar en …`** → a `type: wasm`
  executor did not come up; the concrete error goes to stderr. If it is slow
  but does not fail, it is the slow debug startup (see above): use
  `make release`.
- **`could not connect to the step executors: executor '…' at …`** → a
  declared `grpc` executor is not listening at that address. Start it, or
  re-point it with `--executor`.
- **`step '…' of sequence '…' calls an executor and names none`** → add
  `executor: <name>` to the step; the message lists the executors the root
  sequence declares. There is no executor built into `anvil` to fall back on.
- **`address in use` with two `anvil` processes at once** → should not
  happen: the host starts each `type: wasm` executor on an **ephemeral** port
  per process, so you can launch N `anvil` in parallel (#15).
- **`el ejecutor '…' es 'wasm' y su 'path' '…' no existe`** → a `type: wasm`
  executor's `path:` names a file that is not there. `path` is **the
  executor's binary** — a department's `anvil-exec-wasm` — relative to the
  YAML file, not to where you launch `anvil` from (ADR-0027). The package
  ships one assembled in `ejemplos/departamento/dist/`.
- **`el ejecutor '…' declara 'path: ….wasm', que es un módulo '.wasm'`** →
  `path` points at a module instead of at the executor. Point it at the
  `anvil-exec-wasm` in the folder where the `.wasm` modules are; the executor
  finds them beside itself.

## Next reading

- [roadmap.md](roadmap.md) — what is done (M0→M4b + M5-ext.1/2) and what
  remains (LID postponed).
- [diseno/formato-de-secuencia.md](diseno/formato-de-secuencia.md) — the full
  YAML schema.
- [adr/0011-distribucion-un-binario-hospeda-wasmtime.md](adr/0011-distribucion-un-binario-hospeda-wasmtime.md)
  — why one binary hosts wasmtime.
- [adr/0013-cargador-wasm-host-side-y-routing.md](adr/0013-cargador-wasm-host-side-y-routing.md)
  — the name→endpoint routing and the host-side `.wasm` loader.
- [adr/0014-cargador-wasm-host-side-m5-ext2.md](adr/0014-cargador-wasm-host-side-m5-ext2.md)
  — the `.wasm` loader by path (M5-ext.2, implemented).