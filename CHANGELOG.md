# Changelog

Cambios reseñables de Anvil. El formato sigue
[Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el versionado es
[SemVer](https://semver.org/lang/es/).

Anvil está en 0.x: la superficie pública —el formato de secuencia YAML, el
contrato `paso.proto` y el reporte textual (RNF-08)— puede cambiar entre
minors, con el cambio anotado aquí.

## [No publicado]

**`--validate` deja de decir «válida» a secuencias que no lo son.** El manual
promete que el flag «carga la secuencia, valida el schema, resuelve
subsecuencias y detecta ciclos — sin ejecutar nada ni levantar el ejecutor».
Validaba menos de lo que prometía y conectaba más. Las cinco entradas de abajo
son la misma idea: lo que se puede decidir sin hardware se decide al cargar, no
a mitad de la corrida ni en silencio.

**Hay secuencias que hoy cargan y pasarán a fallar al cargar.** Cada una de
ellas es una definición que ya estaba rota: todas morían en ejecución, sólo que
más tarde y con la unidad medio probada. Un caso concreto del propio repo:
`ejemplos/medir_fuentes.yaml` escribe `parameters.canal`, legítimo porque
`ejemplos/subsecuencia.yaml` la invoca — pero corrida **como raíz** ahora se
rechaza al cargar en vez de morir a mitad.

### Cambiado

- **BREAKING — leer una variable no declarada es error de carga** (issue
  anlaco/Anvil-Test#19). Cualquier lectura de `locals.X` / `parameters.X` /
  `file_globals.X` en `precondicion`, en la `condicion` de un `pass_fail`, o en
  el lado derecho de un `statement` o de un `asigna`, se valida contra las
  declaraciones de **su propia** secuencia. Antes `--validate` decía «válida» y
  la corrida moría a mitad con «no existe 'locals.X'». Es lo que el manual ya
  prometía: los tres scopes son estrictos.
  **No se comprueban tipos** (`bool * número` y compañía): eso no es decidible
  sin evaluar y sigue siendo error de ejecución (ADR-0019, Regla 2).
- **BREAKING — escribir donde no se puede es error de carga** (issue #17).
  `file_globals` es de sólo lectura en cualquier secuencia; `parameters` sólo
  es escribible **desde una subsecuencia**, que es el modo documentado de
  devolver un valor al llamador (ADR-0010) — en la secuencia raíz no hay
  llamador al que devolver nada. Las dos se comprobaban ya en runtime, con
  «sólo locals», y ninguna al cargar.
- **BREAKING — `asigna` desde `resultado.valor_medido` en un `sequence_call` es
  error de carga** (issue anlaco/Anvil-Test#20). Una subsecuencia no mide:
  agrega el veredicto de sus pasos. Ese campo valía siempre `nothing` y borraba
  el destino sin avisar — el mismo fallo silencioso que ADR-0019 arregló para
  los campos inexistentes. `resultado.estado` y `resultado.mensaje` siguen
  siendo válidos.
- **BREAKING — una subsecuencia que declara `ejecutores:` es error de carga**
  (issue anlaco/Anvil-Test#21). Anvil nunca leyó esa sección fuera de la raíz:
  la descartaba en silencio, también cuando contradecía a la de la raíz — el
  caso peor pasaba `--validate` en verde y mandaba los pasos a un ejecutor que
  no era el que su autor había escrito. La tabla se declara una sola vez, en la
  secuencia raíz (con `--process-model`, también en el process model). Aplica a
  subsecuencias externas e inline, y el mensaje de «ejecutor no declarado» dice
  ahora dónde declararlo.

### Arreglado

- **`--validate` ya no levanta los puentes `.wasm`** (issue
  anlaco/Anvil-Test#22). Validar una secuencia con un ejecutor `tipo: wasm`
  spawneaba el proceso `anvil-puente-wasm`, que abría un puerto de loopback e
  imprimía dos líneas por delante del veredicto — para no ejecutar nada, y en
  CI, que es donde el flag existe. El guard que ya evitaba arrancar el ejecutor
  embebido bajo `--validate`, `-h` y `-V` cubre ahora también los puentes. Que
  el `.wasm` declarado **exista** se sigue comprobando: es una comprobación de
  fichero y no requiere instanciar nada.

## [0.2.0] — 2026-08-15

**Anvil deja de dar verde donde no puede juzgar.** Toda la versión sale del
[ADR-0019](docs/adr/0019-que-hace-anvil-cuando-no-puede-juzgar.md), y las tres
entradas de abajo son la misma idea aplicada en tres sitios: un secuenciador de
test existe para emitir un veredicto auditable, así que cuando no puede juzgar
tiene que decirlo, no aprobar.

Es la versión contra la que se corren las rondas de betatesting a partir de
hoy: hasta ahora iban contra un binario del 11/08, anterior a estos arreglos.

### Cambiado

- **BREAKING — el veredicto de una secuencia se agrega por severidad, y hay un
  quinto estado, `inconcluso`** (ADR-0019 Regla 1, issue #31). La escala es
  `paso < inconcluso < fallo < error`; `saltado` sigue siendo neutral y fuera
  de ella. La secuencia agrega **al más severo de sus pasos**, en vez de a
  «`paso` si nadie falló».

  Esto cambia la **superficie pública** en dos sitios, y por eso se anota como
  incompatible:

  - **El vocabulario de estados**: quien consuma el JSON o el CSV tiene que
    contar con `inconcluso` en el estado de la secuencia. No aparece nunca como
    estado de un paso: lo produce el motor al agregar, y sólo él — un ejecutor
    no puede devolverlo ni una secuencia escribirlo.
  - **El reporte textual** (RNF-08), como extensión aditiva igual que el
    `saltado` de M4: la línea de cabecera puede decir ahora
    `=== secuencia: inconcluso ===`. Las líneas de paso no cambian, y una
    corrida sin el caso nuevo produce exactamente los mismos bytes de siempre.

  **Hay secuencias que hoy salen en `paso` y pasarán a `inconcluso`, con exit 1
  en vez de 0.** Es el objetivo, no un efecto colateral: cada una de ellas es
  una unidad que se aprobó sin comprobar. Concretamente, las que declaran al
  menos un paso `tipo: pass_fail` en `main` y no evalúan ninguno —porque se
  saltó por precondición, por `disable`, o porque el Main no llegó a él—. Una
  secuencia cuyo criterio son los `limite` de sus pasos **no cambia de
  comportamiento**: ahí el veredicto sí se evaluó, paso a paso.

  El código de salida sigue siendo binario (0 = `paso`, 1 = todo lo demás): el
  std de `wasm32-wasip2` aplana cualquier `exit(n≠0)` a `I32Exit(1)` al cruzar
  `wasi:cli/run`. La distinción vive en el estado y en el informe.

- **BREAKING — `fallo` es del DUT; `error` es de Anvil** (ADR-0019 Regla 2,
  issues #28 y #27). Lo que antes salía `paso` o `fallo` porque Anvil no supo
  interpretar algo, ahora sale `error`. Son tres cambios de comportamiento:

  - **El vocabulario de estados de un ejecutor pasa a ser cerrado**: `paso`,
    `fallo`, `error` y `saltado`, y nada más. Cualquier otra cadena —`"Paso"`,
    `"PASS"`, y también `"inconcluso"`, que sólo produce el motor al agregar—
    convierte el paso en `error`, con un mensaje que nombra el valor recibido y
    enumera los cuatro válidos. Antes esto era un `fallo` mudo (#28); desde la
    Regla 1 y hasta aquí fue un `paso` mudo, que era peor. Un ejecutor de
    terceros que escribiera mal el estado dejaba pasar unidades sin medir.

    Es **extensión aditiva de RNF-08**, con el precedente del `saltado` de M4:
    el formato de línea no cambia, sólo qué estado aparece en él. Los tests que
    congelan el reporte siguen pasando sin tocarse.

  - **`asigna` no escribe si el paso dio `error`**: sin resultado no hay nada
    que volcar, y lo que hacía antes era machacar con un `nothing` una variable
    con valor bueno que el `cleanup` iba a leer para decidir (#27). Si el paso
    dio `fallo`, la `asigna` sí corre: hay medida, y es justo la que interesa.

  - **Un campo inexistente de `resultado` deja de valer `nothing`**: los campos
    son tres y cerrados (`estado`, `mensaje`, `valor_medido`), así que
    `resultado.valor_meddio` es un typo, no un dato ausente. Al ser comprobable
    sin ejecutar, **lo rechaza el cargador** —y por tanto `--validate`—, no la
    unidad en el banco; en ejecución, la `asigna` falla y el paso queda en
    `error`.

- **`--validate` rechaza `asigna` sobre un paso `statement`** (ADR-0019, regla
  de detección, issue #27). Un `statement` no produce `resultado.*`, así que su
  `asigna` era un no-op silencioso que el validador aprobaba. El caso hermano
  (`pass_fail` con `asigna`) ya se rechazaba desde ADR-0018; ahora los dos dan
  el mismo diagnóstico. **Hay secuencias que hoy cargan y dejarán de cargar**:
  la `asigna` que se les quita nunca hizo nada.

## [0.1.0] — 2026-08-10

Primer release. Cierra el MVP completo (M0 → M5-ext.2) y los hallazgos de la
primera campaña de betatesting externa.

### Añadido

- **Motor de ejecución** genérico dirigido por datos (ADR-0005): fases
  Setup → Main → Cleanup con corte al primer fallo y Cleanup garantizado,
  reintentos por paso, estados `paso`/`fallo`/`error`/`saltado` y agregado
  `error > fallo > paso`.
- **La secuencia es un dato**: schema YAML con validación fail-fast al cargar
  (RF-20). El motor no abre ficheros; el cargador resuelve paths y valida.
- **Pasos por gRPC, por nombre** (`paso.proto`): el motor nunca llama a un
  paso directamente, lo que aísla los pasos y deja escribirlos en cualquier
  lenguaje.
- **ResultSinks desacoplados** con lifecycle: consola (formato congelado,
  RNF-08), JSON y CSV, con reintento de escritura (RF-21/22/23). SQLite
  aplazado (ADR-0007).
- **Step types**: `pass_fail` por expresión (ADR-0018), *limit test* con el
  límite como dato evaluado por el motor (ADR-0008), `action`, `statement`
  local y `sequence_call`.
- **Límites como datos**, embebidos en el YAML o inyectados por un sidecar
  `--limits` por nombre de paso (RF-29/30).
- **Variables y expresiones**: scopes Locals / Parameters / FileGlobals,
  motor de expresiones con sintaxis Julia (`crates/expr`, sin dependencias),
  precondiciones por paso, `disable` y `pause_on_fail` (RF-31/33/34/35).
- **Subsecuencias** (`sequence_call`) inline o en archivo aparte, con
  `parameters` de entrada/salida by-reference, detección de ciclos al cargar
  y resultados anidados (ADR-0010).
- **Process model Sequential** como secuencia envoltorio, sin tocar el motor
  (ADR-0016): `--process-model`, con plug-ins de identificación y
  notificación como pasos gRPC.
- **Adapter SCPI/TCP** para instrumentos reales (`pasos_scpi`, ADR-0017).
- **Binario único** que hospeda wasmtime y los guests WASM en sandbox
  (ADR-0011): `./anvil secuencia.yaml`, sin instalar nada.
- **Routing multi-endpoint** (`ejecutores:`, `--ejecutor`, `--solo-loopback`)
  con relajación acotada del loopback (ADR-0013).
- **Cargador de `.wasm` por path**: un componente con interfaz WIT
  `anvil:paso` se carga como ejecutor; el host levanta un puente que traduce
  gRPC ↔ función (ADR-0014/0015). El `.wasm` del usuario es una función pura,
  sin dependencias de Anvil.
- **CLI headless**: `--json`, `--csv`, `--validate`, `--port`, `--quiet`,
  `--help`/`-h`, `--version`/`-V`.

### Añadido — tras la beta

- La **fase** (`setup`/`main`/`cleanup`) de cada paso en el JSON y como
  última columna del CSV (#8). Un fallo de Setup, uno de Main y uno de
  Cleanup piden respuestas operativas distintas, y eran indistinguibles.
- La **secuencia del operador** como campo propio del JSON bajo
  `--process-model` (`secuencia_usuario`, #9): antes el resultado archivado
  no registraba qué test se corrió.
- El **recuento de pasos saltados** en el resultado (`pasos_saltados` /
  `pasos_totales` en JSON, línea de cierre en consola) (#13). `saltado` sigue
  siendo neutral en el agregado, pero ahora un verde dice cuánto no corrió.
- **Puerto efímero** por proceso para el ejecutor embebido (#15): varios
  `anvil` pueden correr en paralelo. Con `--port`, el puerto fija tanto el
  ejecutor como el motor.

### Corregido — tras la beta

- `resultado.*` fuera del `asigna` de su paso es **error de carga** (#12).
  Antes valía `nothing` en silencio: una precondición que lo usara era un
  `false` constante, el paso se saltaba y **la secuencia terminaba en verde**.
- El sidecar `--limits` llega a la secuencia del operador bajo
  `--process-model` (#2), y avisa cuando no afecta a ningún paso (#6).
- El cargador rechaza `asigna`/`statement` sobre un destino no declarado, y
  `asigna` no puede nombrar un `parameter` (#4).
- La primera columna del CSV lleva el nombre de la secuencia (#3).
- Un `path` absoluto de ejecutor `wasm` explica el sandbox del cargador en
  vez de afirmar que el fichero no existe (#5).
- Diagnósticos que apuntan al campo correcto (#10): sidecar envuelto, campo
  desconocido con ubicación y sugerencia, `.wasm` que es módulo core, y un
  YAML inválido que se reportaba dos veces.
- `--port` fijaba sólo el puerto del motor mientras el host bindeaba 9100,
  así que no servía para lo que la documentación decía (#15).

### Documentación

- `SECURITY.md` y `CODE_OF_CONDUCT.md` publican un canal privado real:
  *private vulnerability reporting* de GitHub con correo de respaldo, y
  correo para conducta (#11). Antes ordenaban no abrir un issue público y
  remitían a un contacto que no existía.
- `docs/diseno/variables-y-alcances.md` documenta el alcance de `resultado.*`
  (#14), que sólo estaba en el plano de implementación.
- Informe de la primera beta y suite de regresión ejecutable en
  `docs/qa/` — 17 casos que afirman el comportamiento correcto.

### Conocido

- **`--strict`** para tratar un salto inesperado como fallo sigue pendiente
  (#13): exige decidir qué cuenta como inesperado.
- **Sin paralelismo** dentro de un proceso (Parallel/Batch es post-v1); la
  vía soportada es lanzar varios `anvil`.
- **Sin UI**: headless por CLI.
- *Private vulnerability reporting* no puede activarse mientras el
  repositorio sea privado; hasta entonces vale el correo de `SECURITY.md`.

[0.2.0]: https://github.com/anlaco/anvil/releases/tag/v0.2.0
[0.1.0]: https://github.com/anlaco/anvil/releases/tag/v0.1.0
