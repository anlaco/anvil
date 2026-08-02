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
| **CSV** | Una fila por paso (nombre, estado, medida, límites). | ✅ M2 |
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