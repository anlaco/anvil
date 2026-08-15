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

Cada paso devuelve un `estado` (texto, no enum — RF-10), y son **cuatro,
cerrados**: `paso`, `fallo`, `error`, `saltado`. Que el tipo sea texto es por el
contrato (`paso.proto` viaja así, y un paso puede estar escrito en cualquier
lenguaje); el vocabulario no por eso es abierto. Cualquier otra cadena —`"Paso"`
con mayúscula, `"PASS"`, `"ok"`— **la convierte el motor en `error`**, con un
mensaje que nombra el valor recibido y enumera los válidos
(ADR-0019 Regla 2, issue #28):

```
[error] verificar_led: el ejecutor devolvió el estado 'Paso', que no es ninguno
de 'paso', 'fallo', 'error', 'saltado': Anvil no juzga la unidad con un estado
que no entiende (el paso decía: 'led encendido')
```

No es purismo: un estado que Anvil no entiende **no dice nada sobre la unidad**,
así que tratarlo como veredicto —en cualquier dirección— es inventarse una
afirmación. Antes esto acababa en `fallo` mudo; al introducir la escala de
severidad pasó a `paso` mudo, que dejaba pasar unidades sin medir.

| Estado | Significado | Corta el Main | Cuenta para el agregado |
|---|---|---|---|
| `paso` | El paso cumplió su criterio. | No | Es el mínimo de la escala |
| `fallo` | No cumplió un **criterio de aceptación** (p. ej. medida fuera de rango). Resultado **válido**. | Sí | Sí |
| `error` | No pudo ejecutarse (comunicación, nombre desconocido, excepción). | Sí | Sí (manda sobre fallo) |
| `saltado` | No se ejecutó (`disable` o precondición falsa, RF-33/34). | No | No: es **neutral** |

Y un quinto que **ningún paso devuelve**:

| Estado | Significado |
|---|---|
| `inconcluso` | Anvil no pudo juzgar. Sólo existe como **agregado de una secuencia**: lo produce el motor, y sólo él (ADR-0019). Un ejecutor que devolviera la cadena `"inconcluso"` no lo estaría declarando — sería un estado no reconocido más. |

### Agregado por severidad (ADR-0019, Regla 1)

`ResultadoSecuencia::estado()` devuelve **el más severo de sus pasos**, en esta
escala:

```
paso  <  inconcluso  <  fallo  <  error
```

con `saltado` fuera de ella. En el código, el orden de declaración del enum
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
`tipo: pass_fail` en `main` y ninguno llegó a evaluarse** — se saltó por
precondición, está `disable`, o el Main cortó antes de llegar. El paso se sigue
reportando `[saltado]` (es lo que ocurrió); lo que cambia es el agregado.

Una secuencia cuyo criterio son los `limite` de sus pasos **no cambia de
comportamiento**: ahí el veredicto sí se evaluó, paso a paso.

Un `pass_fail` con `disable: true` cuenta como declarado y no evaluado: la
unidad tampoco se ha medido, y eximirlo convertiría el flag en una puerta
trasera al verde falso. Si un salto intencionado debe tratarse distinto, eso es
criterio del usuario y vive en `--strict` (#13, #23).

La propagación anidada es **nivel a nivel**: el `ResultadoStep` de un
`sequence_call` lleva el agregado de su subsecuencia, así que la severidad de un
descendiente profundo llega a la raíz por el mismo camino que un `fallo`.

## Límites como medida (MVP, ya en el contrato)

Un `ResultadoStep` puede llevar medida:

```rust
ResultadoStep::medido(nombre, estado, mensaje, valor, min, max)
// → valor_medido, limite_min, limite_max (Option<f64>)
```

Viajan en `paso.proto` como **string** (vacío si no hay; enteros sin
decimales). El paso **decide** `paso`/`fallo` comparando `valor` contra
`[min, max]` — la comparación vive en el lado del paso (ADR-0005: el motor
no conoce el dominio).

Ejemplo del repo (`pasos_demo::medir_voltaje`): mide 4.2 contra rango
4.5–5.5 → `fallo` ("voltaje fuera de rango").

## Límites como datos first-class (MVP-parcial, implementado en M3)

El límite deja de estar *embebido* en el código del paso y pasa a ser **datos**
en la secuencia, no aserciones ad-hoc en código:

```yaml
pasos_main:
  - nombre: medir_voltaje
    reintentos: 1
    limite:
      tipo: rango          # rango | comparacion
      min: 4.5
      max: 5.5
```

o, para una comparación:

```yaml
  - nombre: verificar_frecuencia
    limite:
      tipo: comparacion
      op: ge               # eq | ne | lt | le | gt | ge
      esperado: 1000.0
```

Consecuencia: el paso mide y reporta que la medición fue bien (`paso`); el
**motor** evalúa el límite contra `valor_medido` y produce `paso`/`fallo`
**sin que el paso conozca el umbral**. Separa el *qué es aceptable* (datos,
cambia en producción) del *cómo se mide* (código del paso).

> **Decisión de diseño (ADR-0008):** los límites viven en la **definición
> de la secuencia** (YAML), no en `paso.proto`. El paso devuelve la medida;
> el **motor** evalúa el límite y produce el estado. Esto mantiene el
> contrato del paso estable y el cambio de límites en producción sin
> re-deploy (online limit editing, post-MVP).
>
> Regla fina: el límite solo **empeora** `paso` → `fallo`. Si el paso ya
> emitió `fallo`/`error` por sí mismo, se respeta (el paso es autoridad sobre
> su ejecución). El motor no convierte un fallo/error en paso. Si no hay
> `valor_medido` (pass/fail, action), el límite no aplica.
>
> Es compatible con ADR-0005: una regla high/low/comparación **declarada como
> dato** es semántica genérica, no conocimiento del dominio. El motor sigue
> sin saber qué mide un voltaje; solo aplica una comparación que la secuencia
> le entrega.

Implementación: `modelo::Limite` (`Rango`/`Comparacion`) con `evalua` pura,
`DefinicionPaso.limite`, y `motor::aplicar_limite` (rellena los campos de
límite del `ResultadoStep` para el reporte y, si procede, convierte
`paso`→`fallo` reescribiendo el mensaje). `ResultadoStep` gana `valor_esperado`
y `operador` — **no** van en `paso.proto`: los rellena el motor; el
`ResultadoStep` enriquecido solo va a los sinks.

## Tipos de límite (MVP-parcial, implementado)

- **Rango** (high/low): `min ≤ valor ≤ max` → `paso`; si no, `fallo`.
- **Comparación**: `valor {op} esperado` con `op` ∈ `eq`/`ne`/`lt`/`le`/`gt`/
  `ge` → `paso`/`fallo`.
- **Sin límite** (Pass/Fail, Action): el paso decide sin medida
  ([modelo-de-pasos.md](modelo-de-pasos.md)).

## Property loader (MVP-parcial, implementado en M3)

Cargar límites desde un **fichero sidecar** (YAML), separando los datos de
test del flujo. El cargador los inyecta en `limite` antes de ejecutar
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
(`cargador::limites_sin_aplicar_programa`, DIAG-1).

## Out-of-scope

- Límites estadísticos / dinámicos (golden sample, CPK en runtime) →
  post-MVP, ligado a monitoring.
- Límites con unidades físicas (V, A, Ω) y conversión → post-MVP.