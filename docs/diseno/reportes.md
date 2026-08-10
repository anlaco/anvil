# Diseño: Reportes (ResultSink)

> **Prioridad:** MVP. **Implementado en M2** (consola + JSON + CSV); SQLite
> **aplazado** (ADR-0007). Este doc define el `ResultSink` desacoplado.

El reporte es columna vertebral del secuenciador, no un addon
([investigación](../investigacion/TestStand-y-competencia.md) §1.5). En
Anvil, el resultado se vierte a **ResultSinks** desacoplados, a imagen del
`ResultListener` de OpenTAP (investigación §6), evitando el XSLT opaco y el
schema de BD rígido de TestStand (investigación §2).

## Estado actual

En M2 el motor vierte el resultado a `ResultSink`s. La base sigue siendo el
formato textual **congelado** (RNF-08), ahora como un sink más
(`reporte_a(&mut Write)`, reutilizado por `SinkConsola`):

```
=== basica: fallo ===
  [fallo] medir_voltaje: voltaje fuera de rango
  [paso]  verificar_led: led encendido   # (no llegaría: corta en 1er fallo)
```

El mismo resultado se vierte también a JSON y CSV; SQLite está aplazado
(ADR-0007).

## ResultSink (MVP)

Un `ResultSink` es un consumidor desacoplado con **lifecycle** (estilo
OpenTAP `ResultListener`):

```
on_inicio_secuencia(definicion)
  on_inicio_paso(paso)
  on_resultado(resultado)      # un ResultadoStep
  on_fin_paso(paso)
on_fin_secuencia(resultado)   # el ResultadoSecuencia agregado
```

- Cada sink implementa el lifecycle e ignora lo que no le importa (cuerpos
  vacíos por defecto).
- El motor **no** sabe a quién reporta: publica eventos, los sinks consumen.
- Múltiples sinks activos a la vez (p. ej. consola + JSON + CSV) vía un
  `SinkCompuesto`.

> **Adaptación del lifecycle (M2):** los sinks de formato (consola/JSON/CSV)
> renderizan en `on_fin_secuencia` a partir del `ResultadoSecuencia`
> agregado, porque la cabecera congelada (`=== nombre: estado ===`)
> necesita el estado agregado, que solo se conoce al final. Los hooks de
> streaming (`on_inicio_paso`/`on_resultado`/`on_fin_paso`) se disparan
> igual y quedan listos para sinks de log/UI en vivo futuros; los sinks de
> formato los ignoran.

## Sinks del MVP

| Sink | Qué produce | Estado |
|---|---|---|
| **consola** | El reporte textual congelado actual (compatibilidad, RNF-08). | ✅ M2 |
| **JSON** | Un documento estructurado con la secuencia, pasos y estados. | ✅ M2 |
| **CSV** | Una fila por paso (nombre, estado, fase, medida, límites). | ✅ M2 |
| **SQLite** | Persistencia local para consulta y analítica ligera. | ⏸️ Aplazado (ADR-0007) |

> **Decisión:** el reporte textual congelado se conserva como un sink más,
> no se rompe (RNF-08). Quien dependa del formato de consola actual sigue
> teniéndolo.

> **Extensión aditiva de M4 (RNF-08):** el reporte añade el estado
> `"saltado"` para los pasos saltados por `disable` o precondición falsa
> (RF-33/34). Es un nuevo **valor** de `estado`, no un cambio de formato: la
> línea sigue siendo `  [estado] nombre: mensaje`. `"saltado"` es **neutral**
> en el agregado `error > fallo > paso` (no cuenta como fallo ni error). Los
> sinks JSON/CSV lo muestran como string en `estado_paso`/`estado`. Sin
> campos nuevos en `ResultadoStep` ni en `paso.proto` (ADR-0009).

> **Anidamiento de M4b (RNF-08):** un paso `sequence_call` produce un
> `ResultadoStep` cuyo `estado` es el agregado de la subsecuencia y que lleva
> sus sub-pasos anidados. La consola los **indenta** (+2 espacios por nivel);
> JSON los anida como `sub_pasos` (recursivo); CSV los **aplana** como filas
> extra con `nombre_paso = padre/hijo` **sin añadir columnas** (la cabecera
> congelada no cambia). Los pasos sin sub-pasos producen la misma línea de
> siempre. Sin cambios en `paso.proto` (ADR-0010).

### Visibilidad de los pasos saltados (#13)

