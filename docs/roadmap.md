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
- Pasos demo simulados (`pasos_demo`; retirados con el ejecutor embebido, [ADR-0041](adr/0041-there-is-no-embedded-executor.md)).
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

## M5 — Process model Sequential + CLI · MVP-parcial ✅ (hecho)

- Separación "secuencia vs. cómo se corre en producción" (Sequential
  simple + plug-ins, **no** el process model de TestStand 1:1) (RF-38).
  El PM es una **secuencia YAML envoltorio** (`process_models/sequential.yaml`)
  cuyo `main` hace `sequence_call` a la secuencia del usuario (nombre
  reservado `secuencia_usuario`, reescrito por el cargador); el motor no
  se toca (ADR-0005/0010). Plug-ins (`identificar_uut`, `notificar_resultado`)
  son pasos `grpc`. Sin callbacks. → ADR-0016.
- Adapter gRPC de instrumentos pulido (RF-36). Crate `pasos_scpi` con un
  paso real SCPI/TCP (`medir_voltaje_scpi`), testeado contra un mock TCP en
  loopback; el ejecutor compone adaptadores (`pasos_scpi` + `pasos_demo`).
  → ADR-0017.
- CLI headless maduro (RF-40): `--process-model`, `--validate`, `--port` +
  reintento de conexión, `--quiet`, `--help`, `--version`. **Empaquetado
  como un binario único** que hospeda wasmtime y los dos guests WASM
  (motor + ejecutor) en sandbox, hablando gRPC por loopback (ADR-0011):
  `./anvil secuencia.yaml`, sin instalar wasmtime. El guest motor sigue
  disponible como `.wasm` para depuración con el CLI de wasmtime.

> **Retirado después ([ADR-0041](adr/0041-there-is-no-embedded-executor.md)):**
> el binario ya no lleva ejecutor de pasos. `pasos_demo`, `pasos_scpi`, el
> ejecutor embebido, `--port` y `process_models/sequential.yaml` no existen;
> lo que queda de M5 es el mecanismo `--process-model`, el CLI y el binario
> único con el guest motor. Los ejemplos corren contra el banco de demo, un
> componente WASM (`ejemplos/departamento/demo`). Lo de arriba es el registro
> de cuándo se hizo.

### M5-ext — Executores de lenguaje y cargador de `.wasm` · MVP extendido

#### M5-ext.1 — Routing multi-endpoint y relajación acotada del loopback ✅ (hecho, ADR-0013)

- **Routing nombre→endpoint** (RF-36.1, RF-36.3): `ejecutores:` en el YAML
  (embebido/wasm/grpc), `ejecutor:` por paso, tabla de conexiones en el
  motor (`Motor::desde_programa`), override CLI `--executor nombre=host:puerto`.
- **Relajación acotada del loopback** (ADR-0011): IPs no-loopback solo si se
  declaran en `ejecutores:`; sin declaración, loopback-only. Flag
  `--loopback-only` en el host.
- **`TipoEjecutor::Wasm` definido y validado al cargar** (el path debe
  existir); la instanciación llegó con M5-ext.2 (ADR-0014).
- Demo `ejemplos/demo_ejecutores.yseq`: embebido + ejecutor Python en
  loopback (sin Docker). Hoy es el banco de demo WASM + Python (ADR-0041).

#### M5-ext.2 — Cargador de `.wasm` por path host-side ✅ (hecho, ADR-0014/0015)

- **Cargador de `.wasm` por path** (RF-36.2, modelo `.vi` de TestStand): el
  **host** (no el ejecutor embebido — un guest WASM no puede instanciar
  wasmtime dentro de sí mismo, ADR-0013) spawnea el **puente**
  `anvil-puente-wasm` (embebido en `anvil`, extraído a temp) con `--wasm
  <path> --port <efímero>`; el puente carga el componente y lo sirve como
  gRPC en loopback. Deduplicación por path (dos ejecutores con el mismo
  `.wasm` → un puente), preload al arrancar.
- **El `.wasm` del usuario es una función** (ADR-0015): componente WASM con
  interfaz WIT `anvil:paso` (`run(nombre, intento) -> resultado`),
  compilado con `cargo component` + `wit-bindgen` (público). Sin `wasi-grpc`,
  sin `modelo`, sin `ANVIL_PORT`, sin clonar el repo. El puente (nativo:
  wasmtime + tonic + wit-bindgen) traduce gRPC↔función; sandbox WASI vacío
  (el componente es una función pura). `paso.proto` no cambia (RNF-05).
