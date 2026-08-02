# ADR-0008: Los límites los evalúa el motor, no el paso

- **Estado:** Aceptada
- **Fecha:** 2026-08-02 (M3)
- **Relaciona:** [ADR-0005](0005-motor-generico-dirigido-por-datos.md),
  [contrato-grpc.md](../contrato-grpc.md),
  [limites-y-estados.md](../diseno/limites-y-estados.md)

## Contexto

Hasta M3, el límite de una medida (p. ej. 4.2 V contra el rango 4.5–5.5)
estaba **embebido en el código del paso** (`pasos_demo::medir_voltaje` lo
comparaba y emitía `fallo`). Eso mezclaba dos cosas:

- el *cómo se mide* (código del paso, estable), y
- el *qué es aceptable* (umbral, cambia por lote/variante en producción).

Cambiar un umbral requería tocar y redistribuir el paso. RF-29 pide que los
límites sean **datos first-class**, y `contrato-grpc.md` ya fija que los
límites **no** viajan por `paso.proto` ("No lleva ... límites como datos
first-class"): viven en la definición de la secuencia.

Quedaba por decidir **quién** evalúa ese límite declarado como dato. Dos
opciones:

1. Un paso *built-in* "limit test" genérico que recibe el límite y la medida.
   Pero pasar el límite al paso exige extender `PeticionPaso` con el límite →
   contradice `contrato-grpc.md` (el límite no va en el cable) y rompe la
   estabilidad del contrato (RNF-05).
2. El motor evalúa el límite, declarado en la secuencia, contra la medida que
   devuelve el paso. El contrato no se toca.

## Decisión

**El paso devuelve la medida; el motor evalúa el límite declarado como dato y
produce el estado final.** `paso.proto` no cambia. El límite vive en el YAML
de la secuencia (`DefinicionPaso.limite: Option<Limite>`), y opcionalmente en
un sidecar (property loader, RF-30).

Implementación: `motor::aplicar_limite(def, resultado)`, invocada tras la
llamada gRPC en `ejecuta_con_reintentos`. Rellena los campos de límite del
`ResultadoStep` (para el reporte) y, si el paso dijo `paso` y hay
`valor_medido`, evalúa `Limite::evalua(valor)`.

### Regla fina: el límite solo empeora `paso` → `fallo`

El límite es una **regla de aceptación adicional**, no una absolución:

- Si el paso emitió `fallo` o `error` por sí mismo, se respeta — el paso es
  autoridad sobre su ejecución (sabe cosas que el límite no, p. ej. una
  comunicación rota). El motor **no** convierte `fallo`/`error` en `paso`.
- Solo si el paso dijo `paso` (la medición fue bien) y el límite no se cumple,
  el motor lo convierte en `fallo` y reescribe el mensaje al formato del
  límite (`"{valor} fuera de rango [{min}, {max}]"` o
  `"{valor} {op} {esperado} no cumplido"`).
- Si no hay `valor_medido` (pass/fail, action sin medida), el límite no aplica.

## ¿Es compatible con ADR-0005?

Sí. ADR-0005 dice que el motor **no conoce el dominio** (qué instrumento, qué
mide, qué significa). Una regla de aceptación high/low/comparación **declarada
como dato** no es conocimiento del dominio: es semántica genérica que la
secuencia entrega. El motor sigue sin saber que 4.2 es un voltaje; solo aplica
una comparación que le pasaron. ADR-0005 matiza "genérico ≠ tonto: el motor sí
aplica la semántica"; evaluar un límite declarado entra en esa semántica.

Lo que el motor **no** hace es interpretar el dominio del paso (sigue sin
saber qué mide); solo decide `paso`/`fallo` a partir de un `Limite` explícito y
de un número. La lógica de dominio especializada sigue perteneciendo al paso o
al expression engine (RF-35).

## Por qué `valor_esperado`/`operador` no van en `paso.proto`

El `ResultadoStep` enriquecido (con el límite aplicado) **no vuelve al
cable**: solo va a los `ResultSink`s. El motor lo construye a partir del
`ResultadoPasoProto` que llega del paso y lo enriquece con el `Limite` del
YAML. Por eso `ResultadoStep` gana `valor_esperado` y `operador` sin que
`paso.proto` los lleve — son campos del lado del motor/sinks, no del wire.

## Consecuencias

**Positivas:**

- Cambiar un umbral en producción = tocar el YAML (o el sidecar), no el
  código del paso ni el contrato. Es el valor de RF-29/RF-30 y la base del
  *online limit editing* (post-MVP).
- El contrato `paso.proto` sigue estable (RNF-05): no se añaden campos al
  cable.
- El paso queda más simple: solo mide y reporta que la medición fue bien; la
  regla de aceptación vive fuera.

**Negativas:**

- El estado que viaja por el cable deja de ser el estado final cuando hay
  límite: el paso manda `paso` (medí bien) y el motor produce el veredicto.
  Es un cambio de **interpretación**, no del contrato. Los pasos que no
  traigan límite conservan la semántnea anterior (el paso decide).
- El mensaje de un paso que falla por límite lo escribe ahora el motor, no el
  paso (formato estándar del límite).

## Alternativas descartadas

- **Pasar el límite al paso por `PeticionPaso`:** rompe `contrato-grpc.md` y
  RNF-05, y acopla el wire a la noción de límite.
- **Dejar el límite embebido en el paso:** no cumple RF-29; cambiar umbrales
  exige redistribuir código.

## Enlaces

- [ADR-0005](0005-motor-generico-dirigido-por-datos.md),
  [contrato-grpc.md](../contrato-grpc.md),
  [limites-y-estados.md](../diseno/limites-y-estados.md),
  [formato-de-secuencia.md](../diseno/formato-de-secuencia.md).