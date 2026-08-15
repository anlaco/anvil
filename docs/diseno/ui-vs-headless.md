# Diseño: UI vs. headless

> **Prioridad:** MVP-parcial. **Headless/CLI en el MVP**; Operator UI web +
> UIMsgs + editor visual son post-MVP.

Anvil nace **headless primero**: se corre con `wasmtime run anvil.wasm
secuencia.yaml`. Sin UI gráfica en v1. La UI llega después, cuando el
núcleo sea estable. Esto también evita el dolor de TestStand: una UI
acoplada al motor que se queda atrás (Sequence Editor dev vs. Operator
Interfaces prod desincronizados).

## MVP: headless/CLI

- El motor corre como un `.wasm` bajo wasmtime; la salida es el ResultSink
  de consola (ver [reportes.md](reportes.md)).
- Las "opciones" de ejecución (pause-on-fail, step, disable) son flags CLI o
  campos del YAML (ver [motor-de-ejecucion.md](motor-de-ejecucion.md)).
- Determinismo: sin UI, la ejecución es reproducible (RNF-03).

### CLI maduro (M5, RF-40)

El CLI `anvil` (`crates/motor/src/bin/anvil.rs`, también distribuido como
binario único que hospeda wasmtime, ADR-0011) soporta:

```
anvil <secuencia.yaml> [--process-model <pm.yaml>] [--json <ruta>] \
  [--csv <ruta>] [--limits <ruta>] [--port <n>] [--validate] [--quiet] \
  [--help] [--version]
```

- `--process-model <ruta>` envuelve la secuencia en un PM Sequential
  (RF-38, ADR-0016). Sin él, la secuencia corre tal cual.
- `--validate` carga y valida el programa (schema, lvalues, firmas,
  ciclos) sin ejecutar ni conectar al ejecutor — útil en CI sin hardware.
