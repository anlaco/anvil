# Diseño: UI vs. headless

> **Prioridad:** MVP-parcial. **Headless/CLI en el MVP**; Operator UI web +
> UIMsgs son post-MVP. El **editor de secuencias existe desde 0.4.0** y su
> paridad con TestStand la gobierna
> [ADR-0043](../adr/0043-the-editor-is-laid-out-as-teststand-and-declares-what-it-does-not-do.md).

Anvil nació **headless primero**: se corre con `anvil secuencia.yseq`, y el
MVP entero (M0–M5) se cerró sin una sola pantalla. La UI llegó después, con el
núcleo ya estable. Eso evita el dolor de TestStand —una UI acoplada al motor
que se queda atrás, Sequence Editor de desarrollo contra Operator Interfaces de
producción desincronizados— y se sostiene con una regla, no con una intención:
**hay un solo motor y las dos front-ends son clientes suyos**
([ADR-0031](../adr/0031-one-engine-two-front-ends.md),
[ADR-0034](../adr/0034-the-engine-is-a-service-and-the-front-ends-are-clients.md)).
El editor hospeda el mismo `anvil-guest.wasm` que embebe el binario nativo; no
hay un fork del motor para la pantalla, y no puede haberlo.

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
  [--csv <ruta>] [--limits <ruta>] [--executor n=host:puerto] [--validate] [--quiet] \
  [--help] [--version]
