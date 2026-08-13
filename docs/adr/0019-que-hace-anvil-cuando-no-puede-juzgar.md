# ADR-0019: Qué hace Anvil cuando no puede juzgar

- **Estado:** Aceptada
- **Fecha:** 2026-08-13
- **Cómo se decidió:** redactada desde dirección sobre seis issues verificados
  ejecutando el binario, y aceptada tras contrastar las afirmaciones sobre la
  competencia con fuentes primarias. Ese contraste **refutó** una de ellas —ver
  la nota de método en «NI TestStand»— y corrigió dos citas más. Lo que
  sobrevivió está enlazado a documentación oficial o a código fuente.
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

Conviene añadir dónde nos deja eso frente a la competencia, porque contrastarlo
con fuentes primarias cambió la respuesta: **no estamos igual que los demás,
estamos por detrás de todos.** OpenTAP distingue seis veredictos con severidad
ordenada; NI TestStand propaga los errores hasta la raíz y muestra cuatro
resultados de ejecución; Anvil agrega efectivamente a dos, `paso` o no `paso`.

Lo único que el líder deja sin resolver es la mitad de los saltos —un paso
`Skipped` no hace fallar a la unidad, y sus usuarios lo sufren—, que es
justamente nuestro #31. Ahí sí hay terreno; en el resto lo que hay es distancia
que recortar.

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

`Verdict` es un tipo de primera clase con seis valores. La severidad **no es una
convención de la documentación: es el valor entero del enum**, verificado en el
código fuente (`Engine/Verdict.cs`):

```csharp
public enum Verdict : int
{
    NotSet = 0,       // "No verdict has been set. This is the default value."
    Pass = 10,        // "Test passed."
    Inconclusive = 20,// "Test had an inconclusive result."
    Fail = 30,        // "Test failed."
    Aborted = 40,     // "Test was aborted."
    Error = 50,       // "Test failed due to an exception or another procedural
                      //  error. Such as no instrument/DUT connection."
}
```

Y la agregación es comparación de enteros pura (`Engine/TestStep.cs`):

```csharp
protected void UpgradeVerdict(Verdict verdict)
{
    if ((int)verdict > (int)this.Verdict)
        this.Verdict = verdict;
}
```

Tres cosas decantan la Regla 1:

1. **`Inconclusive` (20) está por encima de `Pass` (10)**, así que un hijo sin
   resultado impide que el padre pase. Es exactamente lo que Anvil no hace hoy.
2. **`Error` (50) es el más severo y está separado de `Fail` (30)**: la Regla 2,
   ya resuelta por alguien más.
3. **El veredicto sólo sube, nunca baja.** El método se llama `UpgradeVerdict`
   y no existe el inverso. Un `inconcluso` ya establecido no puede volver a
   `paso` por lo que haga un paso posterior — propiedad que Anvil debe copiar
   explícitamente, porque es la que impide reintroducir el verde falso por la
   puerta de atrás.

El veredicto se propaga **nivel a nivel**: cada padre toma el peor de sus hijos
directos, pero como el de cada hijo ya incorpora recursivamente el de los suyos,
la severidad de un descendiente profundo **sí llega a la raíz**. Importa para
nosotros porque `sequence_call` anida igual.

