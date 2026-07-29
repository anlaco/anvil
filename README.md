# anvil

Un secuenciador de test: corre secuencias de pasos contra equipo real,
reintenta los que fallan y reporta el resultado. Escrito en **Rust
compilado a WASM** (`wasm32-wasip2`, bajo wasmtime).

La secuencia es **datos**, no código: el motor la recorre sin saber qué hace
cada paso, y cada paso se invoca **por gRPC por su nombre** — nunca con una
llamada directa. Eso aísla los pasos entre sí y deja la puerta abierta a
escribirlos en cualquier lenguaje.

## Correr el ejemplo

```sh
cargo build --target wasm32-wasip2 -p ejecutor_pasos -p motor

# terminal 1
wasmtime -S cli -S tcp=y -S inherit-network=y \
  target/wasm32-wasip2/debug/ejecutor_pasos.wasm

# terminal 2
wasmtime -S cli -S tcp=y -S inherit-network=y \
  target/wasm32-wasip2/debug/basica_datos.wasm
```

Los flags de wasmtime no son opcionales: sin `-S tcp=y -S
inherit-network=y` el guest no puede tocar la red.

## Estructura

```
crates/
  modelo/          modelo de datos + mensajes de paso.proto (prost)
  pasos_demo/      los pasos de la secuencia de ejemplo
  ejecutor_pasos/  servidor gRPC: despacha pasos por nombre
  motor/           cliente gRPC: recorre la secuencia
```

La pila gRPC vive aparte, en
[`anlaco/wasi-grpc`](https://github.com/anlaco/wasi-grpc): gRPC sobre
sockets WASI nativos, porque `tonic`/`tokio` no compilan a WASM. anvil es su
primer consumidor y la dogfoodea.

## La especificación

Estas son las decisiones que definen el producto. Sobrevivieron a un cambio
completo de lenguaje, y no se tocan sin querer tocarlas:

- **Semántica de ejecución.** Setup → Main (solo si el Setup fue bien) →
  Cleanup. El Main **corta en el primer fallo**; el Cleanup corre **siempre**
  — un equipo que se quedó encendido es peor que una secuencia que falló.
- **Reintentos por paso.** Cada paso declara cuántos intentos admite. El
  número de intento llega al paso, que puede usarlo.
- **Tres estados:** `paso`, `fallo`, `error`. En el agregado de la secuencia,
  un `error` manda sobre un `fallo`.
- **El contrato** está en `secuenciador/rpc/paso.proto`: `PeticionPaso`,
  `ResultadoPasoProto`, `service EjecutorPasos { rpc Invoca }`. Es la fuente
  de verdad; los structs `prost` de `crates/modelo/src/proto.rs` lo espejan a
  mano (wasi-grpc v0.1 no trae codegen).

## Verificar

```sh
cargo test              # tests unitarios
./verifica_paridad.sh   # la secuencia de ejemplo en 4 combinaciones
```

`verifica_paridad.sh` corre `basica_datos` con motor y ejecutor en Rust, en
Ana, y **cruzados en las dos direcciones**, exigiendo que las cuatro salidas
sean idénticas. Las cruzadas son las que prueban de verdad que el contrato
gRPC se respeta byte a byte y que las dos implementaciones son
intercambiables.

## Licencia

**anvil es AGPL-3.0-or-later** (ver [LICENSE](LICENSE)). anvil es el
producto: se *usa*, no se linka. La AGPL impide que alguien lo cierre y lo
revenda, y **no afecta a tus secuencias de test** — son datos que le pasas al
secuenciador, no obra derivada de él. Los límites de aceptación y el know-how
de producto que hay en una secuencia son tuyos y siguen siendo tuyos.

Las librerías sobre las que se apoya van deliberadamente **Apache-2.0**:

| Pieza | Licencia | Por qué |
|---|---|---|
| Interfaces WIT | Apache-2.0 | Queremos que se adopten como referencia |
| `wasi-grpc`, `wasi-visa` | Apache-2.0 | Se linkan en código ajeno |
| anvil | AGPL-3.0 | Es el producto |

Un paso de test se **linka** con las librerías, así que copyleft ahí
contagiaría el código de quien las use. En el secuenciador no ocurre.

## La versión en Ana

El proyecto se escribió primero en [Ana](https://github.com/anlaco/anlaco-lang)
(archivos `.ana`), y ese código **sigue aquí y sigue funcionando**:

```
grpc/                 pila HTTP/2 + HPACK + protobuf en Ana
secuenciador/*.ana    modelo, ejecutor, pasos, motor y ejecutor gRPC
```

No es histórico muerto: es la referencia contra la que se verifica la
paridad, y la prueba viva de que el contrato es independiente del lenguaje.
Correrlo necesita `bin/anac`, que no está versionado (es un artefacto de
build — ver `bin/VERSION.md`).

Ana la desarrolla un equipo independiente. Si algo hace falta y Ana no lo
tiene, se abre un issue en su repositorio (`gh issue create --repo
anlaco/anlaco-lang`) — nunca se arregla desde aquí. La guía del lenguaje,
con el protocolo para reportar, está en `.claude/skills/ana/`.
