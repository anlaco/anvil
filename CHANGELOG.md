# Changelog

Cambios reseñables de Anvil. El formato sigue
[Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el versionado es
[SemVer](https://semver.org/lang/es/).

Anvil está en 0.x: la superficie pública —el formato de secuencia YAML, el
contrato `paso.proto` y el reporte textual (RNF-08)— puede cambiar entre
minors, con el cambio anotado aquí.

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

[0.1.0]: https://github.com/anlaco/anvil/releases/tag/v0.1.0
