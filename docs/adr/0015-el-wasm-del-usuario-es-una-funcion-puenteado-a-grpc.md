# ADR-0015: El `.wasm` del usuario es una función (componente WIT), el host lo puentea a gRPC

- **Estado:** Aceptada
- **Fecha:** 2026-08-04 (M5-ext.2, rework)
- **Reemplaza:** ADR-0014 en la parte del `.wasm` del usuario (la convención
  `ANVIL_PORT` y "el `.wasm` es un servidor gRPC"). El resto de ADR-0014 —
  el host como cargador, puerto efímero, deduplicación por path, overrides
  `--ejecutor` sintéticos — se mantiene.
- **Relaciona:** ADR-0001, ADR-0005, ADR-0011, ADR-0013, ADR-0014
  (superseded en el contrato del `.wasm`),
  [arquitectura.md](../arquitectura.md),
  [contrato-grpc.md](../contrato-grpc.md),
  [diseno/executores-lenguaje.md](../diseno/executores-lenguaje.md),
  [planes/m5-ext.md](../planes/m5-ext.md)

## Contexto

ADR-0014 decidió que el `.wasm` del usuario es **él mismo un servidor gRPC**:
bind a `ANVIL_PORT`, aceptar conexiones, decodificar `PeticionPaso`,
despachar, responder `ResultadoPasoProto`. Para eso, el `.wasm` tiene que
**linkar** `wasi-grpc` y `modelo` al compilar.

Dos problemas:

1. **No cumple la tesis "agnóstico al origen del `.wasm`"** (ADR-0013). Un
   paso de test no debería saber de gRPC ni de protobuf: es una función de
   medición. Pedirle a su autor que programme un servidor de red dentro de
   su `.wasm` es pedirle infraestructura, no una prueba. Y encima necesita
   dos crates privadas del monorepo (`wasi-grpc` es un repo privado), así
   que un usuario externo **no puede compilar su paso** sin clonar el repo.
2. **El "hola mundo" de la guía es imposible**: escribir Rust, compilar a
   `.wasm`, ejecutar con el binario `anvil` — sin clonar repos ni linkar
   libs privadas.

A la vez, el usuario (dueño del producto) confirmó la arquitectura del
puente: un binario standalone que traduce gRPC↔función, embebido en `anvil`
para el hola mundo (opción A) y distribuible suelto para el caso remoto
(Raspberry Pi), que se clona tal cual del interno.

## Decisión

### 1. El `.wasm` del usuario es un componente WASM que exporta una función `run`

El contrato del `.wasm` de paso deja de ser `paso.proto` por gRPC. Es una
**interfaz WIT** mínima, `anvil:paso@0.1.0` (en
`packaging/anvil-puente-wasm/wit/anvil-paso.wit`):

```wit
interface paso {
  record resultado {
    estado: string,        // "paso" | "fallo" | "error"
    mensaje: string,
    valor-medido: option<f64>,
  }
  run: func(nombre: string, intento: s32) -> resultado;
}
world anvil-paso { export paso; }
```

El autor del paso compila con `cargo component` (instalable con
`cargo install cargo-component --locked`) y `wit-bindgen` (público, en
crates.io). Su `.wasm` **no sabe de gRPC ni de protobuf ni de Anvil**: es
una función Rust de ~15 líneas. Sin `wasi-grpc`, sin `modelo`, sin
`ANVIL_PORT`, sin clonar el repo.

### 2. El puente `anvil-puente-wasm` traduce gRPC↔función

Nuevo binario nativo (`packaging/anvil-puente-wasm`, workspace aparte como
`anvil-host`):

- Linka **wasmtime** (component API, carga el `.wasm` y llama `run`),
  **wit-bindgen** (bindings host del WIT) y **tonic** (servidor gRPC
  nativo). El puente es código **nativo**: puede usar tonic sin
  restricciones; `wasi-grpc` queda sólo para los guests WASM de Anvil
  (ejecutor embebido), donde tonic/tokio no compilan.
- CLI: `anvil-puente-wasm --wasm <ruta.wasm> [--port <puerto>] [--bind <ip>]`.
  `--bind 0.0.0.0` habilita el caso remoto (Raspberry Pi) — el mismo
  binario, sin tocar nada.
- Instancia el componente **una vez** al arrancar (preload, como TestStand)
  en un Store con sandbox WASI **vacío** (sin preopens ni red: el
  componente es una función pura; no toca el host). 1 Store, N llamadas.
- Por cada `Invoca` del motor: llama `run(nombre, intento)`, traduce el
  `resultado` a `ResultadoPasoProto` (paso.proto no cambia, RNF-05) y
  responde. Un pánico del guest → `Status::internal` → el motor lo ve como
  error del paso, no corta la secuencia por red.
- Sale solo si el host muere: stdin en pipe desde el host; EOF → exit.

### 3. `anvil-host` spawnea el puente para cada `tipo: wasm`

- El puente va **embebido** en el binario `anvil` (`include_bytes!` +
  `build.rs` lo copia a `OUT_DIR`, como los guests WASM). Al arrancar se
  extrae a temp (con hash del contenido: una versión del binario = un
  fichero) y se spawnea con `--wasm <path> --port <efímero>`.
