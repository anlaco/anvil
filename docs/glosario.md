# Glosario

Vocabulario del dominio de Anvil. Acuña los términos propios y mapea los de
TestStand/OpenTAP que se usan en el resto de la documentación. Cuando un
término de TestStand no se replica igual en Anvil, se dice explícitamente.

> La base de dominio es
> [`investigacion/TestStand-y-competencia.md`](investigacion/TestStand-y-competencia.md)
> (§1.4 y §6). Las definiciones de Anvil se anclan al código:
> `crates/modelo/src/lib.rs`, `crates/modelo/paso.proto`.

## Núcleo de Anvil

- **Secuenciador de test.** Software que *orquesta* pruebas contra equipo
  real: corre una secuencia de pasos, reintenta los que fallan y reporta.
  TestStand lo llama *test executive* (no es un *test runner* como pytest:
  este último ejecuta código que tú escribes; un secuenciador lo orquesta).
  Ver [vision.md](vision.md).

- **Secuencia.** Una lista ordenada de pasos agrupados en Setup, Main y
  Cleanup. En Anvil es **datos** (`DefinicionSecuencia`), no código: el
  motor la recorre sin saber qué hace cada paso. Hoy se construye en código
  (`crates/motor/src/bin/basica_datos.rs`); el objetivo es cargarla desde
  YAML (ver [diseno/formato-de-secuencia.md](diseno/formato-de-secuencia.md)).

- **Paso.** La unidad de test. Se invoca **por gRPC por su nombre**; el
  motor nunca lo llama directamente. Definido por `DefinicionPaso{nombre,
  reintentos}` en `crates/modelo/src/lib.rs`. Lo que devuelve tras correr es
  un `ResultadoStep`.

- **Setup / Main / Cleanup.** Las tres fases de una secuencia:
  - **Setup**: prepara el equipo. Corren todos; si alguno no pasa, **el Main
    se salta entero**.
  - **Main**: las mediciones. Solo corre si el Setup fue bien y **corta en
    el primer fallo**.
  - **Cleanup**: libera recursos. Corre **siempre**, pase lo que pase antes
    — un equipo que se quedó encendido es peor que una secuencia que falló.
  Semántica implementada en `crates/motor/src/lib.rs::ejecuta_secuencia`.

- **Despacho por nombre.** El motor pide un paso por su `nombre` (string) al
  ejecutor por gRPC; el ejecutor lo ata a una función concreta. Es el único
  punto donde el nombre del cable se ata a código (hoy en
  `crates/pasos_demo/src/lib.rs::despacha`). Un nombre desconocido devuelve
  `error`, no pánico: una secuencia mal escrita no debe tumbar el ejecutor.

- **Reintento.** Cada paso declara cuántos intentos admite (`reintentos`).
  Es el número **total** de intentos: `1` = un solo tiro, sin reintentos. El
  motor reintenta mientras el paso no pase y queden intentos. El número de
  `intento` (empezando en 1) llega al paso, que puede usarlo para simular
  fallos transitorios.

- **Estado.** Uno de tres: `paso`, `fallo`, `error`. Se mantiene como
  **texto** (no enum) porque viaja así en `paso.proto` y porque el contrato
  admite pasos escritos en cualquier lenguaje.
  - `paso`: el paso cumplió su criterio.
  - `fallo`: el paso no cumplió un criterio de aceptación (p. ej. una medida
    fuera de rango). Es un resultado **válido**, no un error del motor.
  - `error`: el paso no pudo ejecutarse (p. ej. nombre desconocido, o un
    fallo de comunicación). Un `error` **manda sobre un `fallo`** en el
    agregado de la secuencia.

- **Agregado de secuencia.** El estado global de una secuencia corrida:
  `error` si algún paso dio `error`; si no, `fallo` si alguno dio `fallo`; si
  no, `paso`. Implementado en `ResultadoSecuencia::estado`.

- **Medida.** Un resultado numérico con límites: `valor_medido` contra
  `limite_min`/`limite_max`. En el contrato viaja como **string** (ver
  [contrato-grpc.md](contrato-grpc.md)).

- **Reporte.** La salida textual del resultado de una secuencia. El formato
  actual (`ResultadoSecuencia::reporte`) es **parte de la spec**: no se toca
  sin querer tocar la especificación. Post-MVP se sustituye por un
  **ResultSink** desacoplado (ver [diseno/reportes.md](diseno/reportes.md)).

