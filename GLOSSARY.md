# Glosario: la interfaz de Anvil, en inglés

Anvil aspira a que su formato de secuencia sea un estándar. Ninguno de los
formatos que lo son —HTML, ODF, OOXML— se escribió en otra lengua que el
inglés, y el vocabulario de este dominio (*pass*, *fail*, *limit*, *step*,
*setup*, *cleanup*) ya está en inglés en la cabeza de quien viene de TestStand
o de OpenTAP. Escribirlo en castellano no ahorra una traducción: la añade.

Este fichero fija esa traducción **una sola vez**, para que no se improvise en
cada sitio y acaben conviviendo `executor` y `runner` para lo mismo.

## La frontera

> **Todo lo que entra al repo, en inglés.**

La frontera se movió: primero era «la interfaz en inglés, lo de dentro en
castellano», y desde el 28/08/2026 lo son también el código, los ADRs, `docs/`
y los mensajes de commit. Este fichero sigue existiendo porque su trabajo no
era marcar esa frontera, sino **fijar la traducción de cada término del dominio
una sola vez**, para que no convivan `executor` y `runner` para lo mismo. Se
consulta **antes** de nombrar algo nuevo en la superficie pública.

La migración es *boy scout* y no retroactiva: un fichero entero en castellano
hoy no es una anomalía, es el punto de partida. La decisión está en
`CONTRIBUTING.md`.

El sitio donde se cruza la frontera ya existe y tiene nombre:
`PasoYaml::a_definicion()` en `crates/cargador`, que traduce lo que se
deserializa al modelo interno.

## El diccionario

### Claves de la secuencia

| Antes | Ahora |
|---|---|
| `nombre` | `name` |
| `subsecuencias` | `subsequences` |
| `ejecutores` | `executors` |

Sin cambio: `setup`, `main`, `cleanup`, `locals`, `parameters`,
`file_globals`.

### Claves del paso

| Antes | Ahora |
|---|---|
| `nombre` | `name` |
| `reintentos` | `retries` |
| `limite` | `limit` |
| `tipo` | `type` |
| `precondicion` | `precondition` |
| `asigna` | `assign` |
| `condicion` | `condition` |
| `secuencia` | `sequence` |
| `parametros` (paso `grpc`) | **`inputs`** |
| `parametros` (`sequence_call`) | **`args`** |
| `ejecutor` | `executor` |

Sin cambio: `disable`, `pause_on_fail`, `statement`, y los valores de `type`
(`grpc`, `statement`, `sequence_call`, `pass_fail`).

**Por qué `parametros` se parte en dos.** Significaba dos cosas —los valores
*by-value* que se envían a un paso (ADR-0020) y los argumentos
*by-reference* de un `sequence_call` (ADR-0010)— y además chocaba con el scope
`parameters` de la secuencia. Traducir las dos a `parameters` habría hecho el
problema peor: tres cosas con el mismo nombre. Con `inputs` y `args`, copiar un
bloque de un sitio al otro deja de poder cambiar el significado en silencio.

### Límite

| Antes | Ahora |
|---|---|
| `tipo: rango` | `type: range` |
| `tipo: comparacion` | `type: comparison` |
| `esperado` | `expected` |

Sin cambio: `min`, `max`, `op` y los operadores (`ge`, `le`, `gt`, `lt`, `eq`,
`ne`).

### Ejecutores

| Antes | Ahora |
|---|---|
| `tipo: embebido` | `type: embedded` |
| `puerto` | `port` |

Sin cambio: `host`, `path`, `tipo: wasm|grpc`.

### Lenguaje de expresiones

| Antes | Ahora |
|---|---|
| `resultado.<campo>` | `result.<field>` |
| `resultado.estado` | `result.status` |
| `resultado.mensaje` | `result.message` |
| `resultado.valor_medido` | `result.measured_value` |
| `resultado.salidas.<n>` | `result.outputs.<n>` |

