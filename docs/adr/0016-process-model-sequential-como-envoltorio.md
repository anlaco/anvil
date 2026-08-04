# ADR-0016: El process model Sequential es un envoltorio de secuencia (reusa M4b); `secuencia_usuario: true` marca la invocación

- **Estado:** Aceptada
- **Fecha:** 2026-08-04 (M5)
- **Relaciona:** ADR-0005, ADR-0010, ADR-0011,
  [proceso-de-test.md](../diseno/proceso-de-test.md),
  [formato-de-secuencia.md](../diseno/formato-de-secuencia.md)

## Contexto

M5 (fin del MVP) pide el **process model Sequential** (RF-38): separar "qué
test correr" (la secuencia del operador) de "cómo se corre en producción"
(identificar UUT, notificar, reportar). La misma secuencia va de I&D a
fábrica cambiando solo el process model — la frase rectora de NI: *"the test
process can change but the tests executed remain the same."*

TestStand resuelve esto con un `ProcessModel.seq` editable con callbacks y
entry points (frágil: tocar un callback rompe secuencias existentes). Anvil
**no** replica ese modelo 1:1 (out-of-scope, RF-N01): el diseño
([proceso-de-test.md](../diseno/proceso-de-test.md)) propone un **Sequential
simple** donde las operaciones comunes son **pasos plug-in** que el motor
corre alrededor de la secuencia del operador.

La maquinaria de **sequence call** (M4b, ADR-0010) ya permite que una
secuencia raíz invoque a otra por path y anide su resultado. La pregunta:
¿cómo se expresa el process model como datos sin tocar el motor, y cómo se
cablea la secuencia del operador (que el usuario pasa por CLI) dentro del
process model?

## Decisión

El **process model es un YAML envoltorio** que se corre con un flag nuevo:

```sh
./anvil --process-model pm.yaml secuencia_del_operador.yaml
```

- El **PM es la raíz** del `Programa`; la **secuencia del operador** se
  inyecta como subsecuencia externa bajo la clave canónica
  `CLAVE_SECUENCIA_USUARIO` (`__anvil_usuario__`).
- El PM la invoca con un paso `secuencia_usuario: true` (campo nuevo en
  `DefinicionPaso`/`PasoYaml`). El cargador reescribe ese paso a
  `secuencia: Some(CLAVE_SECUENCIA_USUARIO)`, así el **motor la resuelve
  como cualquier subsecuencia externa por path** — sin aprender un caso
  nuevo (ADR-0010, ADR-0005: el motor no abre ficheros).
- La frontera PM↔operador se comunica por `asigna`/`locals` (p. ej.
  `asigna: { estado_uut: "${resultado.estado}" }` captura el estado
  agregado del operador). `secuencia_usuario: true` **no admite**
  `secuencia` ni `parametros` (MVP-parcial: sin by-reference en la frontera
  del PM).
- **Sin `--process-model`**, cargar un YAML con `secuencia_usuario: true`
  es error (fail-fast): no hay secuencia usuario que invocar.

### Composición en el cargador (guest-side)

`cargar_programa_con_process_model(pm, usuario)` compone el `Programa` en
el **cargador** (no en el host): coherente con ADR-0005 (el motor re-parsea
el YAML él mismo; no recibe un `Programa` en memoria). El host sólo
recolecta los `ejecutores:` de ambos para la relajación acotada del loopback
(ADR-0011) y pasa el flag al motor.

Fail-fast al cargar:
- El PM debe invocar a la secuencia usuario (algún paso con
  `secuencia_usuario: true`); si no, error.
- La secuencia usuario no puede usar `secuencia_usuario` (sólo el PM).
- Colisiones de claves de subsecuencias externas o de nombres de ejecutores
  entre PM y usuario → error.
- Ciclos: el grafo completo se revalida; la secuencia usuario no puede
  referenciar al PM (su `secuencia_usuario` falla en ese contexto).

### Fases del PM

Reusan las fases de cualquier secuencia (no se inventan fases nuevas):
- **Setup** del PM = process setup (una vez por ejecución).
- **Main** del PM = pre-UUT → call `secuencia_usuario` → post-UUT.
- **Cleanup** del PM = process cleanup (siempre, pase lo que pase).

MVP-parcial: **un UUT, sin loop**. El loop sobre UUTs es post-MVP (ligado
al paralelismo, RF-39).

### Paths `wasm` normalizados al cargar

Los `TipoEjecutor::Wasm { path }` se guardan **normalizados a clave
canónica** (relativos al directorio del YAML que los declara). Antes el
host los re-resolvía contra el directorio de la secuencia usuario; con un
PM eso es otro directorio. Ahora el host los usa tal cual (M5-ext.2,
ADR-0014/0015).

## Alternativas consideradas

- **Nombre reservado `__usuario__` en `secuencia:`** (sin campo nuevo):
  reusa el campo existente, pero colisiona con la convención `es_path` y
  es menos explícito. Se descartó por el campo `secuencia_usuario: true`.
- **Composición en el host** (inyección del `Programa` en memoria): rompe
  ADR-0005 (el motor no recibe un `Programa` del host; re-parsea el YAML).
  Se descartó: el cargador guest-side reusa la maquinaria existente.
- **Callbacks overrideable estilo TestStand** (PreUUT/PostUUT que el
  operador sobrescribe en su secuencia): frágil (contrato implícito) y
  out-of-scope (RF-N01). El PM es **editable** (un YAML que cada grupo
  copia y adapta), no un modelo con callbacks.

## Recortes MVP-parcial (señalados)

- **Un UUT, sin loop** sobre UUTs (post-MVP, ligado a RF-39).
- **Parallel/Batch** (post-MVP).
- **By-value / by-reference transitivo** en la frontera PM↔operador
  (heredado de ADR-0010).
- **Sidecar de límites** (`--limits`) alcanzando a la secuencia usuario
  (heredado de ADR-0010: aplica a la raíz).
- Identificación/notificación reales (scanner, MES, luz de operador): el PM
  de serie simula; los reales son pasos plug-in del usuario.

## Consecuencias

- `paso.proto`/`proto.rs` sin cambios (RNF-05): el ejecutor gRPC no sabe que
  vive en un process model.
- El motor gana un caso mínimo en `ejecuta_sequence_call` (resolver
  `secuencia_usuario` contra `CLAVE_SECUENCIA_USUARIO`); el resto es
  maquinaria M4b intacta.
- El CLI gana `--process-model`, `--help`/`-h` y `--version`/`-V` (RF-40).
- Cada grupo/línea mantiene su PM como un YAML aparte y lo reutiliza con
  distintas secuencias sin tocarlas (patrón plug-in, no callback override).