- `--port <n>` fija el puerto del ejecutor embebido: el que bindea el ejecutor
  y el que busca el motor, que reintenta la conexión si no está listo (5 s
  máx). En el binario único el host ya espera al ejecutor. **Sin el flag**, el
  host reserva un puerto efímero por proceso, para que varios `anvil` puedan
  correr en paralelo (#15); el guest ejecutor suelto sigue usando 9100 por
  defecto, que es lo que asume el flujo de dos terminales.
- `--quiet` silencia el reporte de consola y los logs informativos de
  stderr; los errores y los exit codes se preservan (RNF-08: el formato
  congelado se omite, no se cambia). JSON/CSV siguen emitiéndose.
- `--help`/`--version` salen antes de cargar/conectar.

### Exit codes (#16)

El contrato del binario es **binario**:

| Código | Significa |
|---|---|
| `0` | la secuencia corrió y el veredicto agregado es `paso` |
| `1` | cualquier otra cosa: veredicto `fallo`, `error` o `inconcluso`, error de carga, error de uso, ejecución interrumpida |

El veredicto sale de `ResultadoSecuencia::estado()`, que agrega **al paso más
severo** en la escala `paso < inconcluso < fallo < error` (ADR-0019, Regla 1).
`saltado` queda fuera de la escala y es neutral (RF-33/34: un paso saltado por
`disable` o por precondición falsa no es un fallo). `--quiet` no lo altera:
silencia el reporte, no el veredicto.

`inconcluso` es el estado que produce el motor cuando la secuencia declara un
veredicto (`tipo: pass_fail` en `main`) y ninguno llega a evaluarse — issue #31,
donde una unidad salía aprobada sin que nadie la midiera. **Sale 1**, como todo
lo que no es `paso`. El `if` que lo decide niega `"paso"` en vez de enumerar los
estados malos, precisamente para que un estado nuevo no se cuele como éxito: por
eso este cambio de semántica no tocó una línea del cálculo del exit code.

Es lo que un pipeline necesita —distinguir «pasó» de «no pasó»— y es todo lo
que la plataforma permite. Es también donde nos quedamos cortos frente a
OpenTAP (`tap run` devuelve 20 para `Inconclusive`) y a pytest (exit 5 para «no
se recogió ningún test»): la distinción entre «no cumple» y «no se pudo juzgar»
vive en el estado y en el informe, no en el código de salida. **No hay códigos
granulares, y no pueden haberlos hoy**: el std de Rust en `wasm32-wasip2` aplana cualquier
`process::exit(n≠0)` a `I32Exit(1)` al cruzar `wasi:cli/run`, y esa interfaz
devuelve `result<_, _>`, sin código. El propio `exit(2)` que el guest usa para
el error de uso se ve como `1` a través del host (`anvil --flag-inventado` → 1);
sólo llega intacto corriendo el guest suelto o compilado nativo. Un esquema
0/1/2/3 exigiría un canal nuevo entre guest y host, y eso sería un ADR.

El contrato está fijado por `packaging/anvil-host/tests/exit_codes.rs`, que
lanza el binario real: es la única forma de observar el aplanamiento — un test
contra el motor nativo pasaría en verde sin probar nada de esto.

Parseo manual, sin `clap`/`getopts`: el flag set es pequeño y se evita
peso en el `.wasm` (ADR-0001). Si el flag set crece > ~10 o aparecen
subcomandos, se reconsidera con un ADR (post-MVP). El host embebido
hereda los args al guest motor, así los flags fluyen al binario único.

## Desacoplo motor ↔ UI: UIMsgs (post-MVP)

TestStand desacopla el motor de la UI de operador con *User Interface
Messages*: el motor postea mensajes (trace, estado, errores) que cualquier
Operator Interface consume; los no soportados se ignoran (investigación
§1.6). Anvil adopta la misma idea **post-MVP**:

- El motor emite eventos; una UI web los consume.
- Así una UI corre cualquier secuencia y viceversa, sin acoplar el motor a
  un toolkit gráfico (coherente con WASM, ADR-0001: el motor sigue siendo un
  `.wasm` sin toolkit).

## Operator UI web (post-MVP)

- UI web de operador (copiar de Litmus/OpenHTF, investigación §3/§6).
- **Roles**: admin / engineer / technician / operator con login separado
  del SO (estándar en ATE comercial: Astronics/Advantest, ProDSP —
  investigación §5, Could).
- Lee el estado del motor por los UIMsgs; no lo acopla.

## Editor visual (post-MVP) — con drag-and-drop e introspección de firma

Cuando Anvil tenga editor visual, el objetivo es:

1. **Drag-and-drop del archivo** del code module (`.vi`/`.dll`/`.py`/
   `.scilab`) sobre el editor.
2. El editor **auto-descubre y actualiza los parámetros y el valor de
   retorno** del paso a partir de la firma del módulo, como hace TestStand
   al añadir un code module.

Esto exige que un paso **exponga su firma** (parámetros: nombre, tipo,
dirección in/out; tipo de retorno). Hoy `paso.proto` solo describe *cómo
invocar* y *qué devuelve* a nivel de mensaje, no la firma tipada. Hay que
añadir un **mecanismo de introspección** (p. ej. un RPC `Describe` o un
sidecar de metadatos) — extensión futura del contrato, detallada en
[contrato-grpc.md](../contrato-grpc.md) y ligada al registro de pasos
([modelo-de-pasos.md](modelo-de-pasos.md)).

> **Tensión resuelta:** la introspección de firma vive en el **lado del
> ejecutor** (que provee el catálogo de pasos y sus firmas), no en el
> núcleo del motor (que sigue genérico, ADR-0005). El editor y el ejecutor
> hablan firmas; el motor sigue hablando solo `nombre`/`estado`.

## Por qué headless primero

- El núcleo (semántica, reintentos, contrato, ResultSinks) es lo que
  diferencia a Anvil; la UI no (Flojoy ya tiene editor visual AGPL,
  OpenTAP editor comercial — investigación §3). La UI no es la tesis
  (ver [vision.md](../vision.md)).
- Headless permite CI sin hardware (record/replay, ver
  [integracion-instrumentos.md](integracion-instrumentos.md)) y
  determinismo desde el día 1.

## Out-of-scope

- Editor visual en el MVP (es post-MVP, ligado a introspección de firma).
- Debugger visual completo.
- UI atada a un toolkit de escritorio (la UI es web, no nativa).