Fuentes: [`Engine/Verdict.cs`](https://github.com/opentap/opentap/blob/main/Engine/Verdict.cs) ·
[`Engine/TestStep.cs`](https://github.com/opentap/opentap/blob/main/Engine/TestStep.cs) ·
[Test Step — Developer Guide](https://doc.opentap.io/Developer%20Guide/Test%20Step/Readme.html).

### NI TestStand — resuelve la Regla 2, no resuelve la Regla 1

> **Nota de método.** Una versión anterior de este ADR afirmaba, citando un hilo
> de foro, que en TestStand un paso en `Error` acaba como `Passed`. **Es falso**,
> y la documentación oficial lo desmiente. Queda escrito porque el error es
> instructivo: se sostenía sobre una fuente secundaria y habría sido refutado
> por el primer ingeniero de TestStand que leyera esto.

**Lo que TestStand hace bien, y nosotros no.** La distinción de la Regla 2 es
suya y está en su manual:

> «TestStand does not use run-time errors to indicate UUT test failures. Instead,
> a run-time error indicates a problem exists with the testing process itself and
> testing cannot continue.»

Y —esto es lo que desmontó nuestra afirmación— **la propaga hasta la raíz**:

> «When a subsequence with a run-time error returns to a calling sequence,
> TestStand sets the calling sequence step status to Error, and the calling
> sequence continues to propagate the run-time error up the call stack […] If
> TestStand returns the run-time error to the root sequence invocation, **the
> result status for the execution is Error**.»

Es exactamente lo que la Regla 2 propone y Anvil hoy no hace: en Anvil un
`estado` no reconocido acaba en `fallo` mudo (#28), y una `asigna` rota acaba en
`paso` (#27). En TestStand eso sería `Error`, y se vería.

**Lo que TestStand no resuelve, y es nuestro #31.** De los ocho valores de Step
Status —`Passed`, `Failed`, `Error`, `Done`, `Terminated`, `Skipped`, `Running`,
`Looping`—, **sólo `Failed` está descrito como causante del fallo de la
secuencia**, vía la opción *Step Failure Causes Sequence Failure*, «enabled […]
for most step types» por defecto. Un paso `Skipped` no hace fallar a la unidad.

Ahí sigue viva la queja: el hilo *«If steps are skipped teststand still
passes»* lo abre un ingeniero de reparaciones al que le aprueban unidades a
medio probar, y las respuestas proponen iterar `locals.resultlist` o modificar
el process model. Un integrador cuenta que en una instalación grande acabó
marcando esos casos con un estado inventado: *«the test was marked as
**Incomplete**»* — es decir, reinventando a mano el `inconcluso` que este ADR
añade al motor.

**Y el dato que más apoya la Regla 1, que no esperábamos encontrar aquí.** El
resultado de una ejecución en TestStand **no es binario**: el process model
estándar muestra un banner de cuatro estados —`Passed`, `Failed`, `Error`,
`Terminated`—. El líder del mercado lleva años sin conformarse con dos
categorías de veredicto.

Puesto así, el argumento de este ADR deja de ser «TestStand está roto» —no lo
está— y pasa a ser el correcto: **Anvil es hoy el que menos distingue de los
tres.** OpenTAP tiene seis verdicts, TestStand cuatro resultados de ejecución, y
Anvil agrega efectivamente a dos: `paso` o no `paso`.

**Un matiz que un evaluador de TestStand conocerá, y conviene no esconder.**
Existe la opción de estación *On Run-Time Error → Ignore*, y con ella
*«the execution will produce a "Pass" result for the sequence when all other
steps in the sequence have passed»*. Es decir: TestStand **permite** degradar
errores a `Pass`, pero hay que activarlo a conciencia. Eso no es lo mismo que
hacerlo por defecto, y la propia KB de NI documenta el camino contrario —un
callback `SequenceFilePostStepRuntimeError` con
`#NoValidation(RunState.Caller.RunState.SequenceFailed = True)`— para quien
quiera que un error falle la unidad.

La lectura honesta de ese artículo es que **NI conoce el problema y lo
documenta**. Se puede leer como «está resuelto» o como «hace falta trabajo extra
para lo que debería ser el comportamiento por defecto». Este ADR hace la segunda
lectura, y sólo para el caso de los saltos: ahí no hay ni siquiera una página
oficial que explique cómo fallar por un paso `Skipped`.

Fuentes: [Run-Time Errors](https://www.ni.com/docs/en-US/bundle/teststand/page/run-time-errors.html) ·
[Step Status](https://www.ni.com/docs/en-US/bundle/teststand/page/step-status.html) ·
[Producing a Fail Result for a Sequence when Step has Runtime Errors](https://knowledge.ni.com/KnowledgeArticleDetails?id=kA00Z000000PAUlSAO) ·
[If steps are skipped teststand still passes](https://forums.ni.com/t5/NI-TestStand/If-steps-are-skipped-teststand-still-passes/td-p/3067070) (foro, sólo como indicio del uso real).

### pytest — la ausencia de resultado tiene código de salida propio

Siete códigos, y el que importa: **`5` = «No tests were collected»**, distinto de
`1` («Tests were collected and run but some of the tests failed»), de `3`
(«Internal error») y de `4` («usage error»). No hubo fallo; es que no hubo
evaluación, y eso no es éxito.

Es el análogo exacto de #31 y la prueba de que la distinción no es teórica: la
herramienta de test más usada del mundo gasta un código de salida en ella.

Fuente: [pytest — Exit codes](https://docs.pytest.org/en/stable/reference/exit-codes.html).

### Robot Framework — el precedente más parecido a la Regla 1, y su límite

Añadió `SKIP` como estado de primera clase en **RF 4.0** (marzo de 2021, issue
#3622), sustituyendo al viejo mecanismo de *criticality*. Y su regla de
agregación de suite es, literalmente, la que este ADR propone:

> «If any test has failed, suite status is FAIL. If there are no failures but at
> least one test has passed, suite status is PASS. **If all tests have been
> skipped or there are no tests at all, suite status is SKIP.**»

Un salto individual es neutral; que se salte *todo* no lo es. Es la Regla 1 con
otro nombre, en una herramienta con quince años de uso.

**Y aquí está su límite, que conviene no ocultar**: eso no llega al código de
salida. *«The return code to the system is the number of failed tests, skipped
tests do not affect it»* — una suite 100 % saltada devuelve **0**, igual que una
que pasó entera. Un CI que sólo mire `$?` no las distingue.

Fuentes: [Test Execution — User Guide](https://github.com/robotframework/robotframework/blob/master/doc/userguide/src/ExecutingTestCases/TestExecution.rst) ·
[Release notes RF 4.0](https://github.com/robotframework/robotframework/blob/master/doc/releasenotes/rf-4.0.rst).

### Dónde llega la distinción en cada uno

Que un sistema separe «no evaluado» de «correcto» no significa que lo lleve
hasta el final. Verificado, llega hasta aquí:

| | En el estado / veredicto | En el código de salida |
|---|---|---|
| OpenTAP | Sí, `Inconclusive` (20) | **Sí** — `tap run` devuelve 20 |
| pytest | — | **Sí** — exit 5 |
| Robot Framework | Sí, suite `SKIP` | **No** — devuelve 0 |
| **Anvil (esta decisión)** | **Sí, `inconcluso`** | **Sí, pero sólo como ≠ 0** |

Ninguno de los tres activa por defecto un modo «saltar = fallar», y esta
decisión tampoco lo hace: un paso saltado a mitad de secuencia sigue siendo
neutral. Lo que deja de ser neutral es que se salte **el veredicto**.

### Resumen, tras contrastar

Ninguno de los cuatro es un modelo a copiar entero, y ninguno está roto. Lo que
la comparación deja claro es más incómodo que un «los demás lo hacen mal»:

| | «No cumple» ≠ «no se pudo juzgar» | El veredicto ausente afecta al agregado | Llega al código de salida |
|---|---|---|---|
| OpenTAP | Sí — `Error` (50) ≠ `Fail` (30) | Sí — `Inconclusive` (20) > `Pass` (10) | Sí — exit 20 |
| NI TestStand | **Sí** — `Error` se propaga hasta la raíz | **No** — sólo `Failed` falla la unidad | 4 categorías de resultado |
| pytest | Sí — exit 3 ≠ exit 1 | Sí — exit 5 «no tests collected» | Sí |
| Robot Framework | Parcial | Sí — suite `SKIP` si todo se saltó | **No** — devuelve 0 |
| **Anvil hoy** | **No** — #27, #28 | **No** — #31 | No |
| **Anvil con este ADR** | Sí — Regla 2 | Sí — Regla 1 | Sólo como ≠ 0 |

**Anvil es hoy el único de la lista que no hace ninguna de las dos cosas**, y
eso —no un defecto ajeno— es lo que justifica este ADR.

El argumento hacia fuera que sí se sostiene, dicho con precisión: TestStand
resuelve bien la mitad de errores y deja la de los saltos al usuario; OpenTAP
resuelve las dos y es la referencia. Anvil debería salir a competir haciendo las
dos por defecto, que es la posición de OpenTAP, no la de TestStand. Cualquier
afirmación más fuerte que esta es refutable por un ingeniero con la
documentación de NI delante — y ya nos pasó al escribir la primera versión de
esta sección.

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