## Componentes

- **Motor.** El cliente gRPC que recorre la secuencia. No sabe qué hace cada
  paso: los pide por nombre al ejecutor. Crate `crates/motor`.

- **Ejecutor de pasos.** El servidor gRPC que despacha pasos por nombre: el
  adaptador entre el motor genérico y los pasos concretos. Hoy es
  `crates/ejecutor_pasos` (binario que escucha en `127.0.0.1:9100`).

- **Ejecutor de lenguaje.** Ejecutor de pasos distribuido como **módulo
  aparte** (`executores/`), uno por sistema (Python, LabVIEW, MATLAB, …),
  que habla el mismo `paso.proto` con gRPC nativo de su ecosistema. Son
  **alternativas opt-in** al ejecutor WASM embebido; pueden mezclarse en la
  misma secuencia. Licencia Apache-2.0. Ver
  [diseno/executores-lenguaje.md](diseno/executores-lenguaje.md) y
  [ADR-0012](adr/0012-executores-de-lenguaje-como-modulos.md).

- **Cargador de `.wasm`.** El **host** (`anvil-host`) carga **módulos `.wasm`
  propios por path** en runtime (modelo `.vi` de TestStand: compilar y
  referenciar, sin recompilar). Cada módulo corre en su propio `Store`
  (aislamiento entre pasos). **M5-ext.2, condicionado a Telekino**: el
  ejecutor embebido no puede hacerlo (es él mismo un guest WASM; ver
  [ADR-0013](adr/0013-cargador-wasm-host-side-y-routing.md)). En M5-ext.1 el
  `TipoEjecutor::Wasm` se valida al cargar pero no se instancia.

- **Routing nombre→endpoint.** (M5-ext.1, implementado) El YAML declara
  `ejecutores:` y cada paso `grpc` su `ejecutor:`; el motor despacha por
  nombre contra una tabla de conexiones (embebido por defecto). Override por
  CLI `--ejecutor nombre=host:puerto`. Ver
  [ADR-0013](adr/0013-cargador-wasm-host-side-y-routing.md).

- **LID** (*Legacy Isolation Domain*). Patrón de despliegue: un ejecutor de
  lenguaje corre en un **SO legacy** (Windows 7/10, VM, PC en red) con
  **aislamiento declarado** (solo salen las puertas pactadas: instrumentos
  por red, ficheros). Anvil lo ve como un endpoint gRPC más; el mecanismo de
  aislamiento (contenedor/VM/firewall de SO) se define al construir.
  **Aplazado a post-M5-ext.** Ver
  [diseno/executores-lenguaje.md](diseno/executores-lenguaje.md) y
  [ADR-0013](adr/0013-cargador-wasm-host-side-y-routing.md).

- **Contrato gRPC.** La superficie pública del paso: `paso.proto` define
  `PeticionPaso`, `ResultadoPasoProto` y `service EjecutorPasos{rpc Invoca}`.
  Es la **fuente de verdad** del contrato; los structs `prost` de
  `crates/modelo/src/proto.rs` lo espejan a mano (wasi-grpc v0.1 no trae
  codegen). Ver [contrato-grpc.md](contrato-grpc.md).

- **Adapter** (término de TestStand). El mecanismo que llama a un *code
  module* en un lenguaje concreto (LabVIEW, C/C++, .NET, Python). En Anvil
  **el adapter es gRPC**: cualquier lenguaje que hable gRPC es un adapter,
  sin código de pegamento en el motor. Ver
  [diseno/modelo-de-pasos.md](diseno/modelo-de-pasos.md) e
  [integracion-instrumentos.md](diseno/integracion-instrumentos.md).

- **ResultSink.** *(Propuesta, post-MVP.)* Consumidor de resultados
  desacoplado (consola/JSON/CSV/SQLite/STDF), a imagen del `ResultListener`
  de OpenTAP. Reemplaza al `reporte()` textual actual. Ver
  [diseno/reportes.md](diseno/reportes.md).

## Runtime

- **WASM.** WebAssembly. Anvil se compila a `wasm32-wasip2` (WASI Preview 2)
  y corre bajo `wasmtime`. Da portabilidad cross-platform y **aislamiento**
  del secuenciador. Ver [ADR-0001](adr/0001-rust-wasm.md).

