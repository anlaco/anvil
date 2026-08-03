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

## M2 — ResultSink desacoplado · MVP / MVP-parcial ✅ (hecho salvo SQLite)

- Reemplazar `println!` por un `ResultSink` con lifecycle (estilo
  `ResultListener` de OpenTAP): **consola, JSON, CSV** (RF-21, RF-22).
  **SQLite aplazado** (ADR-0007: SQLite es C y no compila en `wasm32-wasip2`
  en este toolchain; el valor real vs TestStand es el desacoplamiento, no
  la BD embebida).
- Reintento/reconexión ante fallos transitorios (RF-23, MVP-parcial):
  infraestructura ligera de reintento de IO para los sinks de fichero.
- El reporte textual congelado se conserva como uno de los sinks (RNF-08).

→ [diseno/reportes.md](diseno/reportes.md), [adr/0007-sqlite-aplazado.md](adr/0007-sqlite-aplazado.md)

## M3 — Step types built-in + límites como datos · MVP / MVP-parcial ✅ (hecho)

- Built-in MVP: **pass/fail**, **limit test**, **action** (RF-25, RF-26,
  RF-27). **statement** se implementa en M4-núcleo (paso local, sin gRPC);
  **sequence call** se aplaza a **M4b** (depende de subsecuencias y de
  Parameters entrada/salida reales).
- Límites high/low/comparación como **datos first-class** (RF-29): el límite
  vive en el YAML, no en el código del paso; el paso mide y el **motor**
  evalúa el límite declarado (ADR-0008). `paso.proto` no cambia.
- **Property loader**: límites desde un fichero sidecar (RF-30, MVP-parcial);
  el cargador los inyecta por nombre de paso, sobreescribiendo el embebido.
- **Empaquetado/versionado de pasos (registro/descubrimiento):** aplazado a
  post-M3 — toca el contrato gRPC (superficie crítica, RNF-05) y merece un
  cierre con ADR aparte.

→ [diseno/modelo-de-pasos.md](diseno/modelo-de-pasos.md),
[diseno/limites-y-estados.md](diseno/limites-y-estados.md),
[adr/0008-limites-evaluados-por-el-motor.md](adr/0008-limites-evaluados-por-el-motor.md)

## M4-núcleo — Variables, control de flujo y expresiones · MVP-parcial ✅ (hecho)

- **Expression engine** (`crates/expr`) — subconjunto, sintaxis **Julia**
  (no C-like) (RF-35): `+ - * /`, `== != < > <= >=` (encadenables), `&& || !`
  (con cortocircuito), `nothing` para ausencia, acceso a scopes y a
  `resultado.*`, asignación con `=`. AST acotado, sin deps externas (compila
  a WASM, ADR-0001); Bool estricto (sin truthiness, como Julia).
- Scopes **Locals / Parameters / FileGlobals** (RF-31), **motor-side**: viven
  en `EntornoMotor`; el cableo al paso por el wire es post-MVP (ADR-0009).
- **Precondición** por step (RF-33): el motor evalúa antes de invocar; si
  falsa, se salta sin gastar intento.
- Control de flujo: **disable** y **pause_on_fail** (RF-34). `step` se aplaza
  (WASI P2 sin espera fiable).
- Paso **statement** local (RF-27): ejecuta sentencias sin gRPC.
- **Sequence call** (RF-27) aplazado a **M4b**: requiere subsecuencias
  llamables y Parameters entrada/salida reales. `paso.proto` no cambia
  (patrón ADR-0008 → ADR-0009).

→ [diseno/variables-y-alcances.md](diseno/variables-y-alcances.md),
[diseno/motor-de-expresiones.md](diseno/motor-de-expresiones.md),
[diseno/motor-de-ejecucion.md](diseno/motor-de-ejecucion.md),
[adr/0009-expresiones-precondiciones-y-asignaciones-las-evalua-el-motor.md](adr/0009-expresiones-precondiciones-y-asignaciones-las-evalua-el-motor.md)

## M4b — Sequence call / subsecuencias · MVP-parcial ✅ (hecho)

- **Sequence call** (RF-27): invocar otra secuencia como un paso, con
  **Parameters entrada/salida by-reference** reales (como TestStand) y
  anidamiento del `ResultadoSecuencia`.
- Subsecuencias **inline** (por nombre, bajo `subsecuencias:`) o **en archivo
  aparte** (por path relativo). El cargador resuelve paths, valida lvalues y
  firma, y detecta ciclos al cargar; el motor no abre ficheros (ADR-0005).
- Relajación acotada de "sólo se muta Locals" (ADR-0009): la subsecuencia
  escribe en sus `parameters` (retorno); la raíz, no; el paso gRPC sigue
  aislado.
- Recortes MVP-parcial: by-value y by-reference transitivo post-MVP; las
  inline no se llaman entre sí (para eso, archivo externo); sin
  `reintentos`/`limite` en el call.

