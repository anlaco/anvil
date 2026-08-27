# anvil

Un secuenciador de test: corre secuencias de pasos contra equipo real,
reintenta los que fallan y reporta el resultado. Escrito en **Rust
compilado a WASM** (`wasm32-wasip2`, bajo wasmtime).

La secuencia es **datos**, no código: el motor la recorre sin saber qué hace
cada paso, y cada paso se invoca **por gRPC por su nombre** — nunca con una
llamada directa. Eso aísla los pasos entre sí y deja la puerta abierta a
escribirlos en cualquier lenguaje.

## Documentación

La documentación de producto (visión, requisitos, arquitectura, ADRs,
diseño del dominio, licencia y roadmap) vive en [`docs/`](docs/README.md).
Empieza por [`docs/vision.md`](docs/vision.md).

## Correr el ejemplo

**Un binario** (`anvil`, ADR-0011) hospeda wasmtime y los dos guests WASM en
sandbox. Está enlazado estáticamente contra musl: no necesita Rust, ni cargo,
ni glibc, ni nada instalado en el sistema.

```sh
curl -LO https://github.com/anlaco/anvil/releases/download/v0.3.0/anvil-v0.3.0-x86_64-linux-musl.tar.gz
tar xzf anvil-v0.3.0-x86_64-linux-musl.tar.gz
cd anvil-v0.3.0-x86_64-linux-musl

./anvil ejemplos/subsecuencia.yaml --json ./out.json --csv ./out.csv
```

Linux x86_64, cualquier libc. La [página del release][rel] publica el SHA256
del `.tar.gz`; para comprobarlo, `sha256sum -c SHA256SUMS` con el segundo
asset descargado al lado. Los `.yaml` van en el paquete porque
`subsecuencia.yaml` invoca a `medir_fuentes.yaml` por path relativo.

[rel]: https://github.com/anlaco/anvil/releases/latest

## Compilar desde fuentes

Sólo hace falta si vas a tocar el código; para *usar* Anvil, descarga el
binario de arriba.

> **Antes de compilar: clona `wasi-grpc` al lado de este repo.** La pila gRPC
> se referencia por ruta relativa y todavía no está publicada en crates.io, así
> que sin ella `cargo` **no llega ni a leer el manifiesto**. El repo es público
> y Apache-2.0: clonarlo es todo lo que hace falta.
>
> ```sh
> git clone https://github.com/anlaco/anvil
> git clone https://github.com/anlaco/wasi-grpc   # hermano, no dentro
> cd anvil
> ```
>
> Es un apaño y está reconocido como tal: [#25](https://github.com/anlaco/anvil/issues/25).

```sh
make release   # guests WASM → puente → host, en ese orden

./packaging/anvil-host/target/release/anvil ejemplos/subsecuencia.yaml --json ./out.json --csv ./out.csv
```

Son tres compilaciones encadenadas (el `build.rs` del host copia los
artifacts, no los construye), y el orden importa; el `Makefile` existe para no
tener que recordarlo. A mano:

```sh
cargo build --release --target wasm32-wasip2 -p motor -p ejecutor_pasos      # guests
cargo build --release --manifest-path packaging/anvil-puente-wasm/Cargo.toml # puente (ADR-0015)
cargo build --release --manifest-path packaging/anvil-host/Cargo.toml        # host (wasmtime embebido)
```

Eso deja un binario enlazado contra la glibc de tu máquina, que es lo que
quieres para desarrollar. El **binario que se publica** en los releases es otra
cosa: se compila al target `x86_64-unknown-linux-musl` para que corra en
cualquier Linux. Requiere un compilador de C para musl, porque `wasmtime`
arrastra `zstd-sys`; sirve `musl-gcc` o `zig cc -target x86_64-linux-musl` tras
`rustup target add x86_64-unknown-linux-musl`. El puente hay que copiarlo a
`packaging/anvil-puente-wasm/target/release/` antes de compilar el host: su
`build.rs` busca los artifacts ahí, sin contemplar el subdirectorio del triple.

`make build` hace lo mismo en debug. Úsalo para desarrollar, pero cuenta con
que ese binario **arranca en decenas de segundos**: wasmtime compila los
guests sin optimizar cada vez. El de release arranca en ~1 s.

Para depurar los guests sueltos con el CLI de wasmtime (dos terminales):

```sh
cargo build --target wasm32-wasip2 -p ejecutor_pasos -p motor
# terminal 1
wasmtime -S cli -S tcp=y -S inherit-network=y \
  target/wasm32-wasip2/debug/ejecutor_pasos.wasm
# terminal 2
wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
  target/wasm32-wasip2/debug/anvil-guest.wasm ejemplos/basica.yaml
```

Los flags de wasmtime no son opcionales: sin `-S tcp=y -S
inherit-network=y` el guest no puede tocar la red. Más en la
[guía de inicio rápido](docs/guia-inicio-rapido.md).

## Estructura

```
crates/
  modelo/          modelo de datos + mensajes de paso.proto (prost)
  cargador/        YAML → modelo: valida, resuelve paths y detecta ciclos
  expr/            motor de expresiones (subconjunto de sintaxis Julia)
  result_sink/     sinks del reporte: consola, JSON, CSV
  pasos_demo/      los pasos de la secuencia de ejemplo
  pasos_scpi/      paso real por SCPI sobre TCP (ADR-0017)
  ejecutor_pasos/  servidor gRPC: despacha pasos por nombre
  motor/           cliente gRPC: recorre la secuencia (bin `anvil-guest`)
packaging/
  anvil-host/      host nativo: un binario que hospeda wasmtime + los dos guests
                   (workspace aparte; el core no arrastra wasmtime)
  anvil-puente-wasm/  puente gRPC ↔ componente WASM del usuario (ADR-0015);
                   va embebido en `anvil` y se extrae a temp al arrancar
```

La pila gRPC vive aparte, en
[`anlaco/wasi-grpc`](https://github.com/anlaco/wasi-grpc): gRPC sobre
sockets WASI nativos, porque `tonic`/`tokio` no compilan a WASM. anvil es su
primer consumidor y la dogfoodea.

## La especificación

Estas son las decisiones que definen el producto. No se tocan sin querer
tocarlas:

- **Semántica de ejecución.** Setup → Main (solo si el Setup fue bien) →
  Cleanup. El Main **corta en el primer fallo**; el Cleanup corre **siempre**
  — un equipo que se quedó encendido es peor que una secuencia que falló.
- **Reintentos por paso.** Cada paso declara cuántos intentos admite. El
  número de intento llega al paso, que puede usarlo.
- **Un vocabulario cerrado de estados:** `pass`, `fail`, `error` y `skipped`.
  En el agregado de la secuencia un `error` manda sobre un `fail`, y el motor
  puede añadir `inconclusive` cuando no ha podido juzgar (ADR-0019).
- **El contrato** está en `crates/modelo/paso.proto`: `StepRequest`,
  `StepResult` y `service StepExecutor { rpc Invoke, rpc Describe }`. Es la
  fuente de verdad; los structs `prost` de `crates/modelo/src/proto.rs` lo
  espejan a mano (wasi-grpc v0.1 no trae codegen). `Describe` devuelve el
  catálogo del ejecutor —qué pasos sirve y con qué firma— y es lo que permite
  a `--validate --with-executors` cazar un nombre mal escrito sin ejecutar
  nada (ADR-0021).

## Verificar

```sh
make test               # 342 tests del core + 26 del host + los del ejecutor Python
make check              # clippy de los tres workspaces
```

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
