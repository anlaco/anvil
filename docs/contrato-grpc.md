# Contrato gRPC del paso

La **superficie pública** del paso: lo que el motor envía y lo que el paso
devuelve, por gRPC. Es lo estable; lo interno de cada paso es opaco.

**Fuente de verdad:** [`crates/modelo/paso.proto`](../crates/modelo/paso.proto).
Los structs `prost` de
[`crates/modelo/src/proto.rs`](../crates/modelo/src/proto.rs) lo **espejan a
mano** (wasi-grpc v0.1 no trae codegen, [ADR-0006](adr/0006-wasi-grpc-propio.md)):
si se toca uno, hay que tocar el otro.

## El contrato (hoy)

```proto
syntax = "proto3";

message Reference {          // ADR-0022: un objeto que se queda en el ejecutor
  string executor = 1;       // lo estampa Anvil
  string lifetime = 2;       // la vida del ejecutor, de su Catalog
  string payload  = 3;       // opaco: lo acuña el ejecutor, Anvil no lo lee
}

message Value {
  string name = 1;
  oneof value {              // sin rama puesta = error, no un cero
    double    number    = 2;
    string    text      = 3;
    bool      boolean   = 4;
    Reference reference = 5;
  }
}

message StepRequest {
  string name = 1;     // el paso a invocar (despacho por nombre, ADR-0003)
  int32  attempt = 2;  // nº de intento, desde 1 (para simular fallos transitorios)
  repeated Value inputs = 3;  // ya evaluados por el motor (ADR-0020)
  int32  contract = 4;            // la versión que habla el motor
}

message StepResult {
  string nombre = 1;
  string status = 2;     // "pass" | "fail" | "error" | "skipped"
  string message = 3;
  string measured_value = 4; // medida: como string
  string limit_min = 5;
  string limit_max = 6;
  repeated Value outputs = 7;  // valores con nombre, aparte de la medida
  int32  contract = 8;         // el eco: la versión que el ejecutor entendió
}

service StepExecutor {
  rpc Invoke(StepRequest) returns (StepResult);
}
```

- **Sin `package`** en el `.proto`, así que la ruta del método es
  directamente `/StepExecutor/Invoke` (constante `RUTA_INVOCA` en `proto.rs`).
- **Un método, unaria** (unary RPC): una petición → una respuesta. Sin
  streaming en el MVP.

## Semántica de campos

### `StepRequest`

| Campo | Tipo | Significado |
|---|---|---|
| `name` | string | El paso a invocar. El ejecutor lo ata a una función en `despacha`; desconocido → `error` (RF-12). |
| `attempt` | int32 | Número de intento **desde 1**. Llega al paso para simular fallos transitorios (ver `pasos_demo::conectar`: falla el 1, pasa el 2+). |
| `inputs` | repeated Valor | Los parámetros del paso, **ya evaluados**: el motor resuelve las expresiones `${...}` del YAML contra su entorno antes de llamar (ADR-0009). El paso no ve `locals`; se le pasan valores. Un `oneof` sin rama es `error` (ADR-0019, Regla 2). |
| `contract` | int32 | La versión de contrato que habla el motor. Ver «Versionado» más abajo. |

### `StepResult`

| Campo | Tipo | Significado |
|---|---|---|
| `name` | string | Nombre del paso (devuelto por el paso). |
| `status` | string | `"pass"` / `"fail"` / `"error"`. **Texto, no enum**: viaja así y admite pasos en cualquier lenguaje (RF-10). El motor solo interpreta esto para el agregado. |
| `mensaje` | string | Texto humano del resultado. |
| `measured_value` | string | La medida, como texto. **Vacío** si el paso no mide (Pass/Fail). |
| `limit_min` / `limit_max` | string | Límites high/low, como texto. Vacíos si no aplican. |
| `outputs` | repeated Valor | Valores con nombre que devuelve el paso **además** de la medida. No participan en el veredicto: `asigna` los lee como `resultado.salidas.<nombre>`. |
| `contract` | int32 | **El eco**: la versión que el ejecutor ha entendido. Ver «Versionado». |

## Versionado del contrato (ADR-0020 §4)

Un entero monótono, `contrato`, en la petición y en la respuesta. El motor
manda el que habla; el ejecutor devuelve **el que ha entendido**. No hay rutas
versionadas ni RPC de saludo: dos mecanismos para lo mismo divergen.

