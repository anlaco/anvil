# ADR-0019: Qué hace Anvil cuando no puede juzgar

- **Estado:** Propuesta *(escrita desde dirección el 2026-08-13; decide el
  responsable del repo)*
- **Fecha:** 2026-08-13
- **Relaciona:** ADR-0002, ADR-0005, ADR-0008, ADR-0009, ADR-0018,
  RNF-08 ([requisitos.md](../requisitos.md)),
  issues #22, #23, #26, #27, #28, #31

## Contexto

Seis de los veintitrés issues abiertos son el mismo defecto con seis caras.
Todos verificados ejecutando el binario, el 2026-08-13:

| | Qué se traga en silencio | Qué devuelve |
|---|---|---|
| #27 | Un typo en `asigna` (`resultado.valor_meddio`) destruye una variable | secuencia entera en **`paso`** |
| #31 | El `pass_fail` que hace de veredicto se salta por precondición | **`paso`** y **exit 0** |
| #28 | Un ejecutor devuelve `"Paso"` con mayúscula | **`fallo`** mudo |
| #26 | El sidecar convierte `comparacion le 90.0` en `rango [0,500]` | `fallo` → **`paso`**, y el JSON pierde `operador` y `valor_esperado` |
| #22 | `--limits` repetido: aplica el último | sin aviso |
| #23 | Una precondición salta un paso de `cleanup` de seguridad | sin aviso |

Por separado cada uno parece un descuido. Juntos son una postura: **cuando
Anvil no puede juzgar algo, hoy adivina, y adivina hacia el verde.**

El caso #31 lo enseña entero, y es el más grave porque cada pieza es correcta:

- Un paso `saltado` es neutral en el agregado — correcto, y documentado (§5).
- Una secuencia sin fallos agrega a `paso` — correcto.
- Desde #16, el código de salida refleja el veredicto: `paso` → 0 — correcto.

Tres decisiones correctas encadenadas producen **una unidad que sale de la línea
marcada como buena sin que nadie la haya medido**, y un pipeline que la aprueba.
No hay ningún componente que arreglar: hay una regla que no está escrita.

Esa regla es lo que decide este ADR. Sin ella, los seis issues se arreglan por
separado, cada uno con el criterio de quien lo toque, y el resultado es una
semántica incoherente que habrá que volver a unificar más tarde y más caro.

### Por qué importa aquí más que en otro software

Anvil no calcula un resultado: **emite un veredicto sobre una unidad física que
alguien va a enviar, montar o certificar**. El producto no es la ejecución, es
la afirmación que queda escrita. Un secuenciador que no distingue «cumple» de
«no lo he comprobado» no tiene un bug de usabilidad: no sirve para lo que se
usa.

Y aquí hay una oportunidad que la investigación de este ADR destapó, así que
conviene decirla en el contexto y no sólo al final: **NI TestStand tiene este
mismo agujero después de veinte años**, y taparlo es trabajo del usuario. No es
un terreno donde haya que alcanzar al líder; es uno donde el líder deja algo sin
resolver que a sus usuarios les cuesta unidades mal aprobadas.

## Decisión

Tres reglas. Todo lo demás se deriva de ellas.

### Regla 1 — `paso` es una afirmación, no un valor por defecto

Anvil sólo puede decir `paso` de lo que ha comprobado. Para poder decirlo hacen
falta dos cosas que hoy no existen: **un estado para «no lo sé» y una
agregación por severidad.**

**Se añade un quinto estado, `inconcluso`**, con esta severidad:

```
paso  <  inconcluso  <  fallo  <  error
```

y `saltado`, que sigue siendo neutral, fuera de la escala.

**La secuencia agrega al más severo de sus pasos**, en vez de a «`paso` si nadie
falló». Es el algoritmo de OpenTAP, y es lo que impide que una ausencia de
información se convierta en una afirmación.