Sin cambio: los scopes `locals`, `parameters`, `file_globals`; los operadores
(`&&`, `||`, `!`, `==`…) y el literal `nothing`.

### Estados

| Antes | Ahora |
|---|---|
| `paso` | `pass` |
| `fallo` | `fail` |
| `error` | `error` |
| `saltado` | `skipped` |
| `inconcluso` | `inconclusive` |

Son vocabulario **cerrado**: cualquier otra cadena que devuelva un ejecutor
convierte el paso en `error` (ADR-0019, Regla 2). Viven a la vez en el WIT, en
`paso.proto`, en el JSON, en el CSV y en el reporte, así que se mueven juntos o
el contrato miente.

### Contrato gRPC (`paso.proto`)

| Antes | Ahora |
|---|---|
| `PeticionPaso` | `StepRequest` |
| `ResultadoPasoProto` | `StepResult` |
| `Valor` | `Value` |
| `EjecutorPasos` / `Invoca` | `StepExecutor` / `Invoke` |
| `intento` | `attempt` |
| `parametros` | `inputs` |
| `contrato` | `contract` |
| `valor_medido` | `measured_value` |
| `limite_min` / `limite_max` | `limit_min` / `limit_max` |
| `salidas` | `outputs` |
| `numero` / `texto` / `booleano` | `number` / `text` / `boolean` |

Términos nuevos del contrato 4 (ADR-0022), que nacen ya en inglés:

| Término | Qué es |
|---|---|
| `Reference` | El handle a un objeto que se queda en el ejecutor. **Referencia**, nunca *handle* ni *puntero* en la superficie pública. |
| `executor` (de una `Reference`) | El nombre que la secuencia le da al ejecutor dueño. Lo estampa Anvil. |
| `lifetime` | La **vida** del ejecutor: la acuña él al arrancar y la publica en su `Catalog`. Nunca *session* ni *epoch*. |
| `payload` | La parte opaca que acuña el ejecutor. Nunca *id*, que sugeriría que Anvil la interpreta. |
| `type: reference` | Cómo se declara una variable de referencia en `locals:`. |

### WIT

`step-result` y no `result`, y `type` con `#[serde(rename)]` en Rust: las dos
son concesiones a palabras reservadas del lenguaje, no al idioma.

| Antes | Ahora |
|---|---|
| `anvil:paso@0.2.0` | `anvil:step@0.3.0` |
| `run(nombre, intento, parametros)` | `run(name, attempt, inputs)` |
| `record resultado` | `record step-result` |
| `record nombrado` | `record named` |
| `variant valor` | `variant value` |
| `valor-medido` | `measured-value` |

### Informes

| Antes | Ahora |
|---|---|
| `secuencia` / `secuencia_usuario` | `sequence` / `user_sequence` |
| `pasos` / `sub_pasos` | `steps` / `sub_steps` |
| `pasos_saltados` / `pasos_totales` | `skipped_steps` / `total_steps` |
| `nombre_secuencia` / `nombre_paso` | `sequence_name` / `step_name` |
| `estado_paso` | `step_status` |
| `valor_esperado` / `operador` | `expected_value` / `operator` |
| `fase` | `phase` |
| `parametros` / `salidas` | `inputs` / `outputs` |

Sin cambio: `setup`, `main`, `cleanup` como valores de `phase`.

### CLI

| Antes | Ahora |
|---|---|
| `--ejecutor` | `--executor` |
| `--solo-loopback` | `--loopback-only` |

Sin cambio: `--json`, `--csv`, `--limits`, `--validate`, `--process-model`,
`--port`, `--wasm`, `--quiet`.

## Lo que todavía está en castellano y acabará en inglés

Los **mensajes de error y el reporte de texto** son interfaz de usuario y
acabarán traducidos, pero eso es traducir prosa y no identificadores: entra por
la regla del *Boy Scout* —cada fichero que se abra para modificarlo se traduce,
en un commit aparte del cambio que motivó abrirlo— y no de golpe.
