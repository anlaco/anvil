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

message Value {
  string name = 1;
  oneof value {              // sin rama puesta = error, no un cero
    double number  = 2;
    string text    = 3;
    bool   boolean = 4;
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
(`anvil:step@0.3.0`) y **es el puente quien responde el eco** por ellos: un
componente no sabe de gRPC ni de versiones (ADR-0015).

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
tipos son **los tres de siempre** (número, texto, booleano): esto describe el
cable, no inventa un sistema de tipos.

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

**El puente WASM contesta que no describe.** `anvil:step` exporta un único
`run(name, attempt, inputs)` y el componente despacha por nombre dentro de sí
mismo: desde fuera no hay lista que publicar. Es la factura del despacho por
nombre (ADR-0003), y hacerlo describible exige tocar el WIT — la decisión que
espera el issue #39.

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