| Versión | Qué trae |
|---|---|
| 1 | El contrato original: `PeticionPaso{nombre, intento}`. Un ejecutor de contrato 1 no conoce el tag 8 y devuelve `0` por el default de proto3 — así es como se le reconoce. |
| 2 | Parámetros de entrada y salidas con nombre. |
| 3 | El contrato en inglés: `StepRequest`/`StepResult`, `inputs`/`outputs`, y los estados `pass`/`fail`/`skipped`. |
| 4 | La **referencia a objeto**: una cuarta rama del `oneof`, un cuarto `ValueType` y `Catalog.lifetime` (ADR-0022). |

**Por qué hace falta el eco.** Un campo aditivo es «compatible» sólo en el
sentido de que el mensaje decodifica. Un ejecutor de contrato 1 ignora
`parametros` —proto3 se lo permite—, **mide otra cosa y dice `paso`**. Ese
verde falso no lo delata ninguna otra señal. Por eso:

> Si el paso declaró `parametros` (o su `asigna` lee `salidas`) y el eco es
> menor que 3, el paso es **`error`**, nombrando el endpoint y las dos
> versiones. Nunca `fallo`, y nunca se ejecuta con los parámetros perdidos.

Y su recíproco, que es lo que mantiene vivo lo que ya funciona: si el paso
**no** declara parámetros ni lee salidas, un ejecutor de contrato 1 sigue
siendo válido y no cambia nada.

**Cuándo sube el número.** No es «aditivo vs. rupturista»:

> Sube `contrato` todo cambio en el que **el silencio de un par antiguo pueda
> alterar un veredicto**. Lo que un par puede ignorar sin que la afirmación
> sobre la unidad cambie (un campo informativo, una traza), no lo sube.

Retirar o renombrar un tag exige ADR, entrada *breaking* en el CHANGELOG y
`reserved` sobre el tag: **un tag no se reutiliza jamás**.

**Los pasos WASM no ven este número.** El WIT se versiona por recompilación
(`anvil:step@0.4.0`) y **es el puente quien responde el eco** por ellos: un
componente no sabe de gRPC ni de versiones (ADR-0015).

## The object reference (ADR-0022)

A step sequence needs several steps to work on the same bench state — the
*rack*. That object **cannot cross the wire**: it holds open sockets and vendor
driver locks. So it stays in the executor, and what travels is a reference to
it.

### It names a slot, not an object

Mutating the state behind a reference does not change its identity: a step that
reconfigures the bench answers **the reference it was given**. A new one is
minted only when another object was really born — deriving one configuration
from another, duplicating.

This is not a preference. `ejecuta_con_reintentos` evaluates a step's
parameters **once** and re-sends the same ones on every attempt, so an attempt
that handed back a new handle would leave the next attempt holding one the
executor already considers spent. It also means a loop over two hundred units
leaves nothing orphaned in the executor's map, and that forgetting an `assign`
cannot quietly leave the following steps using the bench *before* it was
configured.

### Who mints what

| Field | Minted by | Why |
|---|---|---|
| `executor` | **Anvil**, on receiving it | The process opposite does not know what the sequence called it: the names live in the YAML's `executors:`, which is also what the engine routes on (ADR-0013). |
| `lifetime` | The **executor**, on start-up | It is published in `Catalog.lifetime` and is how a restart becomes detectable. Empty is legitimate and means "unchecked", never "fine". |
| `payload` | The **executor** | Opaque. Anvil never interprets it, never composes one, and accepts none written by hand. |

### Two duties the contract cannot verify

An executor that breaks either is a broken executor, and Anvil cannot see it
from outside:

1. **Never recycle a payload within one lifetime.** If a closed bench's key
   came back for the next open, an old reference would resolve cleanly to a
   live, *different* object: same executor, same lifetime, everything green,
   measuring against the wrong bench.
2. **Mint a different lifetime on every start.**

### What Anvil refuses, and when

| Refusal | When | Needs the executor up? |
|---|---|---|
| A reference in an operation — arithmetic, comparison, a limit, a verdict | Evaluating the expression | no |
| A reference literal written into the sequence | Loading | no |
| A handle passed to a step of another executor, or filled by one | Loading | no |
| A reference declared on a `type: wasm` executor | Loading | no |
| A reference variable written by a `statement`, or from anything but `result.outputs.<name>` | Loading | no |
| An executor that minted under a life its own catalog contradicts | On reading the result | — |
| The executor restarted, or stopped answering | **Before invoking** the step that carries the handle | yes |

The last one is the only one that costs a round trip, and only for a step that
actually carries a handle to an executor that publishes a lifetime. `Describe`
is asked again there and **only its lifetime is read** — the signatures are
still checked exactly once, at start-up (ADR-0021 §3), so the report stays
reconstructible. The verdict is a step in `error` and never an abort: a run
that stops in its tracks does not run its `cleanup`, and that is precisely the
moment Anvil most wants the step that closes the rack to run.

