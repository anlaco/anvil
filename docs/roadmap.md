# Roadmap

Hitos de Anvil de lo más cercano a lo más lejano. Cada hito marca su
alcance (MVP / MVP-parcial / post-MVP). Lo **ya hecho** se marca; el resto
es **propuesta** de orden, no compromiso de fecha.

La priorización viene de
[`investigacion/TestStand-y-competencia.md`](investigacion/TestStand-y-competencia.md)
§5; los requisitos de [requisitos.md](requisitos.md).

## M0 — Prototipo ✅ (hecho)

Lo que ya existe en el repo:

- Motor + ejecutor gRPC (WASM, wasmtime) sobre `wasi-grpc`.
- Contrato del paso en `crates/modelo/paso.proto` (`PeticionPaso`,
  `ResultadoPasoProto`, `service EjecutorPasos`), espejado a mano en
  `proto.rs`.
- Semántica Setup→Main→Cleanup (corte en 1er fallo, Cleanup siempre).
- Reintentos por paso con `intento` comunicado.
- Estados `paso`/`fallo`/`error` + agregado `error > fallo`.
- Reporte textual congelado (`ResultadoSecuencia::reporte`).
- Pasos demo simulados (`pasos_demo`).
- Dogfooding de `wasi-grpc`.

> En M0 la secuencia se construye **en código** (`basica_datos.rs`) y el
> reporte es un `println!`. Esos son los siguientes huecos.

## M1 — Secuencia como datos (YAML) · MVP

- Schema YAML de `DefinicionSecuencia` y cargador (RF-20).
- Validación del schema al cargar.
- Mantiene el motor genérico: el YAML se traduce a `DefinicionSecuencia`,
  el motor no cambia (ADR-0005).

→ [diseno/formato-de-secuencia.md](diseno/formato-de-secuencia.md)

## M2 — ResultSink desacoplado · MVP / MVP-parcial

- Reemplazar `println!` por un `ResultSink` con lifecycle (estilo
  `ResultListener` de OpenTAP): consola, JSON, CSV, SQLite (RF-21, RF-22).
- Reintento/reconexión ante fallos transitorios (RF-23, MVP-parcial).
- El reporte textual congelado se conserva como uno de los sinks.

→ [diseno/reportes.md](diseno/reportes.md)

## M3 — Step types built-in + límites como datos · MVP / MVP-parcial

- Built-in MVP: **pass/fail**, **limit test**, **action**, **sequence call**,
  **statement** (RF-25, RF-26, RF-27).
- Límites high/low/comparación como **datos first-class** (RF-29).
- **Property loader**: límites desde fichero externo (RF-30, MVP-parcial).
- Empaquetado/versionado de pasos (registro/descubrimiento).

→ [diseno/modelo-de-pasos.md](diseno/modelo-de-pasos.md),
[diseno/limites-y-estados.md](diseno/limites-y-estados.md)

## M4 — Variables, control de flujo y expresiones · MVP-parcial

- Scopes **Locals / Parameters / FileGlobals** (RF-31).
- Precondición por step (RF-33).
- Control de flujo: **pause-on-fail**, **step**, **disable** (RF-34).
- **Expression engine** — subconjunto, **sintaxis Python/Scilab/MATLAB-like**
  (no C-like) (RF-35).

→ [diseno/variables-y-alcances.md](diseno/variables-y-alcances.md),
[diseno/motor-de-expresiones.md](diseno/motor-de-expresiones.md),
[diseno/motor-de-ejecucion.md](diseno/motor-de-ejecucion.md)

## M5 — Process model Sequential + CLI · MVP-parcial

- Separación "secuencia vs. cómo se corre en producción" (Sequential
  simple + plug-ins, **no** el process model de TestStand 1:1) (RF-38).
- Adapter gRPC de instrumentos pulido (RF-36).
- CLI headless maduro (RF-40): `wasmtime run anvil.wasm secuencia.yaml`.

→ [diseno/proceso-de-test.md](diseno/proceso-de-test.md),
[diseno/integracion-instrumentos.md](diseno/integracion-instrumentos.md),
[diseno/ui-vs-headless.md](diseno/ui-vs-headless.md)

**Fin del MVP** ≈ M5. Lo siguiente es post-MVP.

## Post-MVP (explícitamente fuera de v1)

- **Paralelismo** (Parallel/Batch) con **cancelación jerárquica**
  (RF-39). → diseno/proceso-de-test.md
- **Operator UI web** + roles (admin/engineer/technician/operator) + login
  separado del SO (RF-41, UIMsgs). → diseno/ui-vs-headless.md
- **Editor visual** drag-and-drop del archivo del code module con
  **auto-introspección de parámetros y retorno** del paso (VI/DLL/Python/
  Scilab), como TestStand. Requiere extensión del contrato para exponer la
  firma del paso (ver [contrato-grpc.md](contrato-grpc.md)). →
  diseno/ui-vs-headless.md, diseno/modelo-de-pasos.md
- **ResultSinks sectoriales**: STDF / ATML (RF-24). → diseno/reportes.md
- **PyVISA/SCPI nativo** (RF-37). → diseno/integracion-instrumentos.md
- **StationGlobals** (RF-32). → diseno/variables-y-alcances.md
- **Custom step types** con substeps (RF-28). → diseno/modelo-de-pasos.md
- **Expression engine avanzado** (subconjunto → completo). →
  diseno/motor-de-expresiones.md
- **Online limit editing** sin re-deploy; MES/ERP y trazabilidad por serial
  number; golden-sample / Jidoka y monitoring en tiempo real.

## Out-of-scope (al menos en v1)

- Replicar el process model de TestStand 1:1 (Parallel/Batch + callbacks +
  entry points).
- Integración con LabVIEW/CVI.
- Debugger visual completo.

## Procesos diferidos

- `docs/rfcs/` y `docs/proceso-rfc.md`: proceso para cambios del contrato
  `paso.proto` o de la semántica. Se activa cuando haga falta.
- `MAINTAINERS.md`, `.github/CODEOWNERS`, `CHANGELOG.md`: hasta primer release
  / >1 mantenedor.

## Cómo se gestiona el alcance

- Cada hito se vincula a issues cuando arranque su implementación.
- Un alcance que se sale del MVP se mueve explícitamente a post-MVP con un
  ADR si cambia una decisión de fondo.
- La regla rectora: **no replicar TestStand 1:1**; copiar lo bueno
  (ResultListener, perfiles de instrumento, ResultSinks industriales) y
  dejar fuera lo frágil (process model monolítico, callbacks que rompen
  secuencias existentes).