# Quick-start guide

Anvil is **one binary**: you download it and run it. Inside, it hosts
`wasmtime` and the two WASM guests (engine + executor) in a sandbox,
speaking gRPC over loopback. You need no `wasmtime` install nor any runtime
— it is embedded. See [ADR-0011](adr/0011-distribucion-un-binario-hospeda-wasmtime.md)
for the why.

## For the end user

Download the `anvil` binary and run:

```sh
./anvil <sequence.yaml> [--process-model <pm.yaml>] [--json <path>] \
  [--csv <path>] [--limits <path>] [--executor name=host:port] \
  [--port <n>] [--validate [--with-executors]] [--quiet]
```

Examples (the repo's own are in `ejemplos/` and `process_models/`):

```sh
./anvil ejemplos/subsecuencia.yaml --json ./out.json --csv ./out.csv
./anvil ejemplos/basica.yaml
./anvil ejemplos/limites.yaml
./anvil ejemplos/variables.yaml
./anvil ejemplos/basica.yaml --limits ejemplos/limites.limits.yaml
./anvil ejemplos/demo_ejecutores.yaml      # routing: embedded + Python on loopback
./anvil ejemplos/demo_ejecutores.yaml --executor python=127.0.0.1:9200
# With a process model (identifies the UUT, runs the sequence, notifies):
./anvil ejemplos/basica.yaml --process-model process_models/sequential.yaml
# Validate without executing or touching hardware (CI):
./anvil ejemplos/subsecuencia.yaml --validate
# And with the executors up, also check step names, parameter names and
# outputs against what each executor says it serves (ADR-0021):
./anvil ejemplos/demo_ejecutores.yaml --validate --with-executors
```

The console prints the textual report to **stdout** (diagnostics go to
stderr, so they do not pollute it). `--json`/`--csv` dump to a file.
`--process-model` wraps the sequence in a Sequential PM (RF-38, ADR-0016);
`--validate` loads and validates without executing; `--quiet` silences the
console. There are no dependencies to install.

> **Executor routing (M5-ext.1, ADR-0013):** `ejemplos/demo_ejecutores.yaml`
> demonstrates the name→endpoint dispatch: `verificar_led` is served by the
> embedded executor (default) and `medir_simulador`/`conectar_equipo` by a
> Python executor on `127.0.0.1:9101` (start `simulador_tcp.py` and
> `server.py` from `executors/python/` in two other terminals). The flag
> `--executor name=host:port` re-points an executor without touching the YAML
> (the `--limits` pattern). With no `executors:` declared, everything goes to
> the embedded executor.
>
> **Writing your own Python step** does not require touching the executor:
> you decorate a function with `@step` and drop the file where `--steps`
> points ([ADR-0021](adr/0021-el-ejecutor-describe-su-catalogo.md); the how,
> in [`executors/python/README.md`](../executors/python/README.md)).

## For developers (build from source)

### Prerequisites

- A Rust toolchain with the `wasm32-wasip2` target (`rust-toolchain.toml`
  pins it).
- No `wasmtime` needed: the host embeds it as a library. (The `wasmtime` CLI
  is only needed if you want to run the guests loose for debugging — see
  below.)
- **The sibling repo [`wasi-grpc`](https://github.com/anlaco/wasi-grpc)
  cloned next to** this one: `motor` and `ejecutor_pasos` reference it with
  `path = "../wasi-grpc"` (dogfooding, see `Cargo.toml`). Without it cargo
  **does not even read the workspace** — it fails before compiling anything,
  with a `failed to load manifest for workspace member`. The expected layout
  is:

  ```
  ..../
    anvil/
    wasi-grpc/
  ```

### Building

The host embeds the two `.wasm` files **and the bridge**, so all three are
built in order. The root `Makefile` does it:

```sh
make build      # debug   → packaging/anvil-host/target/debug/anvil
make release    # release → packaging/anvil-host/target/release/anvil
```

What it does inside, if you prefer it by hand (add `--release` to all three
for the distribution binary):

```sh
# 1. WASM guests (motor + ejecutor) — core workspace
cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos

# 2. gRPC↔component bridge (M5-ext.2, ADR-0015) — its own workspace
cargo build --manifest-path packaging/anvil-puente-wasm/Cargo.toml

# 3. Native host (its own workspace; wasmtime compiles here, not in the core)
cargo build --manifest-path packaging/anvil-host/Cargo.toml
```

> **Debug starts slow, and that is normal.** The debug binary takes tens of
> seconds to bring the executor up because wasmtime compiles the guests
> unoptimized on every start (measured: ~26 s in debug, ~1.2 s in release).
> That is why the host's startup timeout is 60 s (`SONDEOS_ARRANQUE`). For
> anything other than debugging the host itself, use `make release`.

> The host lives in `packaging/anvil-host`, **outside** the core workspace,
> so that `cargo build` / `cargo test` on the core do not drag in wasmtime
> (ADR-0011 decision). That is why it builds with `--manifest-path` (or `cd
> packaging/anvil-host && cargo build`), not with `-p anvil-host`.

The host's `build.rs` copies the already-compiled `.wasm` files (from the
core's `target/`) into `OUT_DIR`; if they are missing, it fails naming the
step-1 command.

### Tests (no network)

```sh
make test                  # 201 core + 7 host
cargo test                 # core only: modelo, cargador, expr, motor, sinks
cargo test -p motor        # sequence call with a mock (no gRPC)
```

### Trying the binary

```sh
./packaging/anvil-host/target/debug/anvil ejemplos/subsecuencia.yaml --json ./out.json --csv ./out.csv
```

Same nested/JSON/CSV report as the smoke test. The executor's logs
("paso pedido: …") go to stderr; stdout stays clean for the report.

## What to look at

**On the console** (nested textual report, M4b):

```
=== basica: paso ===
  [paso] preparar: sequence call 'init_comun' → paso
    [paso] preparar_canal: statement ok
  [paso] test_fuentes: sequence call 'ejemplos/medir_fuentes.yaml' → paso
    [paso] ajustar_canal: statement ok
    [paso] medir_voltaje: medido: 4.2 V
    [paso] desconectar_equipo: equipo desconectado
```

**`out.json`**: nested `sub_pasos`; `medir_voltaje` with `valor_medido: 4.2`
and `limite_min/max`.

**`out.csv`**: flattened rows `test_fuentes/medir_voltaje`; flattening adds
no columns of its own, and the last header column is `fase`
(`setup`/`main`/`cleanup`).

## M4b variations (subsequences)

Edit `ejemplos/subsecuencia.yaml` and run again (no rebuild needed: the YAML
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
      [--limits <path>] [--executor name=host:port] [--port <n>]
      [--validate [--with-executors]] [--quiet] [--help] [--version]
```

- The sequence is the first positional argument (required).
- Console unless `--quiet`; `--json`/`--csv` optional (file).
- `--process-model` wraps the sequence in a Sequential PM (RF-38).
- `--validate` loads and validates without executing or connecting (CI with
  no hardware). It opens no ports: it does not even bring up the bridge of a
  `tipo: wasm` executor, though it does check the declared `.wasm` exists.
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
  without touching the unit. An executor that cannot describe itself —today,
  the bridge of a `.wasm` step— leaves its steps *unchecked*, and says so on
  stderr: neither error nor silence.
- `--port` fixes the port of the embedded executor — both the executor's and
  the one the engine looks for. Without it, the host takes an **ephemeral**
  port per process, so several `anvil` processes can run at once (#15).
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

To run the guests **loose** (without the host), you need the `wasmtime` CLI
and two terminals:

```sh
cargo build --target wasm32-wasip2 -p ejecutor_pasos -p motor
# Terminal 1 — executor (gRPC on 127.0.0.1:9100)
wasmtime -S cli -S tcp=y -S inherit-network=y \
  target/wasm32-wasip2/debug/ejecutor_pasos.wasm
# Terminal 2 — engine
wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
  target/wasm32-wasip2/debug/anvil-guest.wasm ejemplos/subsecuencia.yaml
```

The executor in this mode **does not exit by itself** (accept loop); Ctrl-C
when done. It is only for debugging the guests separately; for normal use,
the `anvil` binary (host) is the recommended path.

## Measuring against an instrument that does not exist

`ejemplos/scpi.yaml` needs no Keithley on the bench. The step
`medir_voltaje_scpi` opens a socket to `ANVIL_SCPI_ADDR` (default
`127.0.0.1:5025`) and sends `MEASURE:VOLTAGE?`; whatever answers on the other
side does not matter as long as it speaks SCPI.
[Crucible](https://github.com/anlaco/Crucible) serves exactly that from a
YAML.

With the Crucible repo cloned alongside and `cargo build` done there, one
more terminal:

```sh
# Terminal 3 — the digital twin, on 5025
./target/debug/crucible perfiles/keithley_2400_demo.yaml
```

And the engine against the SCPI sequence instead of the subsequence one:

```sh
wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
  target/wasm32-wasip2/debug/anvil-guest.wasm ejemplos/scpi.yaml
```

```
=== scpi_demo: paso ===
  [paso] medir_voltaje_scpi: SCPI medido: 4.501385029307777 V
```

Verified on 2026-08-12. It uses the `_demo` profile, not the reference one:
the reference profile starts at rest —`output: false`, like an instrument
just switched on— and since this step measures without configuring anything
first, the answer would be `0.0` and the 4.0–5.0 limit would fail it. The
decimals vary on every run: Crucible's measurement model adds Gaussian
noise.

## Writing your own step in Rust (M5-ext.2, ADR-0015)

The full "hello world": write a step in Rust, compile it to `.wasm` and run
it with Anvil. **No cloning the repo, no `wasi-grpc`, no `modelo`.**
Official reference: `ejemplos/hola-paso/`.

1. Install the component tooling (once):
   ```sh
   cargo install cargo-component --locked
   ```
2. The step project (`hola/Cargo.toml` with `[lib] crate-type = ["cdylib"]`,
   `hola/wit/anvil-paso.wit`, `hola/src/lib.rs`). The WIT is the contract:
   ```wit
   package anvil:paso@0.1.0;
   interface paso {
     record resultado {
       // Uno de "paso" | "fallo" | "error" | "saltado", en minúscula.
       estado: string,
       mensaje: string,
       valor-medido: option<f64>,
     }
     run: func(nombre: string, intento: s32) -> resultado;
   }
   world anvil-paso { export paso; }
   ```

   **`estado` is text, but the vocabulary is closed**: exactly one of
   `"paso"`, `"fallo"`, `"error"` or `"saltado"`. Any other string
   —`"Paso"` with a capital letter is the real case that motivated issue
   #28— turns the step into `error`, with a message naming the value you
   returned. Anvil does not judge a unit with a status it does not
   understand (ADR-0019, Rule 2). The distinction that matters most is the
   middle one: **`fallo` is the unit's** ("I measured and it does not
   comply"), **`error` is the bench's or the step's** ("I could not
   measure").
3. The implementation is a function (~15 lines, with `wit-bindgen`):
   ```rust
   #[allow(warnings)]
   mod bindings;
   use bindings::exports::anvil::paso::paso::{Guest, Resultado};
   struct Component;
   impl Guest for Component {
       fn run(nombre: String, intento: i32) -> Resultado {
           Resultado {
               estado: "paso".to_string(),
               mensaje: format!("hola {nombre} (intento {intento})"),
               valor_medido: Some(4.2),
           }
       }
   }
   bindings::export!(Component with_types_in bindings);
   ```
4. Compile to a component:
   `cargo component build` → `target/wasm32-wasip1/debug/hola.wasm`.
5. Declare it in the YAML (`ejecutores: [{ nombre: hola, tipo: wasm, path:
   ./hola.wasm }]`, a step with `ejecutor: hola`) and run
   `./anvil sequence.yaml`. The host spawns the bridge, which loads your
   component (empty WASI sandbox: no files, no network) and translates
   gRPC↔function.

See [ADR-0015](adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md).

## Continuous integration

`.github/workflows/ci.yml` runs on every push to `main` and on every PR:
`make check` (fmt + clippy for the three workspaces), the core tests, `make
release`, the host tests and the beta regression (informational while defects
remain open).

The only non-obvious part is how CI gets the sibling repo. The workflow uses
a **read-only deploy key** (the public one installed on `wasi-grpc` as
"anvil CI", the private one in this repo's `WASI_GRPC_DEPLOY_KEY` secret),
which was needed when `wasi-grpc` was private: the `GITHUB_TOKEN` GitHub
gives each job only covers the repo itself.

**`wasi-grpc` is public now**, so the key is redundant and a plain checkout
would do. It stays because it works and removing it is touching CI for
nothing; when it gets touched for another reason, it gets simplified.

The job reproduces the sibling layout with two checkouts (`path: anvil` and
`path: wasi-grpc`), because the dependency is by relative path.

**To rotate it:**

```sh
ssh-keygen -t ed25519 -N "" -C "anvil CI" -f /tmp/k
gh repo deploy-key add /tmp/k.pub --repo anlaco/wasi-grpc --title "anvil CI"
gh secret set WASI_GRPC_DEPLOY_KEY --repo anlaco/anvil < /tmp/k
gh repo deploy-key delete <old-id> --repo anlaco/wasi-grpc
shred -u /tmp/k /tmp/k.pub
```

When `wasi-grpc` stabilizes and gets published, the dependency becomes a
version and all of this goes away.

## Troubleshooting

- **`failed to load manifest for workspace member 'crates/ejecutor_pasos'`**
  → the sibling repo `wasi-grpc` is missing next to this one (see
  Prerequisites).
- **`Falta el artifact '…'`** while building the host → `make build` (or
  `make release`) from the root, which chains the three steps in order.
- **`no se pudo cargar la secuencia`** → the YAML path does not exist or is
  not accessible (the host preopens the current directory). For the same
  reason, `--json` and `--csv` can only write **inside the current
  directory**: an absolute path elsewhere gives `No such file or directory
  (os error 44)`.
- **`usa 'resultado.valor_medido' en 'precondicion', donde no está
  disponible`** → `resultado.*` only lives inside the step's own `asigna`:
  a precondition is evaluated *before* invoking it, so there is no result to
  read. Dump the measurement into a local with `asigna` and read it from
  there (see [variables-y-alcances.md](diseno/variables-y-alcances.md)).
- **`el ejecutor de pasos no empezó a escuchar`** → the executor guest failed
  to start; the concrete error goes to stderr. If it is slow but does not
  fail, it is the slow debug startup (see above): use `make release`.
- **`address in use` with two `anvil` processes at once** → should no longer
  happen: since #15 the embedded executor takes an **ephemeral** port per
  process, so you can launch N `anvil` in parallel. If you pin `--port`, that
  port serves the executor **and** the engine, so two processes with the same
  `--port` do collide — that is what you asked for.

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