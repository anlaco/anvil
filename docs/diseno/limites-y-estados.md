# Diseño: Límites y estados

> **Prioridad:** MVP (ya implementado en `crates/modelo/src/lib.rs`).
> Formaliza los estados y los límites; añade como MVP-parcial los límites
> como *datos first-class* y el property loader.

Trazable a `ResultadoStep` (`crates/modelo/src/lib.rs`) y al contrato
`crates/modelo/paso.proto` (ver [contrato-grpc.md](../contrato-grpc.md)).

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

## Límites como datos first-class (MVP-parcial, pendiente)

Hoy el límite está *embebido* en el código del paso. El objetivo (RF-29) es
que los límites sean **datos** en la secuencia, no aserciones ad-hoc en
código:

```yaml
pasos_main:
  - nombre: medir_voltaje
    reintentos: 1
    limite:
      tipo: rango          # rango | comparacion
      min: 4.5
      max: 5.5
      valor_esperado: 4.2  # solo para comparacion
```

Consecuencia: el motor (o un paso de *limit test* genérico) evalúa el
límite contra la medida devuelta y produce `paso`/`fallo` **sin que el paso
conozca el umbral**. Separa el *qué es aceptable* (datos, cambia en
producción) del *cómo se mide* (código del paso).

> **Decisión de diseño (propuesta):** los límites viven en la **definición
> de la secuencia** (YAML), no en `paso.proto`. El paso devuelve la medida;
> el evaluador de límites (lado motor o un built-in limit test) produce el
> estado. Esto mantiene el contrato del paso estable y el cambio de límites
> en producción sin re-deploy (online limit editing, post-MVP).

## Tipos de límite (propuesta, MVP-parcial)

- **Rango** (high/low): `min ≤ valor ≤ max` → `paso`; si no, `fallo`.
- **Comparación**: `valor == esperado` (o `<`, `>`, `<=`, `>=`) → `paso`/`fallo`.
- **Sin límite** (Pass/Fail): el paso decide sin medida
  ([modelo-de-pasos.md](modelo-de-pasos.md)).

## Property loader (MVP-parcial, pendiente)

Cargar límites desde un **fichero externo** (CSV/JSON/YAML), separando los
datos de test del flujo. Un paso *property loader* (built-in post-MVP) o un
cargador de secuencia los inyecta en `limite` antes de ejecutar. Permite
cambiar límites por lote/variante sin tocar la secuencia.

## Out-of-scope

- Límites estadísticos / dinámicos (golden sample, CPK en runtime) →
  post-MVP, ligado a monitoring.
- Límites con unidades físicas (V, A, Ω) y conversión → post-MVP.