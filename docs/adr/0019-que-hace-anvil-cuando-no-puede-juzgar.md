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
usa. Es también el terreno donde NI TestStand lleva veinte años y donde un
competidor nuevo no puede permitirse ambigüedad.

## Decisión

Tres reglas. Todo lo demás se deriva de ellas.

### Regla 1 — `paso` es una afirmación, no un valor por defecto

Anvil sólo puede decir `paso` de lo que ha comprobado.

Concretamente: **si una secuencia declara un veredicto y ese veredicto no se
evalúa, la secuencia no puede agregar a `paso`.** Agrega a `error`, y sale con
código distinto de 0.

- «Declara un veredicto» significa que tiene al menos un paso `tipo: pass_fail`
  en `main`. Una secuencia cuyo criterio son los `limite` de sus pasos no
  cambia: ahí el veredicto sí se evaluó, paso a paso.
- El paso sigue reportándose como `[saltado]`, que es lo correcto y ya se ve en
  consola. **Lo que cambia es la agregación, no el estado del paso.** No hace
  falta un estado nuevo.

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

### Un estado de paso nuevo (`sin_evaluar`, `inconclusive`)

Al estilo del `Inconclusive` de OpenTAP. **Descartada por innecesaria**: el paso
ya se reporta como `saltado` y se ve. El defecto está en la agregación, no en la
visibilidad del paso, y la Regla 1 lo arregla sin ampliar el vocabulario.

Conviene dejar constancia de que **sí sería legítimo si hiciera falta**: el
ADR-0018 descartó las severidades intermedias citando RNF-08, pero RNF-08 dice
«reporte textual congelado», no «no añadir estados» — y M4 ya añadió `saltado`
documentándolo como extensión aditiva, con el test de formato pasando. Esa
puerta no está cerrada; simplemente aquí no hace falta.

### Warnings en vez de errores

Que todo esto avise y siga. **Descartada**: un aviso en stderr durante una
campaña desatendida no lo lee nadie, y `--quiet` hoy ni siquiera limpia ese
canal (#35). Un aviso que nadie ve es el silencio con más pasos.

## Cómo lo resuelve el sector

- **NI TestStand** separa `Failed` de `Error` exactamente en este eje: `Failed`
  es el DUT, `Error` es un problema de ejecución, y son estados distintos con
  tratamiento distinto. Es la Regla 2.
- **OpenTAP** tiene `Verdict` de primera clase —`NotSet`, `Inconclusive`,
  `Pass`, `Fail`, `Aborted`, `Error`— con severidad ordenada: el padre hereda el
  **más severo**, y `NotSet`/`Inconclusive` no se convierten en `Pass` al
  agregar. Es la Regla 1.
- **pytest** devuelve **exit code 5 cuando no recogió ningún test**. La suite no
  falló: es que no hubo suite, y eso no es éxito. Es el análogo exacto de #31, y
  la prueba de que la distinción no es una sutileza teórica.
- **JUnit XML**, el formato que consumen los CI, distingue `failure` de `error`
  y reporta `skipped` aparte, precisamente para que un informe agregado no los
  mezcle.

Cuatro herramientas de cuatro tradiciones distintas, y las cuatro separan «no
cumple» de «no se pudo determinar». Anvil hoy no.

## Recortes

- **No se toca el formato textual del reporte** (RNF-08). Los cambios son de
  agregación, de estado devuelto y de contenido del JSON/CSV.
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
  secuencias que hoy salen en `paso` y pasarán a `error`. Eso es el objetivo,
  no un efecto colateral — cada una de ellas es una unidad que se aprobó sin
  comprobar. Va en la 0.2.0 y se documenta en el CHANGELOG como *breaking*.
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
