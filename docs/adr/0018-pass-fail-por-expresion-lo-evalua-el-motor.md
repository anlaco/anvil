# ADR-0018: El veredicto por expresión (`pass_fail`) lo evalúa el motor

- **Estado:** Aceptada
- **Fecha:** 2026-08-10 (post-MVP, cola de la beta)
- **Relaciona:** ADR-0005, ADR-0008, ADR-0009,
  [modelo-de-pasos.md](../diseno/modelo-de-pasos.md),
  [motor-de-ejecucion.md](../diseno/motor-de-ejecucion.md),
  [informe-beta-2026-08.md](../qa/informe-beta-2026-08.md#diag-2)

## Contexto

Hasta aquí un paso podía fallar por dos vías: porque **el propio paso** lo
decide (`pass/fail` gRPC, RF-25 — `pasos_demo::verificar_led`), o porque el
motor evalúa el **límite** declarado sobre su medida (RF-29, ADR-0008). Las
dos operan sobre **un** paso y **una** medida.

Lo que no había forma de expresar es el criterio de aceptación **compuesto**:
"el DUT es bueno si el voltaje está en rango **y** la temperatura no pasa de
50". El `statement` (RF-27) sólo admite asignación, así que el patrón que sale
de ahí es escribir el veredicto en un local y no volver a mirarlo.

La primera beta externa lo midió: **131 de las 180 secuencias terminan en un
`eval_final`/`dut_ok`/«índice de calidad global ponderado» que no puede hacer
fallar nada**, y sólo 2 vuelven a consumir ese local. Es decorativo, y explica
buena parte de por qué esa suite daba verde casi siempre. Algunos betatesters
escribieron `locals.x = locals.x` para rellenar el hueco (DIAG-2).

En un secuenciador de test, un criterio de aceptación que no puede fallar no es
una carencia de ergonomía: es un veredicto equivocado y silencioso, la misma
familia que DEF-1 y DEF-3 de esa campaña.

## Decisión

Un **tipo de paso nuevo**, `pass_fail`, cuya `condicion` es una expresión
booleana que **evalúa el motor**:

```yaml
- nombre: medir_voltaje
  reintentos: 1
  asigna: { v: '${resultado.valor_medido}' }

- nombre: verificar_dut
  tipo: pass_fail
  condicion: 'locals.v > 4.9 && locals.v < 5.1'
```

- `true` → `paso`; `false` → **`fallo`**; no-Bool o error de evaluación →
  `error`.
- Es el mismo patrón que ADR-0008 (el límite lo evalúa el motor) y ADR-0009
  (la precondición la evalúa el motor): **lo declarado en el YAML lo resuelve
  el motor; el paso no interviene**. `paso.proto` no cambia (RNF-05) y el
  motor sigue siendo genérico (ADR-0005): no conoce el dominio, sólo evalúa
  la expresión que le dan.
- Bool **estricto**, como la precondición: sin truthiness (sintaxis Julia). La
  diferencia con la precondición está en el veredicto — allí un `false`
  **salta** el paso; aquí lo **falla**.
- El corte de Main y `pause_on_fail` salen gratis: el bucle de fases ya trata
  como fallo todo lo que no sea `paso`/`saltado`.

Implementación: `TipoPaso::PassFail` + `DefinicionPaso.condicion`
(`crates/modelo`), parseo y validaciones de coherencia en `crates/cargador`,
y `motor::evalua_pass_fail` (`crates/motor`). **`crates/expr` no se toca.**

Con esto RF-25 queda cubierto por las dos vías que ofrece un secuenciador
maduro: veredicto decidido por el paso, y veredicto decidido por el
secuenciador sobre variables.

## Alternativas consideradas

### Inferir el assert de un `statement` sin `=` (la propuesta del issue #7)

Que `statement: 'locals.v > 4.9'` —una expresión sin asignación— actuara como
assert. No cuesta campos nuevos en el schema. **Descartada** por dos razones:

1. **Un `=` olvidado cambiaría el significado en silencio.** `locals.ok = (v >
   4.9)` asigna y no verifica; `locals.ok == (v > 4.9)` verifica y no asigna.
   Un dedo torcido convierte una cosa en la otra sin ningún aviso — la misma
   clase de fallo silencioso que acabábamos de cerrar en DEF-3 (un `asigna`
   que ensombrecía un `parameter`). Introducirla el mismo mes habría sido
   incoherente.
2. **Va contra el precedente del sector** (ver abajo).

### Que el paso decida (dejarlo como está)

Escribir un paso gRPC que lea las variables y devuelva `fallo`. Rompe el
aislamiento: el paso tendría que conocer el entorno del motor, que es
justamente lo que ADR-0003 y ADR-0009 mantienen separado. Y obliga a compilar
código para expresar un umbral, que es lo contrario de "la secuencia es datos"
(ADR-0002).

### Un `error` de carga para la condición no booleana

Sería fail-fast, pero los tipos son **dinámicos**: `locals.x + 1` sólo se sabe
que no es Bool al evaluar. Sólo se podrían rechazar formas triviales, lo que
daría una garantía a medias. Se deja como `error` de ejecución, idéntico al de
la precondición.

## Cómo lo resuelve la competencia

- **NI TestStand**: su `Statement` step **no** produce veredicto (es
  utilitario, sin *Status Expression*). El veredicto tiene step type propio,
  `Pass/Fail Test`, uno de sus cinco built-in, cuyo *Data Source* es una
  expresión booleana que el secuenciador evalúa — `True` = Pass, `False` =
  Fail. Es exactamente esta decisión.
- **Robot Framework**: keyword dedicado, `Should Be True <expresión>`, que
  falla el test si la condición no se cumple.
- **OpenTAP**: `Verdict` de primera clase (Pass/Fail/Inconclusive/Error/
  Aborted) que el paso fija explícitamente con `UpgradeVerdict`; el padre
  hereda el más severo.
- **pytest** sí usa la forma desnuda (`assert <expr>`), pero ahí el fichero ya
  es un lenguaje de programación, no datos declarativos.

El patrón es claro: en las herramientas **declarativas** de test, el veredicto
por expresión tiene **nombre propio**; la forma implícita sólo aparece donde el
formato ya es código. Anvil es declarativo (ADR-0002).

## Recortes

- **Un `pass_fail` no admite `reintentos > 1`**: evalúa una expresión pura, así
  que el resultado no cambia entre intentos. Es error de carga, siguiendo el
  precedente de `sequence_call`, en vez de aceptarlo e ignorarlo.
- **Un `pass_fail` no admite `asigna`** (no produce `resultado.*` que volcar) ni
  `limite` (no mide) ni `ejecutor` (es motor-side). Todos, error de carga.
- **Sin severidades intermedias** al estilo `Inconclusive` de OpenTAP: los
  estados siguen siendo `paso`/`fallo`/`error`/`saltado` (RNF-08).
- **El mensaje del reporte no reproduce la expresión** que falló («condición no
  cumplida»). Reconstruir el texto desde el AST pide un `Display` para
  `Expresion` que hoy no existe; queda pendiente si el uso lo pide.

## Consecuencias

- La suite de un usuario puede, por fin, **fallar por su propio criterio de
  aceptación**. Es un cambio de fondo en lo que una secuencia puede afirmar.
- **Compatibilidad total**: `tipo` es opcional y sigue defaulteando a `grpc`;
  ninguna secuencia existente cambia de comportamiento. `statement` se queda
  como está —sólo asignación— **a propósito**: cada construcción hace una cosa.
- ADR-0008/0009 se **refuerzan**: la familia "el motor evalúa lo declarado"
  gana su tercer miembro (límite, precondición, veredicto) con la misma forma y
  el mismo criterio de Bool estricto.