`inconcluso` lo produce, de momento, un solo caso: **un `pass_fail` que hace de
veredicto y no llega a evaluarse** (#31). El paso se sigue reportando como
`[saltado]` —es lo que ocurrió— pero la secuencia queda `inconcluso`, no `paso`,
y sale con código distinto de 0.

«Hace de veredicto» significa que la secuencia declara al menos un paso
`tipo: pass_fail` en `main` y ninguno de ellos se evaluó. Una secuencia cuyo
criterio son los `limite` de sus pasos no cambia: ahí el veredicto sí se
evaluó, paso a paso.

> **Por qué un estado nuevo y no `error`.** La primera versión de este ADR hacía
> agregar a `error`, para no ampliar el vocabulario. Es semánticamente falso:
> `error` afirma que algo se rompió, y en #31 no se rompió nada — el motor hizo
> exactamente lo que se le pidió. Confundir «no se pudo comprobar» con «hubo un
> fallo del sistema» reintroduce por el otro lado justo la mezcla que la Regla 2
> viene a deshacer. La investigación de la competencia (abajo) resolvió la duda:
> los cuatro sistemas consultados tratan la ausencia de resultado como una
> categoría propia, y el que mejor lo hace le da un valor con severidad
> intermedia.

### Regla 2 — `fallo` es del DUT; `error` es de Anvil

Son afirmaciones distintas y hoy se confunden:

- **`fallo`** = *la unidad no cumple el criterio*. Es información sobre el
  mundo físico.
- **`error`** = *no he podido juzgar*. Es información sobre la secuencia, el
  ejecutor o la configuración.

Todo lo que hoy devuelve `fallo` o `paso` porque Anvil no supo interpretar algo
pasa a `error`:

- Un `estado` no reconocido (#28) es `error`, no `fallo`. Que un ejecutor
  escriba `"PASS"` no dice nada de la unidad.
- Un `asigna` que lee un campo inexistente de `resultado` (#27) es `error`, no
  un `nothing` silencioso. El motor **ya sabe hacerlo**: `aplica_asigna`
  convierte el paso en `error` cuando la expresión falla, con el principio ya
  escrito — *«una asigna que falla es un fallo de definición»*
  (`crates/motor/src/lib.rs`, test `asigna_que_falla_convierte_el_paso_en_error`).
  No hay camino nuevo que abrir; hay que hacer que este caso tome el que existe.

### Regla 3 — lo que altera el criterio queda escrito en el informe

Cuando Anvil juzga con un criterio distinto del declarado en la secuencia, el
informe conserva **los dos**: el declarado y el aplicado.

Hoy el sidecar sustituye el límite y el JSON pierde el original — `operador` y
`valor_esperado` pasan a `null` (#26). Un informe donde no consta que existía un
`≤ 90 °C` y que algo lo anuló no es auditable, y auditable es la única
propiedad que hace útil a un informe de test meses después.

Esto no limita al sidecar: relajar límites es su función y sigue pudiendo
hacerlo. Limita al **silencio**.

### Regla de detección: lo comprobable estáticamente se rechaza al cargar

Las tres reglas anteriores dicen qué devolver. Esta dice cuándo:

> Si una anomalía es detectable sin ejecutar, es **error de carga** — y por
> tanto la ve `--validate` —, no un `error` de ejecución.

Cae aquí lo que hoy pasa el validador sin protestar: `asigna` sobre un paso
`statement` (#27, y el validador **ya** rechaza el caso hermano `pass_fail` +
`asigna`), `--limits` repetido (#22), un nombre del sidecar que casa con un
paso sin límites (#26), y los campos de `resultado`, que son tres y conocidos.

El motivo es de coste real: `--validate` se corre **antes** de lanzar la
campaña; un `error` de ejecución se descubre con la unidad en el banco.

## Alternativas consideradas

### Dejarlo en `--strict` (issue #13)

Que el comportamiento por defecto siga como está y quien quiera rigor lo pida.
**Descartada como respuesta principal.** El modo que se corre por defecto es el
que acaba en producción, y un verde falso por defecto no es una preferencia de
severidad: es una afirmación equivocada. `--strict` se conserva para lo que sí
es criterio del usuario —cascadas de `saltado`, precondiciones en `cleanup`
(#23)—, no para decidir si un veredicto sin evaluar cuenta como bueno.

### Agregar a `error` en vez de añadir un estado *(era la propuesta inicial)*

Mantener cuatro estados y hacer que un veredicto sin evaluar agregara a `error`.
**Descartada tras consultar a la competencia.** `error` afirma que algo se
rompió —en OpenTAP, literalmente *«instrument, DUT, software errors»*; en
TestStand, *«the system encountered a state where the UUT cannot be tested due
to an internal issue»*— y en #31 no se rompe nada: el motor cumple su
especificación. Meter «no lo sé» dentro de «se ha roto» deshace por un lado la
separación que la Regla 2 construye por el otro, y además destruye información:
quien lea el informe no podría distinguir una unidad que no se pudo medir de una
campaña con el banco caído.

Se conserva escrita porque es la alternativa razonable, y porque el motivo de
descartarla —que la ausencia de resultado es una categoría propia, no un
error— es el que sostiene toda la Regla 1.

### Un estado con otro nombre: `sin_evaluar`, `incompleto`

Descriptivos y correctos. Se elige **`inconcluso`** por dos razones: es una sola
palabra, como `paso`, `fallo`, `error` y `saltado` —`sin_evaluar` introduciría
el primer guion bajo del vocabulario—, y traduce el término que ya usa el sector
(`Inconclusive` en OpenTAP, `Incomplete` en la práctica de TestStand), lo que lo
hace reconocible para quien llega de esas herramientas sin tener que aprender un
concepto nuevo.

**Sobre el precedente de añadirlo:** el ADR-0018 descartó las severidades
intermedias citando RNF-08. Esa cita era incorrecta y conviene dejarlo dicho:
RNF-08 es *«reporte textual congelado: el formato textual actual no se cambia
sin querer»*, no «no se añaden estados». M4 ya añadió `saltado` y lo documentó
como **extensión aditiva de RNF-08**, con el test `reporte_a_congela_el_formato`
pasando (`docs/planes/m4-nucleo.md`). El camino está abierto y tiene precedente
propio en el repo.

### Warnings en vez de errores

Que todo esto avise y siga. **Descartada**: un aviso en stderr durante una
campaña desatendida no lo lee nadie, y `--quiet` hoy ni siquiera limpia ese
canal (#35). Un aviso que nadie ve es el silencio con más pasos.

## Cómo lo resuelve el sector

Consultado el 2026-08-13 en fuentes primarias, porque la primera versión de este
ADR lo escribió de memoria y se equivocaba en lo esencial.

### OpenTAP — lo resuelve bien, y es el modelo que se copia

`Verdict` es un tipo de primera clase con seis valores **en orden creciente de
severidad**, y esta es la definición literal de la documentación:

| | Definición oficial |
|---|---|
| `NotSet` | «No verdict was set (the initial value)» |
| `Pass` | «Step or plan passed» |
| **`Inconclusive`** | **«More information is needed to make a verdict or the results were close to the limits»** |
| `Fail` | «Results fail the limits» |
| `Aborted` | «Test plan is aborted by the user» |
| `Error` | «An error occurred; this could be instrument, DUT, software errors, etc.» |

Y la agregación: *«The test step verdict is set to the most severe verdict of
its direct child steps»*.

Dos cosas que decantan la Regla 1. Primera: **`Inconclusive` está por encima de
`Pass`**, así que un hijo sin resultado impide que el padre pase — que es
exactamente lo que Anvil no hace hoy. Segunda: **`Error` es el más severo de los
seis y está separado de `Fail`**, que es la Regla 2 ya resuelta por alguien más.

Fuente: [Test Step — OpenTAP Developer Guide](https://doc.opentap.io/Developer%20Guide/Test%20Step/Readme.html).

### NI TestStand — tiene el mismo defecto que Anvil, y sus usuarios lo sufren

Este es el hallazgo que cambió el ADR. TestStand **sí** separa las dos
afirmaciones, y las define igual que la Regla 2: *«A Failure reflects the fact
that the system is working properly, but the UUT did not match the expected
specification. An Error indicates that the system encountered a state where the
UUT cannot be tested due to an internal issue of the system.»*

Pero esa distinción **no llega al veredicto de la unidad**:

> «TestStand looks for a specific step result status "Failed" to fail the UUT.
> If the whole sequence has step status as "Passed" or "Skip" or "Done" or
> **"error"** the UUT status will always show as passed only.»

Es decir: en TestStand, por defecto, **ni un paso saltado ni un paso en error
hacen fallar a la unidad**. Es el mismo agujero que #31 y #28, en el líder del
mercado con veinte años de recorrido. Y no es una lectura nuestra: el hilo del
que sale esa cita se titula literalmente *«If steps are skipped teststand still
passes»*, y lo abre un ingeniero de reparaciones al que le aprueban unidades a
medio probar.

Para taparlo hay que **programarlo**. Para los errores, la propia base de
conocimiento de NI indica implementar un callback: seleccionar
`SequenceFilePostStepRuntimeError` e insertar
`#NoValidation(RunState.Caller.RunState.SequenceFailed = True)`. Para los saltos,
las respuestas del foro proponen iterar `locals.resultlist` al final de la
secuencia, o modificar el process model. Y un integrador cuenta que en una
instalación grande de reparación acabó marcando esos casos con un estado
inventado a mano: *«the test was marked as **Incomplete**»*.

Fuentes: [If steps are skipped teststand still passes](https://forums.ni.com/t5/NI-TestStand/If-steps-are-skipped-teststand-still-passes/td-p/3067070) ·
[Producing a Fail Result for a Sequence when Step has Runtime Errors](https://knowledge.ni.com/KnowledgeArticleDetails?id=kA00Z000000PAUlSAO).

**La lectura para Anvil es de posicionamiento, no sólo técnica.** El defecto que
tenemos abierto es el mismo que el del líder, y allí la garantía es *opt-in*: hay
que saber que existe el problema, y escribir código para taparlo. Copiar ese
comportamiento sería copiar el punto que sus propios usuarios documentan como
trampa. Hacerlo bien por defecto es una diferencia que se explica en una frase a
alguien que viene de TestStand — y ese alguien es exactamente nuestro
destinatario.

### pytest — la ausencia de resultado tiene código de salida propio

Siete códigos, y el que importa: **`5` = «No tests were collected»**, distinto de
`1` («some of the tests failed»), de `3` («internal error») y de `4` («usage
error»). No hubo fallo; es que no hubo evaluación, y eso no es éxito.

Es el análogo exacto de #31 y la prueba de que la distinción no es teórica: la
herramienta de test más usada del mundo gasta un código de salida en ella.

Fuente: [pytest — Exit codes](https://docs.pytest.org/en/stable/reference/exit-codes.html).

### Robot Framework — `SKIP` es un estado propio y visible

Añadió `SKIP` como estado de primera clase (issue #2087/#3622). Los tests
saltados **no** hacen fallar la suite —igual que en Anvil, y está bien— pero se
reportan aparte y no se pierden. Confirma el otro lado de la Regla 1: un salto
intermedio es neutral; lo que no puede ser neutral es que se salte *todo*.

Fuente: [New `SKIP` status — robotframework#3622](https://github.com/robotframework/robotframework/issues/3622).

### Resumen

| | Separa «no cumple» de «no se pudo determinar» | Lo aplica al veredicto por defecto |
|---|---|---|
| OpenTAP | Sí, `Inconclusive` con severidad propia | **Sí** |
| pytest | Sí, exit code 5 | **Sí** |
| Robot Framework | Sí, `SKIP` visible | Parcial (por diseño) |
| NI TestStand | Sí, en la definición | **No** — hay que programarlo |
| **Anvil hoy** | **No** | **No** |

Anvil es el único de la lista que no hace ni lo uno ni lo otro.

## Recortes

- **El código de salida sigue siendo binario: 0 = `paso`, 1 = todo lo demás.**
  `inconcluso` sale 1. No se imita el esquema de pytest —un código por
  categoría— y no es por comodidad: el std de `wasm32-wasip2` aplana cualquier
  `process::exit(n≠0)` a `I32Exit(1)` al cruzar `wasi:cli/run`, que además
  devuelve `result<_, _>` sin código. Está medido y documentado en #16 y en
  `docs/diseno/ui-vs-headless.md`. La distinción vive en el estado y en el
  informe, no en el código de salida.
- **El formato textual del reporte se extiende, no se cambia** (RNF-08): una
  línea `[inconcluso]` más, igual que M4 hizo con `[saltado]`. El test que
  congela el formato debe seguir pasando, y se añade uno que congele la línea
  nueva.
- **`inconcluso` no se puede escribir en una secuencia ni devolver desde un
  ejecutor.** Lo produce el motor al agregar, y sólo él. Un ejecutor que
  devuelva la cadena `"inconcluso"` cae bajo la Regla 2 como cualquier otro
  valor no reconocido: `error`. Se evita así que un paso pueda declararse a sí
  mismo no concluyente, que es una puerta trasera al verde falso por el otro
  lado.
- **`NotSet` y `Aborted` de OpenTAP no se copian.** El primero no aplica: en
  Anvil todo paso ejecutado deja estado. El segundo pide un mecanismo de
  cancelación que no existe todavía; si llega, entra en la escala de severidad
  por su sitio y este ADR no lo estorba.
- **`saltado` sigue siendo neutral en un paso intermedio.** La Regla 1 aplica al
  veredicto de la secuencia, no convierte cada salto en un problema: eso es
  criterio del usuario y vive en `--strict` (#13, #23).
- **La procedencia del límite (Regla 3) es del JSON y el CSV**, no del reporte
  de consola.
- **`--strict` no se diseña aquí.** Este ADR fija el suelo; qué añade `--strict`
  por encima es el issue #13.
- **No se decide el scope del sidecar** (issue #26, MEJ-14): este ADR obliga a
  que se registre lo que aplica, no a cómo se elige a qué aplica.

## Consecuencias

- **Es un cambio incompatible en el veredicto**, y hay que decirlo así: hay
  secuencias que hoy salen en `paso` y pasarán a `inconcluso` o a `error`. Eso
  es el objetivo, no un efecto colateral — cada una de ellas es una unidad que
  se aprobó sin comprobar. Va en la 0.2.0 y se documenta en el CHANGELOG como
  *breaking*.
- **Quien consuma el JSON o el CSV tiene que contar con un estado más.** Es la
  consecuencia cara de este ADR y la razón de hacerlo ahora: hoy los
  consumidores son la beta propia y nadie más. Dentro de seis meses, con
  usuarios reales, el mismo cambio exigiría un periodo de deprecación.
- **La agregación por severidad hay que escribirla una vez y usarla en los dos
  sitios**: el agregado de la secuencia y el de un `sequence_call` sobre sus
  `sub_pasos`. Hoy son dos caminos distintos en `crates/motor`, y dejarlos
  divergir es cómo se cuela el próximo verde falso.
- **Los seis issues se convierten en trabajo derivado** con criterio ya fijado,
  y pueden hacerse por separado sin que se contradigan entre sí.
- **Un test de regresión por regla**, no por issue. Y visto fallar: reintroducir
  el defecto a mano y comprobar que el test se pone rojo — la campaña de beta ya
  produjo un test que no protegía nada por saltarse este paso.
- **El manual gana una sección corta**: qué significa cada estado y cuándo Anvil
  se niega a juzgar. Hoy `estado` se documenta como cadena libre (#28) y el
  manual no dice en ninguna parte que `fallo` y `error` afirman cosas distintas.
- **Refuerza ADR-0018.** Aquel decidió que una secuencia pudiera fallar por su
  propio criterio de aceptación; este garantiza que ese criterio no se pueda
  evaporar en silencio. Sin el segundo, el primero es opcional en la práctica.
