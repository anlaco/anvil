# Requisitos

SRS ligero: lo que Anvil debe hacer, **verificable y trazable** a su fuente
real del repo o a un ADR. Las características de calidad (correcto, no
ambiguo, verificable, trazable) se conservan; la maquinaria formal de un SRS
de 20 subsecciones no.

Prioridad: **MVP** (Must), **MVP-parcial** (Should), **post-MVP** (Could),
**out-of-scope** (Won't). La base priorizada es
[`investigacion/TestStand-y-competencia.md`](investigacion/TestStand-y-competencia.md)
§5; lo ya decidido se ancla al código.

## Requisitos funcionales

### Ejecución de la secuencia

| ID | Requisito | Prioridad | Trazabilidad |
|---|---|---|---|
| RF-01 | Una secuencia se compone de tres fases: **Setup**, **Main**, **Cleanup**. | MVP | `modelo/src/lib.rs::DefinicionSecuencia` |
| RF-02 | **Setup**: corren todos los pasos; si alguno no pasa, el Main se salta entero. | MVP | `motor/src/lib.rs::ejecuta_secuencia` |
| RF-03 | **Main**: solo corre si el Setup fue bien; **corta en el primer fallo**. | MVP | `motor/src/lib.rs::ejecuta_secuencia` |
| RF-04 | **Cleanup**: corre **siempre**, pase lo que pase antes. | MVP | `motor/src/lib.rs::ejecuta_secuencia` |
| RF-05 | Cada paso se invoca **por gRPC por su nombre**, nunca por llamada directa. | MVP | ADR-0003; `motor/src/lib.rs::ejecuta_paso` |
| RF-06 | El motor no conoce la implementación del paso; despacha por nombre. | MVP | ADR-0005 |

### Reintentos

| ID | Requisito | Prioridad | Trazabilidad |
|---|---|---|---|
| RF-07 | Cada paso declara un número máximo de **intentos** (total, no extras: 1 = sin reintentos). | MVP | `modelo/src/lib.rs::DefinicionPaso.reintentos` |
| RF-08 | El motor reintenta mientras el paso no pase y queden intentos. | MVP | `motor/src/lib.rs::ejecuta_con_reintentos` |
| RF-09 | El **número de intento** (desde 1) llega al paso, que puede usarlo. | MVP | `paso.proto::PeticionPaso.intento` |

### Estados y agregado

| ID | Requisito | Prioridad | Trazabilidad |
|---|---|---|---|
| RF-10 | Cada resultado tiene un **estado**: `paso`, `fallo` o `error` (texto, no enum). | MVP | `modelo/src/lib.rs::ResultadoStep.estado` |
| RF-11 | Un `fallo` (criterio de aceptación no cumplido) es un resultado **válido**, no un error del motor. | MVP | `motor/src/lib.rs::Error` (solo Red/Protobuf) |
| RF-12 | Un nombre de paso desconocido produce `error`, no pánico. | MVP | `pasos_demo/src/lib.rs::despacha` |
| RF-13 | Agregado de secuencia: `error` si alguno dio `error`; si no, `fallo` si alguno dio `fallo`; si no, `paso`. | MVP | `modelo/src/lib.rs::ResultadoSecuencia::estado` |

### Contrato del paso (gRPC)

| ID | Requisito | Prioridad | Trazabilidad |
|---|---|---|---|
| RF-14 | El contrato del paso se define en `paso.proto`: `PeticionPaso`, `ResultadoPasoProto`, `service EjecutorPasos{rpc Invoca}`. | MVP | `modelo/paso.proto` |
| RF-15 | La ruta del método es `/EjecutorPasos/Invoca` (sin `package` en el `.proto`). | MVP | `modelo/src/proto.rs::RUTA_INVOCA` |
| RF-16 | Las medidas viajan como **string**; un campo vacío no se transmite (proto3). | MVP | `modelo/src/proto.rs::a_texto` |
| RF-17 | Los valores enteros se codifican sin decimales (`"5"`, no `"5.0"`). | MVP | `modelo/src/proto.rs::a_texto` |
| RF-18 | El contrato es **versionado** y estable; un cambio que rompa compatibilidad exige un ADR/RFC. | MVP | [contrato-grpc.md](contrato-grpc.md) |

### Secuencia como datos

| ID | Requisito | Prioridad | Trazabilidad |
|---|---|---|---|
| RF-19 | La secuencia es **datos** (`DefinicionSecuencia`), no código. | MVP | ADR-0002; `modelo/src/lib.rs` |
| RF-20 | Existe un **schema YAML** para `DefinicionSecuencia` y un cargador. | MVP | [diseno/formato-de-secuencia.md](diseno/formato-de-secuencia.md) *(propuesta)* |

### Reportes / ResultSink

| ID | Requisito | Prioridad | Trazabilidad |
|---|---|---|---|
| RF-21 | El resultado de la secuencia se vierte a un **ResultSink** desacoplado. | MVP | [diseno/reportes.md](diseno/reportes.md) *(propuesta)* |
| RF-22 | ResultSinks mínimos: consola, JSON, CSV, SQLite. | MVP | diseno/reportes.md |
| RF-23 | El ResultSink reintenta/reconecta ante fallos transitorios (p. ej. corte de red). | MVP-parcial | diseno/reportes.md |
| RF-24 | STDF y ATML como ResultSinks sectoriales. | post-MVP | diseno/reportes.md |

### Step types y límites

| ID | Requisito | Prioridad | Trazabilidad |
|---|---|---|---|
| RF-25 | Built-in **pass/fail** (sin medida, solo `paso`/`fallo`). | MVP | `pasos_demo/src/lib.rs::verificar_led`; [diseno/modelo-de-pasos.md](diseno/modelo-de-pasos.md) |
| RF-26 | Built-in **limit test** (medida contra high/low o comparación). | MVP | `motor/src/lib.rs::aplicar_limite`; `modelo/src/lib.rs::Limite` (ADR-0008) |
| RF-27 | Built-in **action**, **sequence call**, **statement**. | MVP-parcial | action: `pasos_demo/src/lib.rs::abrir_rele`; statement: `motor/src/lib.rs::ejecuta_statement_puro` (M4-núcleo); sequence call: `motor/src/lib.rs::ejecuta_sequence_call` (M4b, inline + path, by-reference; ADR-0010) |
| RF-28 | **Custom step types** con substeps encapsulados. | post-MVP | diseno/modelo-de-pasos.md |
| RF-29 | Los límites son **datos first-class** (no aserciones ad-hoc). | MVP-parcial | `modelo/src/lib.rs::Limite`; `cargador/src/lib.rs::LimiteYaml`; [ADR-0008](adr/0008-limites-evaluados-por-el-motor.md) |
| RF-30 | **Property loader**: límites desde un fichero externo. | MVP-parcial | `cargador/src/lib.rs::cargar_limites_de_archivo` + `aplicar_limites` |

### Variables y control de flujo

| ID | Requisito | Prioridad | Trazabilidad |
|---|---|---|---|
| RF-31 | Variables con scopes **Locals**, **Parameters**, **FileGlobals**. | MVP-parcial | `modelo::ValorDefinicion` + `motor::EntornoMotor` (M4-núcleo, motor-side); Parameters entrada/salida by-reference en M4b (`motor::ejecuta_sequence_call`, ADR-0010); [diseno/variables-y-alcances.md](diseno/variables-y-alcances.md) |
| RF-32 | **StationGlobals** (compartidas por estación). | post-MVP | diseno/variables-y-alcances.md |
| RF-33 | **Precondición** por step (el paso se salta si no se cumple). | MVP-parcial | `motor::evalua_precondicion` (M4-núcleo); [diseno/motor-de-expresiones.md](diseno/motor-de-expresiones.md) |
| RF-34 | Control de flujo: **pause-on-fail**, **step**, **disable** de pasos. | MVP-parcial | `disable` + `pause_on_fail` en `DefinicionPaso` (M4-núcleo); `step` post-MVP; [diseno/motor-de-ejecucion.md](diseno/motor-de-ejecucion.md) |
| RF-35 | **Expression engine** (sintaxis **Julia**, **no** C-like). | MVP-parcial | `crates/expr` (M4-núcleo); [diseno/motor-de-expresiones.md](diseno/motor-de-expresiones.md) |
| RF-36 | Integración de instrumentos por **adapter gRPC**. | MVP-parcial | [diseno/integracion-instrumentos.md](diseno/integracion-instrumentos.md) |
| RF-36.1 | Un paso puede servirse por un **ejecutor gRPC remoto** en otro lenguaje o SO (executores de lenguaje distribuidos en `executores/`); el motor despacha por **nombre→endpoint**. | MVP extendido (M5-ext.1) ✅ | `Motor::desde_programa` + `ejecutores:`/`ejecutor:` (M5-ext.1, ADR-0013); [ADR-0013](adr/0013-cargador-wasm-host-side-y-routing.md); [diseno/executores-lenguaje.md](diseno/executores-lenguaje.md) |
| RF-36.2 | Un paso **WASM propio** se carga por **path** en runtime (modelo `.vi`), sin recompilar; cada módulo corre en su propio `Store`. Lo carga el **host** (no el ejecutor embebido: un guest WASM no puede instanciar wasmtime dentro de sí mismo). | MVP extendido (M5-ext.2) ✅ | ADR-0014: `ANVIL_PORT` + override `--ejecutor` sintético (el motor nunca ejecuta `Wasm`); [diseno/executores-lenguaje.md](diseno/executores-lenguaje.md) |
| RF-36.3 | El routing `ejecutores:` vive en el **YAML** de la secuencia con **override por flag** `--ejecutor nombre=host:puerto` (patrón embebido-primero, como los límites). | MVP extendido (M5-ext.1) ✅ | `cargador::aplicar_override_ejecutores` (M5-ext.1, ADR-0013); [diseno/executores-lenguaje.md](diseno/executores-lenguaje.md) |
| RF-36.4 | **LID** (despliegue legacy): un ejecutor de lenguaje puede correr en un SO legacy (Win7/VM) con aislamiento declarado; Anvil lo ve como un endpoint gRPC más. | MVP extendido (**aplazado a post-M5-ext**; tecnología a definir) | ADR-0013; [diseno/executores-lenguaje.md](diseno/executores-lenguaje.md) |
| RF-37 | PyVISA/SCPI nativo. | post-MVP | diseno/integracion-instrumentos.md |

### Process model y UI

| ID | Requisito | Prioridad | Trazabilidad |
|---|---|---|---|
| RF-38 | **Process model Sequential** simple; separación secuencia vs. "cómo se corre en producción". | MVP-parcial | [diseno/proceso-de-test.md](diseno/proceso-de-test.md) |
| RF-39 | Paralelismo (Parallel/Batch) con cancelación jerárquica. | post-MVP | diseno/proceso-de-test.md |
| RF-40 | **Headless/CLI** primero. | MVP | [diseno/ui-vs-headless.md](diseno/ui-vs-headless.md) |
| RF-41 | Operator UI web + UIMsgs. | post-MVP | diseno/ui-vs-headless.md |

### Out-of-scope (v1)

- RF-N01: Replicar el process model de TestStand 1:1 (Parallel/Batch +
  callbacks + entry points).
- RF-N02: Integración con LabVIEW/CVI.
- RF-N03: Debugger visual completo.

## Requisitos no funcionales

| ID | Requisito | Prioridad | Trazabilidad |
|---|---|---|---|
| RNF-01 | **Portabilidad**: Anvil se compila a `wasm32-wasip2` y corre bajo `wasmtime` en cualquier SO soportado. | MVP | ADR-0001; `rust-toolchain.toml` |
| RNF-02 | **Aislamiento**: el secuenciador corre en un sandbox WASM; el interior de cada paso es opaco al motor. | MVP | ADR-0001, ADR-0005 |
| RNF-02.1 | **Relajación acotada del loopback**: el motor solo conecta a IPs no-loopback **declaradas** (ejecutores de lenguaje, LID); sin declaración, loopback-only (ADR-0011). El sandbox WASM del núcleo se conserva. | MVP extendido (M5) | [ADR-0012](adr/0012-executores-de-lenguaje-como-modulos.md) |
| RNF-03 | **Determinismo de reintentos**: para la misma secuencia y los mismos pasos, el número de intentos y el orden son reproducibles. | MVP | `motor/src/lib.rs::ejecuta_con_reintentos` *(verificar en CI)* |
| RNF-04 | **Rendimiento**: el coste de una llamada gRPC local es despreciable frente al tiempo de un instrumento real (no es cuello de botella). | MVP | ADR-0003 |
| RNF-05 | **Estabilidad del contrato**: `paso.proto` no se rompe sin versionado y un ADR/RFC. | MVP | [contrato-grpc.md](contrato-grpc.md) |
| RNF-06 | **Seguridad (hardware real)**: un paso defectuoso no puede dañar equipo; el Cleanup garantizado mitiga estados peligrosos. | MVP | [SECURITY.md](../SECURITY.md); RF-04 |
| RNF-07 | **Licencia**: el producto es AGPL-3.0; las librerías (WIT, wasi-grpc, wasi-visa) son Apache-2.0. | MVP | ADR-0004; [licencia.md](licencia.md) |
| RNF-08 | **Reporte congelado**: el formato textual actual (`ResultadoSecuencia::reporte`) es spec; no se cambia sin querer. | MVP | `modelo/src/lib.rs::reporte` |
| RNF-09 | **No re-investigación**: las decisiones se anclan al repo o a la investigación citada, no a suposiciones. | MVP | convención de redacción |

## Verificación

- Todo RF/RNF es trazable (columna *Trazabilidad*) a un archivo del repo o
  a un ADR.
- Los marcados *(propuesta)* son decisiones de diseño a confirmar en su doc
  de `diseno/`.
- Las pruebas unitarias actuales (`cargo test`) cubren RF-09, RF-13, RF-16,
  RF-17, RF-25, RF-26, RF-29, RF-30 (evaluación de límites en `motor` y
  `modelo`, property loader en `cargador`, sinks JSON/CSV con comparación) y,
  desde M4-núcleo, RF-27 (statement), RF-31 (variables/scopes en `EntornoMotor`),
  RF-33 (precondición), RF-34 (disable/pause_on_fail) y RF-35 (expression engine
  en `crates/expr`: parser, evaluator, reglas de tipo, cortocircuito, `Nulo`).