- **wasi-grpc.** Pila gRPC propia (repo aparte, `../wasi-grpc`,
  Apache-2.0): gRPC sobre sockets WASI nativos, porque `tonic`/`tokio` no
  compilan a WASM. Anvil la dogfoodea. Ver
  [ADR-0006](adr/0006-wasi-grpc-propio.md).

- **WIT.** *(Interfaces WIT, Apache-2.0.)* Interfaces del *component model*
  de WASM. Se mencionan en la estrategia de licencia como pieza que se
  adopta como referencia; hoy no hay archivos `.wit` en el repo.

## Dominio (mapeo TestStand → Anvil)

- **UUT / DUT.** *Unit Under Test* / *Device Under Test*: el equipo que se
  prueba. Anvil hoy no modela el UUT explícitamente (post-MVP, ligado al
  process model).

- **Step type.** Plantilla de paso con comportamiento encapsulado.
  TestStand trae *built-in* (Pass/Fail, Numeric Limit, Action, Sequence Call,
  Statement, Synchronization…) y *custom step types* con substeps. En Anvil
  el MVP incluye los built-in básicos (**pass/fail**, **limit test**, action,
  sequence call, statement); los custom son post-MVP. Ver
  [diseno/modelo-de-pasos.md](diseno/modelo-de-pasos.md).

- **Pass/Fail test.** Step type que solo decide *pasa* o *falla* sin medir:
  el paso hace algo y reporta un `estado`. Es el built-in más simple y entra
  en el **MVP** (encaja con `ResultadoStep::nuevo`, sin medida). Ver
  [diseno/modelo-de-pasos.md](diseno/modelo-de-pasos.md).

- **Limit test.** Step type que compara una **medida** contra límites
  high/low (o de comparación) y produce `paso`/`fallo`. Distinto del Pass/Fail
  (este sí mide). En Anvil ya está soportado por `ResultadoStep::medido`. Ver
  [diseno/limites-y-estados.md](diseno/limites-y-estados.md).

- **Variables y scopes.** Jerarquía de variables con alcance:
  **Locals** (locales a una secuencia), **Parameters** (entradas/salidas
  entre secuencias), **FileGlobals** (compartidas en un archivo de
  secuencia), **StationGlobals** (compartidas en la estación, post-MVP en
  Anvil). Ver [diseno/variables-y-alcances.md](diseno/variables-y-alcances.md).

- **Expression engine.** Motor de expresiones para precondiciones,
  postcondiciones, límites y asignaciones sin código pegamento. **Anvil no
  copia la sintaxis tipo C de TestStand:** apunta a una sintaxis familiar para
  ingenieros de test, cercana a **Python / Scilab / MATLAB** (lo que esa
  audiencia ya maneja), no a C. Es una divergencia deliberada. Subconjunto
  en MVP, avanzado post-MVP. Ver
  [diseno/motor-de-expresiones.md](diseno/motor-de-expresiones.md).

- **Process model.** *(TestStand.)* La separación entre "el test" (la
  secuencia) y "cómo se corre en producción" (identificar UUT, notificar,
  loguear, reportar). TestStand la materializa como una secuencia editable
  con callbacks y entry points (Sequential/Parallel/Batch). Anvil **respeta
  la separación pero no replica el modelo 1:1**: MVP = Sequential simple +
  extensión por plug-ins. Ver
  [diseno/proceso-de-test.md](diseno/proceso-de-test.md).

- **Operator Interface / UIMsgs.** *(TestStand.)* La UI de operador
  (producción) frente al Sequence Editor (desarrollo), desacopladas del motor
  por *User Interface Messages*. En Anvil, **headless/CLI en el MVP**;
  Operator UI web + UIMsgs son post-MVP. Ver
  [diseno/ui-vs-headless.md](diseno/ui-vs-headless.md).

- **Property loader.** Cargar límites desde un fichero externo, separando
  los datos de test del flujo. Post-MVP en Anvil (ver
  [requisitos.md](requisitos.md), Should).

- **STDF / ATML.** Formatos de reporte industriales (semiconductora /
  aerospace). Post-MVP en Anvil como ResultSinks sectoriales.