- **El motor nunca ejecuta `Wasm`**: el host compone overrides `--executor`
  sintéticos (M5-ext.1), así el motor sólo ve `embebido`/`grpc`. AOT
  precompile a `.cwasm` + `StoreLimitsBuilder` + lazy loading + modo Debug
  + pooling/async: **post-M5-ext.2**, si la medición de 50+ Stores lo pide.
- **Agnóstico al origen del `.wasm`**: Anvil expone un contrato (WIT
  `anvil:paso`) y un mecanismo de carga (path). Lo que hay detrás —C a
  mano, Rust, Zig, un editor visual, un tercero— es opaco. El roadmap de
  Anvil avanza por sus propios requisitos, no por los de un generador
  externo.
- Demo `ejemplos/demo_wasm.yseq` + componente `ejemplos/hola-paso` (el
  "hola mundo"), verificada end-to-end.

> **Patrón soportado desde M5-ext.1** (sin hito propio): un **único `.wasm`
> que despacha por nombre** (un módulo que atiende N nombres internamente)
> es un ejecutor `grpc` más — 1 Store, N llamadas. Anvil no distingue si
> detrás hay un `.wasm` suelto por path (M5-ext.2) o un módulo que fusiona
> varios pasos. Es el análogo del Run-Time Engine de TestStand: si un
> generador produce ese formato, funciona sin nada especial.

#### M5-ext.3 — LID (Legacy Isolation Domain) · aplazado

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

**Fin del MVP** ✅ M5. Lo siguiente es post-MVP.

## Cola de la beta (post-MVP, en curso)

Hallazgos de la primera campaña externa
([`qa/informe-beta-2026-08.md`](qa/informe-beta-2026-08.md)), por orden de
arreglo. Lo hecho se marca:

- ✅ **DEF-1** — el sidecar de límites llega a la secuencia del operador bajo
  `--process-model` (#2).
- ✅ **DIAG-1** — aviso cuando un límite del sidecar no afecta a ningún paso (#6).
- ✅ **DEF-3** — el cargador rechaza `asigna`/`statement` sobre un destino no
  declarado, y `asigna` no puede nombrar un `parameter` (#4).
- ✅ **DIAG-2** — **veredicto compuesto** `tipo: pass_fail` (RF-25): el motor
  evalúa una condición booleana sobre variables ya pobladas y falla el paso.
  Cierra el hueco que dejaba 131 de 180 secuencias con un veredicto
  decorativo. → [adr/0018-pass-fail-por-expresion-lo-evalua-el-motor.md](adr/0018-pass-fail-por-expresion-lo-evalua-el-motor.md)
- ✅ **DEF-2** — la primera columna del CSV lleva el nombre de la secuencia
  (antes repetía el estado agregado); la segunda columna (`estado`) pasa a
  llevar ese estado agregado, que antes duplicaba `estado_paso` (#3).
- ✅ **DEF-4** — un `path` absoluto de ejecutor `wasm` da un error que explica
  el sandbox del cargador (solo ve el directorio del YAML), en vez de
  afirmar que el fichero "no existe" cuando sí existe en el host (#5).
- ✅ **DIAG-5** — mensajes que apuntan al campo correcto (#10), en cuatro
  frentes: un sidecar envuelto en `limites:` señala el envoltorio y no el
  nombre del paso; un campo desconocido dice **dónde** está
  (`subsecuencias.interna`, `main[0].limite`) y **qué** se quiso escribir
  (`¿querías 'main'?`); un `.wasm` que es módulo core lo dice en vez de
  «failed to parse WebAssembly module»; y `-h`/`-V` existen. De paso, un YAML
  inválido deja de reportarse dos veces (el host callaba mal: avisaba de «no
  se pudo leer … para los ejecutores» cuando el fichero se leía bien).
- ✅ **DIAG-3** — la **fase** (`setup`/`main`/`cleanup`) por paso en el JSON y
  en el CSV (#8): la sella el motor antes de emitirla al sink; en un
  `sequence_call`, el call lleva la del padre y cada sub-paso la suya. La
  consola no cambia (RNF-08). Era la carencia más citada por los betatesters.
- ✅ **DIAG-4** — bajo `--process-model`, la secuencia del operador viaja como
  campo propio del JSON (`secuencia_usuario`, #9), no sólo dentro de una frase
  del reporte; sin PM la clave se omite.
- ✅ **Contacto de seguridad** (#11) — `SECURITY.md` ordenaba no abrir un issue
  público y remitía a un contacto que no existía, así que un reportero externo
  se habría quedado sin ruta legítima. Ahora apunta a *private vulnerability
  reporting* de GitHub, con un correo de respaldo. El mismo hueco estaba en
  `CODE_OF_CONDUCT.md` (ahí PVR no sirve: un caso de acoso no es una
  vulnerabilidad) y `GOVERNANCE.md` enlaza ambos canales.

La cola de defectos y diagnósticos queda **vacía**.

### La lección de producto (§5 del informe)

Aparte de los `DEF-*`/`DIAG-*`, la §5 del informe dejó tres requisitos que no
eran defectos reportados sino la causa de que 6 de los 8 «bugs» de la campaña
no lo fueran. Se cerraron antes de la segunda ronda de beta, porque
contaminaban los datos de la propia campaña:

- ✅ `resultado.*` fuera de `asigna` es **error de carga** (#12), en vez de un
  `nothing` silencioso que dejaba la secuencia en verde sin correr el paso.
- ✅ El reporte declara **cuántos pasos se saltaron** (#13). Queda abierto el
  `--strict`, que exige decidir qué salto es «inesperado».
- ✅ El alcance de `resultado.*` está donde lo lee un usuario (#14).
- ✅ Y la nota suelta de §3: el ejecutor embebido toma un **puerto efímero**,
  así que una campaña puede paralelizarse lanzando N procesos (#15).

Con la trazabilidad cerrada, `docs/qa/regresion/run.sh` sale **entero en
verde** (13 casos, 0 fallos).

## M6 — El editor declara su paridad · post-MVP ✅ (hecho)

Decidido el 20/09/2026 en
[ADR-0043](adr/0043-the-editor-is-laid-out-as-teststand-and-declares-what-it-does-not-do.md).
La arquitectura de información del editor es la del Sequence Editor de
TestStand 2026Q3, y lo que Anvil no hace sale **en gris, en su sitio**, con uno
de tres veredictos: `todo` (deuda, y nombra la función del motor que la
desbloquea), `elsewhere` (Anvil lo resuelve de otra forma) o `never` (fuera de
alcance por decisión).

La dirección no se invierte: **el motor desbloquea la casilla.** Una función se
implementa y se verifica headless —CLI y test— y sólo entonces el editor deja
de pintarla en gris (AP-13, y AP-04 visto del revés).

- **M6.1 — Los principios y la decisión** ✅ (hecho):
  [diseno/principios-del-editor.md](diseno/principios-del-editor.md) define los
  **AP**, que se citaban 24 veces en `editor/` sin estar escritos en ninguna
  parte; ADR-0043 fija alcance, versión de referencia y los tres veredictos.
- **M6.2 — El inventario en un solo sitio** ✅ (hecho): `editor/src/paridad.mjs`
  (datos puros), el test que impide grises huérfanos, y el generador de
  [paridad-teststand.md](paridad-teststand.md) con `--check` en CI.
- **M6.3 — La ventana principal** ✅ (hecho): barra de menús completa, panel
  Sequences, columnas Step/Description/Settings, grupos de fase con contador,
  Templates, Variables con Value y Type, barra de estado.
- **M6.4 — Ejecución y depuración** ✅ (hecho): pestaña Execution con **Call
  Stack**, **filas para los pasos de una subsecuencia** y pestaña Output.
  Terminate, Abort, Break, Resume, Step y Breakpoints salen en gris nombrando
  lo que esperan. Nada de esto tocó el motor: `--events` ya emitía
  `parent_run_id` y `depth` (ADR-0029, ADR-0033) y nadie los leía.

Inventario a día de hoy: **103 casillas** — 35 construidas, 57 pendientes, 7
por otro camino, 4 descartadas ([paridad-teststand.md](paridad-teststand.md)).

Lo que M6.4 hace visible es la cola del motor que sale de aquí, y que **no**
se implementa en M6:

1. **Cancelación con `cleanup` garantizado** — **diseñada**, sin construir:
   [ADR-0045](adr/0045-terminating-a-run-is-a-request-checked-between-steps.md).
   Terminar es una **petición**, que el motor mira entre pasos y nunca dentro de
   una invocación, y que toma la salida que ya corre `cleanup` en cada nivel de
   la pila. Llega como un byte por stdin —el único canal que los dos hosts ya
   tienen, y que el motor no usa— y la corrida reporta `inconclusive` en vez de
   un verde que nadie se ganó. Ctrl-C pasa a significar eso. No hay Abort.
2. **Parada, inspección y reanudación** (breakpoints + watch + step). Cuatro de
   los seis controles de depuración de TestStand esperan esta misma pieza.
3. **Looping por paso**, **Post Actions** (goto/terminate según veredicto) y
   **Additional Results**.

## M7 — Un solo tipo de ejecutor · ✅ (hecho)

[ADR-0046](adr/0046-an-executor-is-an-address-and-nothing-brings-one-up.md).
Un ejecutor es **una dirección**: `type: wasm` desaparece y nada arranca uno
por ti. Cómo levantarlo en una máquina de desarrollo va en la sección opcional
**`dev:`**, que el motor no lee y que una secuencia de producción no trae.

Lo que se gana: una secuencia deja de llevar dentro el binario de nuestro
ejecutor —la queja que arrancó todo esto—, reescribir un departamento de WASM a
Python no cambia una línea, y desaparece del host toda la maquinaria de
arranque. Lo que cuesta: dos órdenes donde había una, y todas las secuencias
escritas antes de 0.9 dejan de cargar, con un error que dice qué escribir.

Enmienda ADR-0011, 0013, 0014, 0023 y parte de 0027.

## Distribución de los ejecutores de lenguaje — **decisión pendiente**

Anotado el 2026-08-27, sin decidir.
[ADR-0012](adr/0012-executores-de-lenguaje-como-modulos.md) fija que un ejecutor
de lenguaje es un **módulo distribuido y adoptable**, y hoy no lo es:
para usar el de Python hay que clonar el repo entero, copiar dos ficheros a
mano y generar los stubs de protobuf con una herramienta de compilación. La
[quickstart pública](https://anlaco.github.io/quickstart.html) lo confiesa en
letra pequeña. **Cuatro comandos que deberían ser uno.**

Hay además una fragilidad medida: la guía acopla el **binario de un release**
con el **código de `main`**. Si el contrato se moviera entre medias no
casarían. No produciría un resultado equivocado —el eco de contrato de
ADR-0020 §4b lo convierte en `error` ruidoso— pero es una trampa que la
versión de un paquete elimina de raíz.

### Las tres formas, y a quién sirve cada una

| | Qué es | Para quién |
|---|---|---|
| **A. Paquete en PyPI** (`pip install anvil-step`) | El canal estándar de Python. Stubs pregenerados, versión que casa con el contrato. | Quien **escribe** pasos: portátil moderno, con Python y con red. |
| **B. Bundle autocontenido** (con el intérprete dentro, en GitHub Releases) | Descomprimir y ejecutar. Sin Python, sin `pip`, sin red. | Quien **despliega** en un banco. |
| **C. Índice propio** (al estilo de los fabricantes) | Servir nuestro propio repositorio de paquetes. | — |

**No son excluyentes, y probablemente hagan falta las dos primeras**, porque
sirven a personas distintas. Anvil ya hace exactamente eso consigo mismo: un
binario autocontenido en Releases *y* el código en GitHub.

**B es la que encaja con la tesis del producto.** El escenario que justifica
que exista el ejecutor de Python (§ADR-0012) es un banco secuestrado por su SO:
Windows 7/10, drivers del fabricante, sin internet y con un departamento de
sistemas que no deja instalar nada. Exigir `pip install` **ahí** contradice la
premisa por la que existe la pieza. Y es coherente con ADR-0011, que ya decidió
lo mismo para el binario: *«un binario, sin instalar nada»*.

**C conviene mirarla con desconfianza.** Los fabricantes sirven su propio
índice porque venden un ecosistema con licencias y derechos de uso que
controlar. Para un módulo Apache-2.0, PyPI **es** el canal: montar índice
propio añade alojamiento, TLS y disponibilidad, y obliga al usuario a
configurar `pip` para confiar en un tercero — que es justo lo que un
departamento de sistemas bloquea antes que PyPI. Salvo que aparezca una razón
que hoy no está escrita, no compra nada.

### El coste real de B, que es donde está la trampa

Un bundle es **por sistema operativo y arquitectura**, y el caso que importa es
**Windows**, que es donde vive el escenario legacy. Toda la CI de este repo es
`ubuntu-latest`: hoy no hay forma de construir ni de verificar ese artefacto.
Eso, y no el empaquetado, es el trabajo de verdad.

### Qué haría falta decidir antes

1. **Si el ejecutor sigue viviendo en este repo** o se saca a uno propio: cambia
   desde dónde se publica. (Lo hablado el 27/08: quedarse, porque lo único que
   comparten todos los ejecutores es `paso.proto`, y separarlo es donde más se
   desincroniza.)
2. **Cómo se versiona respecto al contrato**: el número de contrato ya existe y
   ya se comprueba en el cable; el del paquete tendría que ser legible al lado.
3. **Que lo subido a PyPI no se borra.** Se puede retirar de la vista, no
   deshacer. Una versión mala se arregla publicando otra.

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
- **Un adaptador de LabVIEW/CVI dentro del motor.** Matizado el 20/09/2026, y
  la distinción importa: lo que queda fuera es que Anvil aprenda qué es un VI,
  como hace TestStand, que inspecciona el connector pane desde el *test
  executive*. Un **ejecutor** LabVIEW —un departamento que arranca VI Server,
  abre un proyecto, enumera los VI públicos y lee sus pines— **no es eso**:
  inspecciona dentro de sí mismo y contesta el mismo `Describe` que un `.wasm`
  (ADR-0025, ADR-0044). El motor sigue sin saber qué hay detrás, que es la
  propiedad entera. Ése es el camino por el que Anvil puede sustituir a
  TestStand sin heredar lo que lo hace pesado.
- Debugger visual completo.

## Procesos diferidos

- `docs/rfcs/` y `docs/proceso-rfc.md`: proceso para cambios del contrato
  `paso.proto` o de la semántica. Se activa cuando haga falta.
- `MAINTAINERS.md`, `.github/CODEOWNERS`: hasta >1 mantenedor.
  (`CHANGELOG.md` ya existe desde v0.1.0.)
- **Activar *private vulnerability reporting*** (Settings → Code security):
  sólo existe en repos **públicos**, y esa era la razón para no tenerlo.
  **Anvil ya es público**, así que es un botón pendiente de pulsar, no un
  bloqueo: hay que activarlo y comprobar que la pestaña *Security* ofrece
  «Report a vulnerability». `SECURITY.md` ya lo da como vía preferente (#11)
  y mientras tanto vale el correo de respaldo.

## Cómo se gestiona el alcance

- Cada hito se vincula a issues cuando arranque su implementación.
- Un alcance que se sale del MVP se mueve explícitamente a post-MVP con un
  ADR si cambia una decisión de fondo.
- La regla rectora, en dos mitades que hay que separar
  ([ADR-0043](adr/0043-the-editor-is-laid-out-as-teststand-and-declares-what-it-does-not-do.md)):

  - **La semántica no se replica 1:1.** Se copia lo bueno (ResultListener,
    perfiles de instrumento, ResultSinks industriales) y se deja fuera lo
    frágil (process model monolítico, callbacks que rompen secuencias
    existentes). Sin cambios: es la regla de siempre.
  - **La arquitectura de información del editor sí es la de TestStand**, y
    declara sus huecos donde van a aparecer, con uno de tres veredictos
    —`todo`, `elsewhere`, `never`—. El inventario vive en
    `editor/src/paridad.mjs` y se publica generado en
    [paridad-teststand.md](paridad-teststand.md).

  Hasta el 20/09/2026 esta regla decía sólo la primera mitad, y el editor ya
  hacía la segunda desde el commit `2a4a6c8`. Lo que la separación protege es
  lo mismo que antes: copiar dónde se busca un ajuste no compromete a copiar
  lo que ese ajuste significa.