### WASM does not carry one, and says so

`anvil:step` is `run(name, attempt, inputs) -> step-result` plus `describe()`:
functions, with no resources and no state between calls (ADR-0020 §4d), so a
component has nowhere to keep the map. A reference reaching one is an **explicit error and never a
silence** — refused at load if the executor is declared `type: wasm`, and again
at the bridge. Giving WASM state is a decision with its own ADR (ADR-0022 §8).

## Codificación de medidas

Los tres campos de medida van como **`string`**, no como `double`. Razones
(impuestas por el contrato actual; ver `proto.rs::a_texto`/`de_texto`):

1. **`proto3`: un `string` vacío no se transmite.** Un resultado sin medida
   (Pass/Fail) solo viaja con los tres primeros campos (`nombre`, `estado`,
   `mensaje`). Si fueran `double`, un `0.0` sería ambiguo con "no hay medida".
2. **Enteros sin decimales:** `5.0` se codifica como `"5"`; `4.2` como
   `"4.2"`. Legible y sin ruido (`a_texto`).
3. **Agnóstico de precisión:** el contrato no fija precisión de coma flotante;
   el paso decide cómo representar su medida.

La conversión ida/vuelta `Option<f64> ↔ string` está en `proto.rs` y está
testeada (`ida_y_vuelta_con_medida`, `campos_vacios_no_viajan`,
`entero_sin_decimales`).

## Estados como texto

El `estado` es `string`, no un `enum` protobuf. Es deliberado (ADR-0005):

- El contrato admite **pasos escritos en cualquier lenguaje**; un enum
  protobuf ataría a quien genere bindings.
- El motor **solo** necesita distinguir `paso`/`fallo`/`error` para la
  semántica (corte en 1er fallo, agregado `error > fallo`); cualquier otro
  valor se trataría como no-`paso`.

> Implicación: el motor confía en que el paso emita exactamente uno de los
> tres textos. Un valor distinto no es `paso` → se comporta como fallo en el
> agregado. Restringirlo es responsabilidad del lado del paso (o de un
> validator futuro).

## Versionado del contrato

- `paso.proto` es **superficie pública crítica** (RNF-05). No se rompe sin
  un ADR/RFC.
- **Política decidida en [ADR-0020](adr/0020-parametros-del-paso-en-la-peticion.md)
  — aún no implementada.** Un entero monótono `contrato` en la petición y en la
  respuesta, que el ejecutor devuelve como eco de lo que ha entendido; sube con
  todo cambio cuyo silencio en un par antiguo pueda alterar un veredicto.
  Retirar o renombrar un tag exige ADR y entrada *breaking*; un tag no se
  reutiliza. Sin rutas versionadas ni RPC de saludo.
- **Hoy** el wire **no declara versión** — eso es el contrato 1, y es lo que
  describe este documento. El campo `contrato`, los parámetros y las salidas
  son el contrato 2 y llegan cuando se implemente ADR-0020.

## Introspección de firma: `Describe` (ADR-0021, implementada)

