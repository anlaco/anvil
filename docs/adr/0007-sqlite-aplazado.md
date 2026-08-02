# ADR-0007: Aplazamiento del sink SQLite

- **Estado:** Aceptada
- **Fecha:** M2

## Contexto

[`diseno/reportes.md`](../diseno/reportes.md) lista SQLite como uno de los
sinks del MVP (RF-22): "persistencia local para consulta y analítica
ligera". La investigación
([`investigacion/TestStand-y-competencia.md`](../investigacion/TestStand-y-competencia.md))
sitúa el reporte como columna vertebral del secuenciador, no como addon
(§1.5), y documenta dos cosas que importan aquí:

1. **TestStand no embebe la BD**: el logging va a una BD **externa** por
   ODBC/ADO, y sufre por ello (§2): schema fijo rígido, Locals inaccesibles
   desde la config y **conexión cacheada que rompe con corte de red sin
   auto-retry**. La "oportunidad Anvil" que anota la investigación **no es**
   "SQLite embebido": es *"`ResultSink` desacoplado con
   **reintento/reconexión** y **schema configurable**"*.
2. **OpenTAP sí embebe SQLite** como `ResultListener` (§3), pero *en proceso*
   con el motor (C#/.NET, **sin sandbox**). pytestlab usa HDF5/SQLite (§3).

La diferencia con Anvil: ADR-0001 fija el runtime en **`wasm32-wasip2`** (un
sandbox). Para OpenTAP "sink en proceso" es trivial porque no hay sandbox;
para Anvil "en proceso con el motor" = **dentro del WASM**.

El problema técnico: **SQLite es una librería en C**. El binding Rust
estándar (`rusqlite`) con la feature `bundled` compila la amalgamación en C
de SQLite vía el crate `cc`, que requiere un compilador de C con el target
`wasm32-wasip2`. El toolchain de Anvil **no dispone de ese compilador** en
este entorno (`clang` sin targets wasm, sin `wasm32-wasi-clang`). No existe
una reimplementación madura de SQLite en Rust puro que compile a
`wasm32-wasip2` de forma estable para embebido (las existentes, p. ej.
`limbo`/`turso`, no están listas). RNF-09 ancla las decisiones al repo, no a
suposiciones.

## Decisión

**Aplazar el sink SQLite del MVP.** No bloquea el hito M2: los sinks MVP
entregados son **consola, JSON y CSV** (RF-21, y RF-22 salvo SQLite). SQLite
sale del MVP y pasa a **post-MVP**.

Dos puntos que fijan el alcance:

- El valor diferencial de Anvil frente a TestStand **no es** "BD embebida"
  (TestStand externaliza y sufre). Es el **desacoplamiento del sink + reintento
  + schema configurable**. Eso ya se entrega en M2 con los sinks que sí
  compilan.
- El trait `ResultSink` y su lifecycle (ADR-implicito en
  [`result_sink.rs`](../../crates/modelo/src/result_sink.rs)) quedan fijados
  en M2, de forma que un sink SQLite futuro —embebido o en el host— se añade
  **sin tocar el motor**.

## Consecuencias

**Positivas:**

- M2 se desbloquea sin deuda de toolchain (compilar C→wasm es frágil y ajeno
  al producto).
- El lifecycle del sink queda fijado y SQLite se añade después como un sink
  más, sin migración.
- El MVP entrega ya el desacoplamiento, que es el valor real frente a
  TestStand.

**Negativas:**

- Sin persistencia local **consultable** en el MVP: quien la quiera usa
  JSON/CSV a fichero y los carga fuera (un `sqlite3 .import` o un script).
  Deuda explícita de volver a evaluar SQLite cuando haya toolchain C→wasm o
  una impl. Rust pura madura.

**Neutras:**

- RF-23 (reintento/reconexión) se entrega MVP-parcial sobre los sinks de
  fichero (JSON/CSV), no sobre BD; la infraestructura de reintento queda
  lista para los sinks de red futuros.

## Alternativas descartadas

- **Compilar C→`wasm32-wasip2`:** descartada por imposibilidad técnica (no
  hay compilador de C para el target en el toolchain actual). No es una
  preferencia, es un hecho.
- **Sink SQLite en el host (fuera del WASM):** el motor emite los resultados
  y un colector nativo en el host los ingiere en SQLite. Arquitectónicamente
  viable —el sandbox aísla los *pasos*, no los *resultados*, que son lo que
  precisamente quiere salir— pero añade una frontera de integración (IPC /
  sockets WASI) fuera del alcance del MVP. Abierta para post-MVP.
- **Reimplementación pura de Rust (`limbo`/`turso`):** inmadura para
  embebido estable en `wasm32-wasip2` hoy. Descartada por madurez.

## Enlaces

- [ADR-0001](0001-rust-wasm.md) (runtime WASM),
  [`investigacion/TestStand-y-competencia.md`](../investigacion/TestStand-y-competencia.md)
  §1.5, §2, §3,
  [`diseno/reportes.md`](../diseno/reportes.md),
  [`roadmap.md`](../roadmap.md) M2.