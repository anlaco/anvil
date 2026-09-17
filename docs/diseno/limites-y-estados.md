# Diseño: Límites y estados

> **Prioridad:** MVP. Los estados y los límites-como-medida ya están
> implementados; los límites como *datos first-class* y el property loader se
> implementan en M3 (RF-29, RF-30).

Trazable a `ResultadoStep` (`crates/modelo/src/lib.rs`), al contrato
`crates/modelo/paso.proto` y al `Limite`/`aplicar_limite` del motor
(ver [contrato-grpc.md](../contrato-grpc.md) y [ADR-0008](../adr/0008-limites-evaluados-por-el-motor.md)).
La escala de severidad y el estado `inconcluso` los fija
[ADR-0019](../adr/0019-que-hace-anvil-cuando-no-puede-juzgar.md).

## Los estados

Un ejecutor devuelve un `estado` (texto, no enum — RF-10), y son **cuatro,
cerrados**: `pass`, `fail`, `error`, `skipped`. Que el tipo sea texto es por el
contrato (`paso.proto` viaja así, y un paso puede estar escrito en cualquier
lenguaje); el vocabulario no por eso es abierto. Cualquier otra cadena —`"Paso"`
con mayúscula, `"PASS"`, `"ok"`— **la convierte el motor en `error`**, con un
mensaje que nombra el valor recibido y enumera los válidos
(ADR-0019 Regla 2, issue #28):

```
[error] verificar_led: el ejecutor devolvió el estado 'Paso', que no es ninguno
de 'pass', 'fail', 'error', 'skipped': Anvil no juzga la unidad con un estado
que no entiende (el paso decía: 'led encendido')
```

No es purismo: un estado que Anvil no entiende **no dice nada sobre la unidad**,
así que tratarlo como veredicto —en cualquier dirección— es inventarse una
afirmación. Antes esto acababa en `fallo` mudo; al introducir la escala de
severidad pasó a `paso` mudo, que dejaba pasar unidades sin medir.

| Estado | Significado | Corta el Main | Cuenta para el agregado |
|---|---|---|---|
| `pass` | El paso cumplió su criterio. | No | Es el mínimo de la escala |
| `fail` | No cumplió un **criterio de aceptación** (p. ej. medida fuera de rango). Resultado **válido**. | Sí | Sí |
| `error` | No pudo ejecutarse o juzgarse (comunicación, módulo desconocido, excepción, un `numeric_limit` sin número). | Sí | Sí (manda sobre fail) |
| `skipped` | No se ejecutó (`disable` o precondición falsa, RF-33/34). | No | No: es **neutral** |

Y dos que **ningún ejecutor devuelve** — los produce el motor, y sólo él; un
ejecutor que devolviera esas cadenas cae en `error` como cualquier estado no
reconocido:

| Estado | Significado |
|---|---|
| `done` | El paso terminó y **no juzgó nada** (ADR-0040 §6): un `action` cuyo módulo pasó, un `statement` que terminó bien, un `numeric_limit` con `comparison: none`. **Neutral**, como `skipped`: no corta `setup` ni `main`, no hace fallar la secuencia y no cuenta como `pass`. Una secuencia sólo de pasos `done` agrega a `pass` (ADR-0040 §6, anotado en ADR-0042). |
| `inconclusive` | Anvil no pudo juzgar. Sólo existe como **agregado de una secuencia** (ADR-0019). |

### Agregado por severidad (ADR-0019, Regla 1)

`ResultadoSecuencia::estado()` devuelve **el más severo de sus pasos**, en esta
escala:

```
pass  <  inconclusive  <  fail  <  error
```

con `skipped` y `done` fuera de ella (mapean al mínimo). En el código, el orden de declaración del enum
`Severidad` **es** la escala, y agregar es un `max()` — el mismo modelo que el
`Verdict` de OpenTAP, donde la severidad tampoco es una convención de la
documentación sino el valor entero del enum.

Razonamiento de cada peldaño:

- Un `error` significa que **no sabemos** el estado real del UUT (algo impidió
  medir); es peor que un `fallo`, que significa «medimos y no cumple». Un
  `error` manda aunque llegue antes que un `fallo` (testeado).
- `inconcluso` va **por encima de `paso`** porque una ausencia de información no
  puede convertirse en una afirmación, y **por debajo de `fallo`** porque no
  afirma nada del UUT: sólo dice que no se juzgó. Por eso no tapa un `fallo` ni
  un `error` que también estén presentes (testeado).

Antes de ADR-0019 esto era una cascada `error > fallo > paso` cuyo `else`
devolvía `paso`. Ese `else` era el issue #31: una secuencia cuyo veredicto no se
llegaba a evaluar no había fallado, luego «pasaba», y salía con código 0.

### Cuándo sale `inconcluso`

Un solo caso, de momento: **la secuencia declara al menos un paso
`type: pass_fail` en `main` —con o sin `module`— y ninguno llegó a evaluarse**
— se saltó por precondición, está `disable`, o el Main cortó antes de llegar. El
paso se sigue reportando `[skipped]` (es lo que ocurrió); lo que cambia es el
agregado.

Los `numeric_limit` **no cuentan** para esto: una secuencia cuyo criterio son
sus límites no cambia de comportamiento.

Un `pass_fail` con `disable: true` cuenta como declarado y no evaluado: la
unidad tampoco se ha medido, y eximirlo convertiría el flag en una puerta
trasera al verde falso. Si un salto intencionado debe tratarse distinto, eso es
criterio del usuario y vive en `--strict` (#13, #23).

La propagación anidada es **nivel a nivel**: el `ResultadoStep` de un
`sequence_call` lleva el agregado de su subsecuencia, así que la severidad de un
descendiente profundo llega a la raíz por el mismo camino que un `fallo`.

## Límites como medida (en el contrato)

`paso.proto` sigue llevando `measured_value`, `limit_min` y `limit_max` como
**string** (vacío si no hay). Un ejecutor devuelve la medida; el umbral lo pone
la secuencia (abajo).

Ejemplo del repo (`demo/measure_voltage` en `ejemplos/basica.yaml`): mide 4.2
contra `GELE` 4.5–5.5 → `fail`.

## Límites como datos first-class

El límite no está en el código del paso: es **datos** en la secuencia, en un
paso `type: numeric_limit`, con la forma de TestStand (ADR-0040 §7):

```yaml
main:
  - name: demo/measure_voltage
    type: numeric_limit
    module: demo/measure_voltage
    executor: demo
    limit: { comparison: GELE, low: 4.5, high: 5.5, units: V }
```

o, contra un solo límite:

```yaml
  - name: frequency
    type: numeric_limit
    module: dmm/measure_frequency
    executor: bench
    limit: { comparison: GE, low: 1000.0, units: Hz }
```

Consecuencia: el módulo mide y reporta que la medición fue bien (`pass`); el
**motor** evalúa el límite contra `valor_medido` —o contra el `value` del paso—
y produce `pass`/`fail` **sin que el paso conozca el umbral**. Separa el *qué es aceptable* (datos,
cambia en producción) del *cómo se mide* (código del paso).

> **Decisión de diseño (ADR-0008):** los límites viven en la **definición
> de la secuencia** (YAML), no en `paso.proto`. El paso devuelve la medida;
> el **motor** evalúa el límite y produce el estado. Esto mantiene el
> contrato del paso estable y el cambio de límites en producción sin
> re-deploy (online limit editing, post-MVP).
>
> Regla fina: el límite solo **empeora** `paso` → `fallo`. Si el paso ya
> emitió `fallo`/`error` por sí mismo, se respeta (el paso es autoridad sobre
> su ejecución). El motor no convierte un fallo/error en paso.
>
> **Estrechado por ADR-0040 §5:** un `numeric_limit` sin número que juzgar —el
> módulo no devolvió medida, o `value` no evaluó a número— es `error`, no un
> límite que no aplica en silencio.
>
> Es compatible con ADR-0005: una regla high/low/comparación **declarada como
> dato** es semántica genérica, no conocimiento del dominio. El motor sigue
> sin saber qué mide un voltaje; solo aplica una comparación que la secuencia
> le entrega.

Implementación: `modelo::Limite` (un `Criterio` y `unidades`) con `evalua`
pura, `DefinicionPaso.limite`, `motor::juzga_limite_numerico` (elige el número:
`value` o `valor_medido`) y `motor::aplicar_limite` (rellena los campos de
límite del `ResultadoStep` para el reporte —`limite_min`/`limite_max`,
`valor_esperado`/`operador` para un código de un límite, `comparacion`,
`unidades`— y, si procede, convierte `pass`→`fail` o, con `none`, `pass`→`done`).
Nada de eso va en `paso.proto`: lo rellena el motor para los sinks.

## Códigos de comparación (ADR-0040 §7)

Los de TestStand. Un campo que el código no usa es error de carga, y un código
en minúsculas se señala con su forma en mayúsculas.

| Código | Campos | Pasa si |
|---|---|---|
| `EQ`, `NE`, `GT`, `LT`, `GE`, `LE` | `low` | `valor {=, ≠, >, <, ≥, ≤} low` |
| `GELE`, `GELT`, `GTLE`, `GTLT` | `low`, `high` | dentro: `GE`/`LE` incluyen el límite, `GT`/`LT` no |
| `LEGE`, `LEGT`, `LTGE`, `LTGT` | `low`, `high` | fuera: `LE`/`GE` cuentan el límite como fuera |
| `EQT` | `nominal`, `lower`, `upper`, `threshold` (`percent`/`ppm`/`delta`) | `nominal − lower ≤ valor ≤ nominal + upper`, con `lower`/`upper` en porcentaje o ppm de `|nominal|`, o absolutos con `delta`. **No contrastado** con la documentación de NI: la fórmula no se pudo leer al implementarlo. |
| `none` | — | no compara: registra el valor y da `done` (ADR-0040 §8) |

`units` es texto para el informe: no escala ni afecta a la comparación. En
consola, un fallo se lee `4.2 V fuera de rango [4.5, 5.5]` (`(`/`)` marcan un
extremo excluido), `4.2 >= 5 no cumplido` para un código de un límite,
`… dentro de [a, b], y tenía que quedar fuera` fuera de dos, y
`… fuera de 5 -1/+1 percent [4.95, 5.05]` para `EQT`.

Sin límite (`action`, `pass_fail`): el paso decide sin medida
([modelo-de-pasos.md](modelo-de-pasos.md)).

## Property loader (MVP-parcial, implementado en M3)

Cargar límites desde un **fichero sidecar** (YAML, con la misma forma de límite
por nombre de paso), separando los datos de test del flujo. El cargador los
inyecta en `limite` antes de ejecutar
(`cargador::cargar_limites_de_archivo` + `cargador::aplicar_limites_programa`),
asociando cada límite al paso por `nombre`. El sidecar **manda** sobre el
límite embebido en la secuencia: es el mecanismo para cambiar umbrales por
lote/variante sin tocar la secuencia. Ejemplo en `ejemplos/limites.yaml` +
`ejemplos/limites.limits.yaml`, invocado con
`anvil secuencia.yaml --limits limites.limits.yaml`.

**Alcance: el programa entero.** El nombre casa en la raíz, en las
subsecuencias de archivos externos y en las inline. Que cubriera sólo la raíz
era DEF-1 del informe de beta: bajo `--process-model` la raíz es el process
model y la secuencia del operador queda como subsecuencia, así que el sidecar
no afectaba a nada —y sin decirlo— justo en el modo para el que existe. Un
nombre que no casa en **ninguna** secuencia se avisa por stderr
(`cargador::limites_sin_aplicar_programa`, DIAG-1), y un límite que cae en un
paso que no es `numeric_limit` detiene la corrida nombrando el paso
(`cargador::limites_mal_colocados_programa`, ADR-0042 §4).

## Out-of-scope

- Límites estadísticos / dinámicos (golden sample, CPK en runtime) →
  post-MVP, ligado a monitoring.
- Conversión de unidades físicas (V, A, Ω): `units` es sólo texto → post-MVP.