- El puerto efímero lo reserva el host (`bind 127.0.0.1:0`), igual que en
  ADR-0014. El resto del flujo no cambia: deduplicación por path (dos
  ejecutores con el mismo `.wasm` → un puente), readiness por polling,
  overrides `--ejecutor` sintéticos para el motor (que sólo ve `grpc`).
- El ejecutor embebido (`ejecutor_pasos.wasm`) **no cambia**: sigue siendo
  gRPC-en-WASM con `wasi-grpc`, sigue usando `ANVIL_PORT` (default 9100,
  útil para depurar con `wasmtime run` suelto). Es código de Anvil; linkar
  lo propio es legítimo ahí.

## Por qué esta forma

- **Hace verdad la tesis**: "si habla `paso.proto` por gRPC en loopback,
  Anvil lo atiende" era la condición de ADR-0013 para el `.wasm` de paso.
  Con esto, la condición se simplifica: "si exporta `run` (WIT `anvil:paso`)
  y se compila a componente WASM, Anvil lo atiende". El autor del paso no
  aprende nada de Anvil: escribe una función.
- **El hola mundo de la guía es posible**: Rust + `wit-bindgen` (público),
  `cargo component build`, YAML, `./anvil secuencia.yaml`. Sin repos
  privados.
- **Uniformidad conservada**: el motor sigue hablando gRPC con todo (el
  puente es "un grpc más en loopback" para él). Nada del motor, del
  cargador ni de `paso.proto` cambia.
- **Aislamiento real**: el componente del usuario corre en un Store con
  sandbox WASI vacío (sin ficheros ni red). Antes, el `.wasm` era un
  servidor gRPC con acceso al loopback; ahora es una función pura sin
  acceso a nada — el aislamiento mejora.
- **Un solo binario para el hola mundo**: el puente embebido en `anvil` y
  extraído a temp. El caso remoto (Pi) usa el mismo binario suelto con
  `--bind 0.0.0.0` — se "clona" el puente interno sin tocar nada, como
  pidió el usuario.

## Recortes y compromisos

- **No hay AOT** a `.cwasm` ni `StoreLimitsBuilder` (post-M5-ext.2, si la
  medición de 50+ Stores lo pide).
- **Un puente por path** (no un puente compartido por N paths): cada
  `tipo: wasm` spawnea su propio proceso. El caso 50+ módulos se mitiga con
  la deduplicación por path (dos ejecutores con el mismo `.wasm` → un
  puente) y con el patrón "un `.wasm` que despacha N nombres" (soportado
  desde M5-ext.1 como un `grpc` más). Si el proceso por módulo se nota,
  un futuro "puente multi-wasm" (un puente que carga N componentes) es un
  incremental de este mismo binario.
- **El puente en temp**: se extrae del binario a `/tmp` al arrancar. La
  escritura en temp es necesaria para spawnear un ejecutable; alternativa
  futura: `memfd`/`posix_spawn` en Linux.
- **Sin shutdown ordenado**: el puente sale por EOF del stdin (host vivo) o
  se queda si el host muere sin cerrar el pipe; el sistema operativo lo
  limpia igualmente (el pipe se cierra solo al morir el host).
- **`tonic` sólo en el puente**: `wasi-grpc` sigue siendo la pila de los
  guests WASM. Unificar ambas (abstraer `Transport` en `wasi-grpc`) queda
  descartado por ahora: el puente es nativo y tonic es la herramienta
  estándar; `wasi-grpc` existe para WASM, donde tonic no compila.
- **El WIT se distribuye copiando el fichero** (para la guía). Publicar un
  crate `anvil-paso` en crates.io que lo bundle: post-MVP.

## Consecuencias

- ADR-0014 queda **superseded** en la parte del contrato del `.wasm` del
  usuario (`ANVIL_PORT`, "el `.wasm` es un servidor gRPC"). El resto
  (host como cargador, puerto efímero, deduplicación, overrides) se
  mantiene.
- `TipoEjecutor::Wasm { path }` no cambia en el modelo ni en el cargador:
  sigue siendo una directiva de carga validada al cargar (el path debe
  existir) y traducida por el host a `grpc` (override `--ejecutor`). El
  motor nunca lo ejecuta (`Error::EjecutorWasmSinHost` si llega sin
  traducir).
- `paso.proto` no cambia (RNF-05): sigue siendo el contrato motor↔ejecutor.
  Lo que cambia es **dentro del ejecutor**: para un `.wasm` de paso, el
  puente traduce `paso.proto` ↔ `anvil:paso`.
- El ejecutor embebido sigue con `wasi-grpc` + `ANVIL_PORT` (default 9100).
- La demo `ejemplos/demo_wasm.yaml` usa el componente `ejemplos/hola-paso`
  (el "hola mundo"): el host spawnea el puente, el motor despacha los tres
  pasos (embebido + componente), límite evaluado por el motor.
- La guía "escribe un paso en Rust, compílalo a `.wasm`, ejecútalo con
  Anvil" es la referencia oficial para usuarios externos.
