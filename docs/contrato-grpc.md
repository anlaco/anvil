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

message PeticionPaso {
  string nombre = 1;   // el paso a invocar (despacho por nombre, ADR-0003)
  int32  intento = 2;  // nº de intento, desde 1 (para simular fallos transitorios)
}

message ResultadoPasoProto {
  string nombre = 1;
  string estado = 2;     // "paso" | "fallo" | "error"  (texto, no enum)
  string mensaje = 3;
  string valor_medido = 4;   // medida: como string
  string limite_min = 5;
  string limite_max = 6;
}

service EjecutorPasos {
  rpc Invoca(PeticionPaso) returns (ResultadoPasoProto);
}
```

- **Sin `package`** en el `.proto`, así que la ruta del método es
  directamente `/EjecutorPasos/Invoca` (constante `RUTA_INVOCA` en `proto.rs`).
- **Un método, unaria** (unary RPC): una petición → una respuesta. Sin
  streaming en el MVP.

## Semántica de campos

### `PeticionPaso`

| Campo | Tipo | Significado |
|---|---|---|
| `nombre` | string | El paso a invocar. El ejecutor lo ata a una función en `despacha`; desconocido → `error` (RF-12). |
| `intento` | int32 | Número de intento **desde 1**. Llega al paso para simular fallos transitorios (ver `pasos_demo::conectar`: falla el 1, pasa el 2+). |

### `ResultadoPasoProto`

| Campo | Tipo | Significado |
|---|---|---|
| `nombre` | string | Nombre del paso (devuelto por el paso). |
| `estado` | string | `"paso"` / `"fallo"` / `"error"`. **Texto, no enum**: viaja así y admite pasos en cualquier lenguaje (RF-10). El motor solo interpreta esto para el agregado. |
| `mensaje` | string | Texto humano del resultado. |
| `valor_medido` | string | La medida, como texto. **Vacío** si el paso no mide (Pass/Fail). |
| `limite_min` / `limite_max` | string | Límites high/low, como texto. Vacíos si no aplican. |

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
- **Política propuesta (MVP):** cambios *aditivos* (campos nuevos con tags
  altos, semántica backward-compatible) son permitidos; cambios *rupturistas*
  (renombrar/quitar campos, alterar semántica de estados) exigen un ADR nuevo
  y, idealmente, un proceso RFC (ver [roadmap.md](roadmap.md), diferido).
- Hoy el contrato **no declara versión** en el wire. Cuando haya más de un
  ejecutor o pasos externos, se necesita un mecanismo de versión (campo o
  ruta versionada) — **pendiente**.

## Extensión futura: introspección de firma del paso (post-MVP)

> **No implementada.** Es un hueco explícito para el editor visual
> ([diseno/ui-vs-headless.md](diseno/ui-vs-headless.md)).

Hoy el contrato solo describe *cómo invocar* (`PeticionPaso`) y *qué
devuelve* (`ResultadoPasoProto`), pero **no describe la firma del paso**: ni
qué parámetros admite, ni qué retorna tipadamente. Para que un editor
visual (drag-and-drop del archivo del code module) pueda, como TestStand,
**auto-descubrir y actualizar los parámetros y el valor de retorno** del
paso, hace falta que el paso **exponga metadatos de su firma**.

**Propuesta de extensión (a confirmar con ADR):**

- Un mecanismo de **descripción de paso**: p. ej. un nuevo RPC de
  introspección en `EjecutorPasos` (p. ej. `rpc Describe(DescribePaso) returns
  (FirmaPaso)`) o un sidecar de metadatos, que devuelva parámetros (nombre,
  tipo, dirección in/out) y tipo de retorno para un `nombre` dado.
- Esto permitiría al editor arrastrar un `.vi`/`.dll`/`.py`/`.scilab` y
  auto-poblar la tabla de parámetros del paso, igual que TestStand.

**Tensión a resolver:** la firma introspeccionable vive en el lado del paso
(el ejecutor la provee), no en el motor. El motor sigue siendo genérico
(ADR-0005); solo el editor y el ejecutor necesitan entender la firma. Es un
**extensión del lado del ejecutor**, no del núcleo.

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
atiende un **ejecutor de lenguaje** distribuido (`executores/`, p. ej.
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