→ [diseno/modelo-de-pasos.md](diseno/modelo-de-pasos.md),
[diseno/variables-y-alcances.md](diseno/variables-y-alcances.md),
[adr/0010-sequence-call-lo-orquesta-el-motor-cargador-resuelve-paths.md](adr/0010-sequence-call-lo-orquesta-el-motor-cargador-resuelve-paths.md)

## M5 — Process model Sequential + CLI · MVP-parcial

- Separación "secuencia vs. cómo se corre en producción" (Sequential
  simple + plug-ins, **no** el process model de TestStand 1:1) (RF-38).
- Adapter gRPC de instrumentos pulido (RF-36).
- CLI headless maduro (RF-40). **Empaquetado como un binario único** que
  hospeda wasmtime y los dos guests WASM (motor + ejecutor) en sandbox,
  hablando gRPC por loopback (ADR-0011): `./anvil secuencia.yaml`, sin
  instalar wasmtime. El guest motor sigue disponible como `.wasm` para
  depuración con el CLI de wasmtime.

### M5-ext — Executores de lenguaje y cargador de `.wasm` · MVP extendido

#### M5-ext.1 — Routing multi-endpoint y relajación acotada del loopback ✅ (hecho, ADR-0013)

- **Routing nombre→endpoint** (RF-36.1, RF-36.3): `ejecutores:` en el YAML
  (embebido/wasm/grpc), `ejecutor:` por paso, tabla de conexiones en el
  motor (`Motor::desde_programa`), override CLI `--ejecutor nombre=host:puerto`.
- **Relajación acotada del loopback** (ADR-0011): IPs no-loopback solo si se
  declaran en `ejecutores:`; sin declaración, loopback-only. Flag
  `--solo-loopback` en el host.
- **`TipoEjecutor::Wasm` definido y validado al cargar** (el path debe
  existir), **sin instanciar** (error claro al ejecutar: requiere M5-ext.2).
- Demo `ejemplos/demo_ejecutores.yaml`: embebido + ejecutor Python en
  loopback (sin Docker).

#### M5-ext.2 — Cargador de `.wasm` por path host-side · condicionado a Telekino

- **Cargador de `.wasm` por path** (RF-36.2, modelo `.vi`): el **host**
  (no el ejecutor embebido — un guest WASM no puede instanciar wasmtime
  dentro de sí mismo, ADR-0013) instancia un `Store` por módulo y lo expone
  como endpoint gRPC en loopback. AOT precompile a `.cwasm` +
  `StoreLimitsBuilder` + lazy loading + preload al abrir la secuencia (como
  TestStand). Modo Debug con `Config::debug_info(true)` + LLDB.
- **Condicionado a Telekino**: se implementa cuando el equipo de Telekino
  cierre su formato de salida (un `.wasm` por QVI vs. un `.wasm` fusionado).
  M5-ext.1 ya valida los paths; esta fase es un incremental del host.

#### M5-ext.3 — Modo Run con `.wasm` fusionado de Telekino · condicionado a Telekino

- Si Telekino genera un **único `.wasm`** que despacha por etiqueta, Anvil lo
  consume como un endpoint `grpc` más (1 Store, N llamadas). La fusión es
  responsabilidad de Telekino, no de Anvil.
- Junto con M5-ext.2 forma la **arquitectura a la larga** (ADR-0013,
  `docs/planes/m5-ext.md`): Debug con `.wasm` sueltos por QVI + Run con el
  `.wasm` fusionado — el análogo de TestStand (Dev System depurable vs.
  Run-Time Engine).

#### M5-ext.4 — LID (Legacy Isolation Domain) · post-M5-ext

- Patrón de despliegue para correr un ejecutor de lenguaje en un SO legacy
  (Win7/VM) con aislamiento declarado ("puertas declaradas"). **Aplazado**:
  primero moderno (todo loopback), después legacy. La tecnología de
  aislamiento (Docker/VM/Sandboxie…) se define al construir el primer LID
  real; la investigación está en
  [investigacion/aislamiento-lid.md](investigacion/aislamiento-lid.md).

→ [diseno/proceso-de-test.md](diseno/proceso-de-test.md),
[diseno/integracion-instrumentos.md](diseno/integracion-instrumentos.md),
[diseno/ui-vs-headless.md](diseno/ui-vs-headless.md),
[diseno/executores-lenguaje.md](diseno/executores-lenguaje.md),
[adr/0012-executores-de-lenguaje-como-modulos.md](adr/0012-executores-de-lenguaje-como-modulos.md),
[adr/0013-cargador-wasm-host-side-y-routing.md](adr/0013-cargador-wasm-host-side-y-routing.md)

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
- **Sink SQLite**: persistencia local consultable (aplazado del MVP, ADR-0007;
  requiere toolchain C→wasm o impl. Rust pura madura, o un sink en el host).
  → diseno/reportes.md
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