```

- `--process-model <ruta>` envuelve la secuencia en un PM Sequential
  (RF-38, ADR-0016). Sin él, la secuencia corre tal cual.
- `--validate` carga y valida el programa (schema, lvalues, firmas,
  ciclos) sin ejecutar ni conectar al ejecutor — útil en CI sin hardware.
- `--executor <nombre>=<host>:<puerto>` re-apunta un ejecutor declarado. El
  motor se conecta sólo a los ejecutores `grpc` que declara la secuencia y
  reintenta la conexión si no están listos (5 s máx); los `type: wasm` los
  arranca el host en un puerto efímero por proceso y se los pasa así, de modo
  que varios `anvil` pueden correr en paralelo (#15). `--port` ya no existe:
  fijaba el puerto del ejecutor embebido, retirado con
  [ADR-0041](../adr/0041-there-is-no-embedded-executor.md).
- `--quiet` silencia el reporte de consola y los logs informativos de
  stderr; los errores y los exit codes se preservan (RNF-08: el formato
  congelado se omite, no se cambia). JSON/CSV siguen emitiéndose.
- `--help`/`--version` salen antes de cargar/conectar.

### Exit codes (#16)

El contrato del binario es **binario**:

| Código | Significa |
|---|---|
| `0` | la secuencia corrió y el veredicto agregado es `pass` |
| `1` | cualquier otra cosa: veredicto `fail`, `error` o `inconclusive`, error de carga, error de uso, ejecución interrumpida |

El veredicto sale de `ResultadoSecuencia::estado()`, que agrega **al paso más
severo** en la escala `pass < inconclusive < fail < error` (ADR-0019, Regla 1).
`skipped` y `done` quedan fuera de la escala y son neutrales (RF-33/34: un paso
saltado por `disable` o por precondición falsa no es un fallo; y un `action` o
un `statement` no juzgaron nada, ADR-0040 §6). Una secuencia entera de pasos
`done` agrega a `pass` y **sale 0**: nada dijo que la unidad estuviera mal. `--quiet` no lo altera:
silencia el reporte, no el veredicto.

`inconcluso` es el estado que produce el motor cuando la secuencia declara un
veredicto (`type: pass_fail` en `main`) y ninguno llega a evaluarse — issue #31,
donde una unidad salía aprobada sin que nadie la midiera. **Sale 1**, como todo
lo que no es `pass`. El `if` que lo decide niega `"pass"` en vez de enumerar los
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
subcomandos, se reconsidera con un ADR (post-MVP). El host
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

## El editor de secuencias — existe, y declara lo que no hace

Ya no es un plan. El editor abre, edita, valida en vivo, guarda y **corre**
(`editor/`), es una SPA que se envuelve en Electron para la descarga
([ADR-0037](../adr/0037-the-editors-shell-is-electron-because-linux-is-the-first-platform.md)),
y sus reglas de diseño —los **AP**— están en
[principios-del-editor.md](principios-del-editor.md).

Su forma es la del Sequence Editor de TestStand 2019, y **lo que Anvil no hace
sale en gris en su sitio**, diciendo qué hace TestStand ahí y si es deuda
(`todo`), otro camino (`elsewhere`) o una decisión (`never`). Así el inventario
de lo que falta vive donde no puede quedarse obsoleto. El mecanismo y su
alcance están en
[ADR-0043](../adr/0043-the-editor-is-laid-out-as-teststand-and-declares-what-it-does-not-do.md);
el inventario publicado, en [paridad-teststand.md](../paridad-teststand.md).

La dirección importa: **el motor desbloquea la casilla, nunca al revés**
(AP-13). El editor no puede pedirle al motor algo que el motor no haga ya, que
es AP-04 — la regla de la que cuelga todo lo demás.

### La introspección de firma: resuelta, y no como decía este documento

El objetivo era el de TestStand: arrastrar el code module sobre el editor y que
**los parámetros y el retorno se descubran solos**. Este documento suponía que
haría falta *inspeccionar* el módulo, como TestStand lee el connector pane de
un `.vi`.

Lo que se hizo fue **preguntar**, no inspeccionar: el RPC `Describe` del
contrato
([ADR-0021](../adr/0021-el-ejecutor-describe-su-catalogo.md),
[ADR-0028](../adr/0028-describe-answers-without-the-bench.md)) devuelve el
catálogo del ejecutor con la firma de cada paso. Preguntar funciona igual para
un WASM, para Python, para una caja en otra sala y para lo que venga después
(`crates/motor/src/catalogo.rs:11-17`).

> **Tensión resuelta:** la firma vive en el **lado del ejecutor**, que es quien
> provee el catálogo, no en el núcleo del motor, que sigue genérico
> (ADR-0005). El editor y el ejecutor hablan firmas; el motor sigue hablando
> sólo `nombre`/`estado`.

El drag-and-drop del módulo sobre el editor es lo que sigue pendiente, y es una
casilla del inventario, no un párrafo de este documento.

## Por qué headless primero

- El núcleo (semántica, reintentos, contrato, ResultSinks) es lo que
  diferencia a Anvil; la UI no (Flojoy ya tiene editor visual AGPL,
  OpenTAP editor comercial — investigación §3). La UI no es la tesis
  (ver [vision.md](../vision.md)). **Eso sigue siendo cierto con el editor ya
  construido**, y es la razón de que ADR-0043 §6 ponga al motor a marcar el
  ritmo: una cadencia dirigida por la pantalla implementa lo que es barato de
  dibujar y aplaza lo caro que sí importa.
- Headless permite CI sin hardware (record/replay, ver
  [integracion-instrumentos.md](integracion-instrumentos.md)) y
  determinismo desde el día 1.

## Out-of-scope

- Editor visual **en el MVP**: lo fue, y el MVP se cerró sin él. Existe desde
  0.4.0 y va por M6 (ver [roadmap.md](../roadmap.md)).
- **Debugger visual completo.** Sigue fuera, y ahora se ve: la ventana de
  ejecución enseña Terminate, Breakpoints, Watch y Step **en gris**, y cuatro
  de esos cinco esperan la misma pieza del motor —parar, mirar y seguir—, que
  no está escrita. Terminate espera además la cancelación con `cleanup`
  garantizado, que es el mayor riesgo declarado del editor hoy
  (`editor/README.md`).
- Operator Interfaces, Type Editor, Deployment Utility, integración con control
  de versiones y Sequence Analyzer: fuera del alcance de paridad (ADR-0043 §3).
- UI atada a un toolkit de escritorio (la UI es web, no nativa).