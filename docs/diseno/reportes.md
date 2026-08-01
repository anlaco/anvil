# Diseño: Reportes (ResultSink)

> **Prioridad:** MVP. **Propuesta** (hoy el reporte es un `println!`
> congelado; este doc define el `ResultSink` desacoplado que lo reemplaza).

El reporte es columna vertebral del secuenciador, no un addon
([investigación](../investigacion/TestStand-y-competencia.md) §1.5). En
Anvil, el resultado se vierte a **ResultSinks** desacoplados, a imagen del
`ResultListener` de OpenTAP (investigación §6), evitando el XSLT opaco y el
schema de BD rígido de TestStand (investigación §2).

## Estado actual

`ResultadoSecuencia::reporte` imprime a consola un formato textual
**congelado** (RNF-08):

```
=== basica: fallo ===
  [fallo] medir_voltaje: voltaje fuera de rango
  [paso]  verificar_led: led encendido   # (no llegaría: corta en 1er fallo)
```

Es la base; el objetivo es verter el mismo resultado a **múltiples sinks**.

## ResultSink (propuesta, MVP)

Un `ResultSink` es un consumidor desacoplado con **lifecycle** (estilo
OpenTAP `ResultListener`):

```
on_inicio_secuencia(secuencia)
  on_inicio_paso(paso)
  on_resultado(resultado)      # un ResultadoStep
  on_fin_paso(paso)
on_fin_secuencia(secuencia, estado_agregado)
```

- Cada sink implementa el lifecycle e ignora lo que no le importa.
- El motor **no** sabe a quién reporta: publica eventos, los sinks consumen.
- Múltiples sinks activos a la vez (p. ej. consola + JSON + SQLite).

## Sinks del MVP

| Sink | Qué produce | MVP |
|---|---|---|
| **consola** | El reporte textual congelado actual (compatibilidad). | ✅ |
| **JSON** | Un documento estructurado con la secuencia, pasos y estados. | ✅ |
| **CSV** | Una fila por paso (nombre, estado, medida, límites). | ✅ |
| **SQLite** | Persistencia local para consulta y analítica ligera. | ✅ |

> **Decisión:** el reporte textual congelado se conserva como un sink más,
> no se rompe (RNF-08). Quien dependa del formato de consola actual sigue
> teniéndolo.

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