`saltado` es **neutral** en el agregado `error > fallo > paso`, y debe seguir
siéndolo: un paso saltado por `disable` o por una precondición falsa no es un
fallo (RF-33/34). Pero esa neutralidad escondía cuánto dejó de correrse — en la
primera campaña de beta, **9 secuencias daban verde saltándose ≥30% de sus
pasos**, y no se detectó hasta auditar los ficheros a mano.

El resultado lleva ahora el recuento, contando el **árbol entero** (los
`sub_pasos` de un sequence call incluidos), porque lo que importa al triar es
cuántos pasos no corrieron, en el nivel que sea:

- **JSON**: `pasos_saltados` y `pasos_totales` en la raíz. Van los dos porque
  el dato útil es el ratio, no el absoluto.
- **Consola**: una línea de cierre, `  (3 de 21 pasos saltados)`, **sólo si
  hubo saltos**. Es una extensión aditiva de RNF-08 en la misma línea que el
  `"saltado"` de M4 y el anidamiento de M4b: las líneas de paso no cambian, y
  una corrida sin saltos produce exactamente los bytes de siempre.
- **CSV**: sin columna nueva. Es un dato de secuencia, no de paso, y se deriva
  contando las filas con `estado_paso = saltado`.

Un `--strict` que trate un salto inesperado como fallo se valoró aparte: exige
decidir qué cuenta como *inesperado* (un `disable` explícito en el YAML lo es;
una precondición falsa, probablemente no), y esa decisión no debía retrasar la
visibilidad. Sigue abierto en #13.

### Trazabilidad: fase y secuencia de operador (#8, #9)

Dos datos que la primera beta externa echó en falta al **post-procesar** los
resultados (DIAG-3 y DIAG-4 del informe):

- **`fase`** (`setup` | `main` | `cleanup`), por paso. Un fallo de Setup (el
  DUT no se pudo ni conectar), uno de Main (el DUT falló el test) y uno de
  Cleanup (el equipo pudo quedar en un estado no seguro) piden respuestas
  operativas distintas, y hasta ahora eran indistinguibles.

  La sella el **motor**, que es quien conoce la fase en curso, antes de emitir
  el resultado al sink — así un sink de streaming la ve ya puesta. No viaja en
  `paso.proto` (el paso no sabe en qué fase corre), igual que
  `valor_esperado`/`operador` bajo ADR-0008. En un `sequence_call`, el paso de
  la llamada lleva la fase del **padre** y cada sub-paso la suya dentro de la
  subsecuencia.

  Sale en **JSON** (clave `fase` por paso, también en los `sub_pasos`) y en
  **CSV** (columna `fase`, añadida **al final** para no mover las diez
  originales). **La consola no cambia**: el formato textual sigue congelado
  (RNF-08), y meter la fase ahí exigiría un ADR aparte.

- **`secuencia_usuario`**, en la raíz del documento JSON. Con
  `--process-model`, la secuencia raíz es el PM (`sequential`), así que
  `secuencia` no dice qué test se corrió — y en producción el PM es
  obligatorio. El CLI, que conoce la ruta inyectada en el PM, se la pasa al
  sink JSON. **Sin `--process-model` la clave se omite** (no va como `null`):
  la raíz ya es la secuencia del usuario. En consola y en el `mensaje` del
  paso ya aparecía desde M5; el campo propio es lo que faltaba para
  post-procesar.

## Reintento y reconexión (MVP-parcial)

TestStand sufre que la conexión de BD cacheada **rompe con corte de red sin
auto-retry** (investigación §2). El `ResultSink` que escribe a red/BD
(SQLite local no, pero un futuro Postgres/STDF-stream sí) debe:

- **Reintentar** escrituras transitorias con backoff.
- **Reconectar** tras corte, sin perder resultados del medio (cola local
  como respaldo).
- No bloquear la ejecución de la secuencia: el sink va en su propio hilo/
  tarea (post-MVP con paralelismo; en el MVP, best-effort tras cada paso).

## ResultSinks sectoriales (post-MVP)

- **STDF** (semiconductora) y **ATML** (aerospace) como sinks first-class
  (RF-24).
- **Parquet/DuckDB** para resultados como dato abierto consultable
  (investigación §6, copiar de Litmus).

## No es el contrato

Los `ResultadoStep` que el sink consume son los mismos del motor; el sink
**no** va por `paso.proto` (eso es la frontera motor↔ejecutor). El sink está
en el lado del motor y consume `ResultadoSecuencia`/`ResultadoStep` Rust.

## Out-of-scope

- Plantillas XSLT (lo que la gente sufre en TestStand) — no.
- Schema de BD fijo impuesto — el SQLite del MVP es esquema simple y
  consultable, no rígido.