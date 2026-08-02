# ADR-0011: Distribución — un binario hospeda wasmtime y los dos guests WASM

- **Estado:** Aceptada
- **Fecha:** 2026-08-02 (M5/packaging)
- **Relaciona:** ADR-0001, ADR-0003, ADR-0006,
  [arquitectura.md](../arquitectura.md),
  [guia-inicio-rapido.md](../guia-inicio-rapido.md)

## Contexto

Hasta M4b, Anvil se distribuía como **dos `.wasm** ` (`anvil.wasm` y
`ejecutor_pasos.wasm`) que el usuario corría con el **CLI de wasmtime**
instalado aparte, con flags obligatorios (`-S cli -S tcp=y -S
inherit-network=y --dir=.`). Para el usuario final eso son dos dependencias:
instalar wasmtime + bajar los `.wasm`.

El objetivo de producto es **descargar una sola aplicación** y correrla, sin
instalar nada. A la vez, se quiere **mantener WASM** (sandbox + JIT, lo que
justifica ADR-0001) y **motor y ejecutor separados** (aislados entre sí,
ADR-0003).

## Decisión

Un **único binario nativo** hospeda **wasmtime como librería** (no el CLI) y
orchesta los **dos guests WASM** (motor + ejecutor) en el mismo proceso:

- El crate `packaging/anvil-host` depende de `wasmtime` + `wasmtime-wasi`
  (enlazado estático). El usuario **no instala wasmtime**. Es un **workspace
  aparte** del core: no figura en el `members` del `Cargo.toml` raíz, para
  que `cargo build` / `cargo test` del core no arrastren wasmtime (que es
  pesado de compilar). El host se compila por separado: `cargo build
  --manifest-path packaging/anvil-host/Cargo.toml`.
- Los dos `.wasm` se **embeben** en el binario (`include_bytes!`) →
  literalmente un fichero.
- Cada guest corre como comando WASI P2 (`wasi:cli/run`) en su propio
  `Store` (sandbox independiente). wasmtime es bloqueante, así que el host
  los corre en **threads** separados: el ejecutor (bind `127.0.0.1:9100`) y
  el motor (conecta, corre la secuencia, sale).
- Los dos guests se hablan por **gRPC sobre loopback TCP dentro del
  proceso**: `wasi:sockets` restringidos a loopback
  (`socket_addr_check → is_loopback()`). El ejecutor bindea `9100`
  (hardcodeado en el `.wasm`, no se toca); el motor conecta. No hace falta
  tocar `wasi-grpc`.
- El host expone el **mismo CLI** que el `anvil.wasm` actual:
  `anvil <secuencia.yaml> [--json <ruta>] [--csv <ruta>] [--limits <ruta>]`,
  pasado al guest motor como args; un `preopened_dir` (el cwd) para que lea
  el YAML y escriba las salidas.

El bin WASM `anvil` (en `crates/motor`) **se conserva** para el path de
desarrollo/depuración con wasmtime CLI. El host nativo se llama también
`anvil` (artefacto `target/release/anvil`); conviven en targets distintos.

## Por qué esta forma

- **Mantiene el sandbox y el JIT** de WASM (ADR-0001): los guests siguen
  aislados del host y entre sí. No se pasa a binarios nativos.
- **Mantiene el adapter gRPC y `wasi-grpc`** (ADR-0006): no se reescribe
  nada; los guests ya hablan `wasi:sockets`/HTTP-2.
- **Mantiene motor y ejecutor separados** (ADR-0003): dos `Store`s
  independientes, aislamiento preservado.
- **Una sola aplicación**: el usuario descarga un binario y corre
  `./anvil secuencia.yaml`. wasmtime va dentro.

## Recortes y compromisos

- **Loopback TCP, no in-memory.** Hoy los dos guests talk por `127.0.0.1:
  9100` real (restringido a loopback). Un transporte **in-memory** entre los
  dos `Store`s (sin puerto del host, ideal en seguridad) exigiría
  reimplementar el WIT de `wasi:sockets` en el host o añadir un trait
  `Transport` a `wasi-grpc` (que hoy no lo tiene — sus `Cliente`/`Servidor`
  se construyen sólo desde `TcpSocket`/`InputStream`/`OutputStream` de
  WASI). Queda **post-MVP**; el loopback restringido es el compromiso
  correcto para el MVP.
- **Embeber, no acompañar.** Los `.wasm` van dentro del binario: para
  actualizar el ejecutor hay que recompilar el host. Aceptado (más limpio
  para el usuario). Si se quisiera actualizar el ejecutor sin recompilar,
  se acompañarían los `.wasm` (post-MVP).
- **Sin cross-compile/release automatizado.** Este ADR fija la arquitectura;
  el empaquetado por SO (tarballs, firmas) es trabajo posterior.

## Consecuencias

- ADR-0001 (Rust+WASM) se **refuerza**: el sandbox WASM sigue, ahora oculto
  tras un binario nativo.
- El CLI de wasmtime deja de ser una dependencia del usuario final; pasa a
  ser una herramienta de desarrollo (depuración de guests sueltos).
- El binario del host es mayor (~54 MB en release) por wasmtime enlazado;
  aceptable para una aplicación de escritorio.
- **Aislamiento del build**: `packaging/anvil-host` es un workspace aparte
  para que el core (`cargo build` / `cargo test`) no compile wasmtime. El
  coste de wasmtime (build lento) queda confinado al empaquetado, no al día
  a día del contribuidor.
- La guía de inicio rápido pasa a "descarga el binario, corre
  `./anvil secuencia.yaml`".