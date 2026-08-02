# Diseño: Límites y estados

> **Prioridad:** MVP. Los estados y los límites-como-medida ya están
> implementados; los límites como *datos first-class* y el property loader se
> implementan en M3 (RF-29, RF-30).

Trazable a `ResultadoStep` (`crates/modelo/src/lib.rs`), al contrato
`crates/modelo/paso.proto` y al `Limite`/`aplicar_limite` del motor
(ver [contrato-grpc.md](../contrato-grpc.md) y [ADR-0008](../adr/0008-limites-evaluados-por-el-motor.md)).

## Los tres estados

Cada paso devuelve un `estado` (texto, no enum — RF-10):

| Estado | Significado | Corta el Main | Cuenta para el agregado |
|---|---|---|---|
| `paso` | El paso cumplió su criterio. | No | Solo si no hay error/fallo |
| `fallo` | No cumplió un **criterio de aceptación** (p. ej. medida fuera de rango). Resultado **válido**. | Sí | Sí |
| `error` | No pudo ejecutarse (comunicación, nombre desconocido, excepción). | Sí | Sí (manda sobre fallo) |

### Agregado `error > fallo`

`ResultadoSecuencia::estado`:

```rust
if any(p.estado == "error") { "error" }
else if any(p.estado == "fallo") { "fallo" }
else { "paso" }
```

Razonamiento: un `error` significa que **no sabemos** el estado real del
UUT (algo impidió medir); es peor que un `fallo`, que significa "medimos y
no cumple". Un `error` manda aunque llegue antes que un `fallo` (testeado).

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
(`cargador::cargar_limites_de_archivo` + `cargador::aplicar_limites`),
asociando cada límite al paso por `nombre`. El sidecar **manda** sobre el
límite embebido en la secuencia: es el mecanismo para cambiar umbrales por
lote/variante sin tocar la secuencia. Ejemplo en `ejemplos/limites.yaml` +
`ejemplos/limites.limits.yaml`, invocado con
`anvil secuencia.yaml --limits limites.limits.yaml`.

## Out-of-scope

- Límites estadísticos / dinámicos (golden sample, CPK en runtime) →
  post-MVP, ligado a monitoring.
- Límites con unidades físicas (V, A, Ω) y conversión → post-MVP.