> Era la «extensión futura» de este documento y el
> [issue #45](https://github.com/anlaco/anvil/issues/45). Se decidió e
> implementó el 27/08/2026 en
> [ADR-0021](adr/0021-el-ejecutor-describe-su-catalogo.md); lo que sigue
> describe lo que hay, no lo que se propone.

`Invoke` dice *cómo invocar* y *qué devuelve* a nivel de mensaje, pero no qué
pasos existen ni qué acepta cada uno. Eso lo dice un RPC aparte, en el mismo
servicio:

```proto
rpc Describe(CatalogRequest) returns (Catalog);
```

`Catalog` trae un `StepSpec` por paso servido: nombre, entradas (nombre, tipo,
obligatorio, valor por defecto), salidas y una línea de documentación. Los
tipos son **los cuatro del cable** (número, texto, booleano, referencia): esto
describe el cable, no inventa un sistema de tipos. El de referencia es **plano**
—dice que es un handle, no de qué clase— porque un tipo por clase exigiría que
Python, Java y LabVIEW se pusieran de acuerdo en cómo se escribe un nombre de
clase, y eso es la IDL que ADR-0022 descarta.

`Catalog` trae además **`lifetime`**: la vida que el ejecutor acuña al arrancar
(ADR-0022 §6). Es lo que permite a Anvil enterarse de que el proceso que tiene
sus referencias se murió y volvió a nacer — una pregunta sobre el mundo, no
sobre la secuencia, que ningún sistema de tipos contesta. Vacío es legítimo y
significa «no publico ninguna»: entonces Anvil **avisa** de que las referencias
contra ese ejecutor no se pueden comprobar por vida, en vez de suponer que
están bien.

**Cuándo se pregunta.** Una vez por endpoint, al arrancar, antes del primer
paso. Nunca paso a paso: enterarse en el paso 47 de que un nombre está mal deja
la unidad medio probada, y un catálogo que cambiara a mitad de corrida haría el
informe irreconstruible.

**Qué se comprueba con eso.** Que el paso exista en el ejecutor al que se
despacha; que sus `inputs` sean parámetros que el paso admite; que no falte
ninguno obligatorio; que un literal sea del tipo declarado; y que
`assign: result.outputs.<nombre>` lea una salida que el paso devuelve — que era
justamente la excepción declarada en ADR-0020 §3 a la regla de detección de
ADR-0019. Un hallazgo detiene la corrida antes del primer paso.

**No contestar está permitido, y se nota.** Un ejecutor de terceros puede no
implementarlo; entonces sus pasos salen como *sin comprobar*, con el motivo y el
recuento. Ni error (cerraría la puerta a terceros) ni silencio (sería el verde
falso de ADR-0019). Para que «no describo» no se confunda con «no sirvo ningún
paso», `Catalog` lleva un booleano `describes`: el default `false` de proto3
hace que **todo silencio caiga del lado seguro**. Es el mismo truco que el eco
de contrato.

**El puente WASM sí describe, desde `anvil:step@0.4.0`.** Hasta esa versión el
WIT exportaba un único `run(name, attempt, inputs)` y el componente despachaba
por nombre dentro de sí mismo: desde fuera no había lista que publicar, y el
puente contestaba `describes = false`. Era la factura del despacho por nombre
(ADR-0003), y se ha pagado tocando el WIT — que es lo que esperaba el issue #39
(ADR-0024). El componente publica su catálogo por `describe` y el puente lo
traduce; una lista vacía se responde como `describes = false`, porque un
componente que no sirve ningún paso no tiene nada que hacer y la lectura segura
es la única útil.

**Añadir este RPC no sube `contrato`.** Un ejecutor que no describe su catálogo
mide exactamente lo mismo: su silencio no puede alterar un veredicto, que es la
regla de ADR-0020 §4c.

**Y sigue sin ser del núcleo.** La firma vive en el lado del ejecutor, que es
quien la provee; el motor sólo pregunta y compara nombres. Sigue sin saber qué
mide un paso (ADR-0005).

Con esto, el editor visual de
[diseno/ui-vs-headless.md](diseno/ui-vs-headless.md) tiene ya la mitad que le
faltaba: arrastrar un code module y auto-poblar su tabla de parámetros es leer
este catálogo.

## Lo que NO es el contrato

- **No** lleva variables, límites como datos first-class, ni
  precondiciones. Esos viven en la definición de la secuencia
  ([diseno/formato-de-secuencia.md](diseno/formato-de-secuencia.md)) y en el
  *expression engine* ([diseno/motor-de-expresiones.md](diseno/motor-de-expresiones.md)),
  no en el cable del paso.
- **No** describe el process model. La separación secuencia vs. proceso de
  test es de nivel superior ([diseno/proceso-de-test.md](diseno/proceso-de-test.md)).

## Reuso hacia ejecutores externos (ADR-0013)

El contrato **no cambia** desde el punto de vista del motor: un paso lo
atiende un **ejecutor de lenguaje** distribuido (`executors/`, p. ej.
Python), un **componente `.wasm` cargado por path** (M5-ext.2; lo carga el
host, ADR-0015) o el ejecutor embebido — el motor siempre habla el mismo
`paso.proto` por gRPC y solo añade routing **nombre→endpoint** en su lado
(`ejecutores:`/`ejecutor:` en el YAML, M5-ext.1).

Lo que cambia es **dentro del ejecutor**: un componente `.wasm` de paso no
habla `paso.proto` — exporta la función `run` (interfaz WIT `anvil:paso`,
ADR-0015) y el **puente** (`anvil-puente-wasm`, nativo) traduce
`paso.proto` ↔ `anvil:paso` por cada `Invoca`. `paso.proto` sigue siendo
la superficie pública del cable (RNF-05); la traducción vive en el puente,
que es código de Anvil. Ver
[diseno/executores-lenguaje.md](diseno/executores-lenguaje.md),
[ADR-0013](adr/0013-cargador-wasm-host-side-y-routing.md) y
[ADR-0015](adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md).