# Contribuir a Anvil

Gracias por interesarte en Anvil, un secuenciador de test open-source que
compite con NI TestStand. Esta guía describe cómo montar el entorno, qué
convenciones seguimos y cómo enviar cambios.

> Documentación de producto: [`docs/README.md`](docs/README.md). Lee
> [`docs/vision.md`](docs/vision.md) y los [ADRs](docs/adr/) antes de
> cambios arquitectónicos.

## Setup del entorno

Requisitos:

- **Rust stable** con el target `wasm32-wasip2` (gestionado por
  `rust-toolchain.toml`; `rustup` lo instala solo).
- **[wasmtime](https://wasmtime.dev/)** para correr los `.wasm`.
- **`wasi-grpc`** (repo aparte) clonado **junto a** este repo, porque se
  referencia por ruta (`../wasi-grpc`):

```
$ ls ..
anvil/        # este repo
wasi-grpc/    # la pila gRPC, github.com/anlaco/wasi-grpc
```

Compilar:

```sh
cargo build --target wasm32-wasip2 -p ejecutor_pasos -p motor
```

Correr el ejemplo (dos terminales):

```sh
# terminal 1 — ejecutor de pasos
wasmtime -S cli -S tcp=y -S inherit-network=y \
  target/wasm32-wasip2/debug/ejecutor_pasos.wasm

# terminal 2 — motor con la secuencia "basica"
wasmtime -S cli -S tcp=y -S inherit-network=y \
  target/wasm32-wasip2/debug/basica_datos.wasm
```

Los flags `-S tcp=y -S inherit-network=y` **no son opcionales**: sin ellos el
guest no toca la red.

## Tests

```sh
cargo test              # tests unitarios (modelo, proto, pasos_demo)
```

Los tests cubren el contrato ida/vuelta, el agregado de estados y el
despacho por nombre. Un cambio al contrato (`paso.proto`) **o** a
`crates/modelo/src/proto.rs` debe actualizar los tests de `proto.rs`.

## Qué se puede tocar

- ✅ Documentación en `docs/` y archivos de comunidad en la raíz.
- ✅ Código en `crates/` siguiendo las decisiones de los ADRs.
- ⚠️ `crates/modelo/paso.proto` y `crates/modelo/src/proto.rs` son
  **superficie pública**: un cambio rupturista exige un ADR (ver
  [`docs/contrato-grpc.md`](docs/contrato-grpc.md)) y mantener los dos
  archivos espejados a mano (wasi-grpc v0.1 sin codegen).
- ⚠️ La **semántica de ejecución** (Setup/Main/Cleanup, reintentos, agregado)
  es spec: no se cambia sin un ADR (ver
  [`docs/diseno/motor-de-ejecucion.md`](docs/diseno/motor-de-ejecucion.md)).

## Convenciones

- **Idioma: inglés, en todo.** Anvil es un producto open-source
  internacional, así que desde el 28/08/2026 van en inglés el código
  —identificadores, comentarios y mensajes de error—, los mensajes de commit,
  los ADRs y `docs/`. La interfaz ya lo estaba: el YAML de secuencia,
  `paso.proto`, el WIT, las claves del JSON, las columnas del CSV, los estados y
  los flags del CLI. La traducción de cada término está fijada en
  [`GLOSSARY.md`](GLOSSARY.md): consúltalo antes de nombrar algo nuevo en la
  superficie pública.
  **No es una reescritura de golpe.** Lo que sigue en español se traduce por la
  regla del *Boy Scout* —el fichero que se abre para modificarlo se traduce, en
  un commit aparte del cambio que motivó abrirlo—, así que un fichero entero en
  español hoy no es una anomalía, es el punto de partida. Documenta con la
  herramienta nativa del lenguaje: `rustdoc` en Rust.
- **Commits:** [conventional commits](https://www.conventionalcommits.org/)
  (`feat:`, `fix:`, `chore:`, `docs:`, `refactor:`…) **y en inglés**. No es
  retroactivo: el historial ya escrito en español se queda como está. Mira el
  historial reciente.
- **Estilo de código:** el del entorno (`cargo fmt`); comentarios como los
  existentes: concisos, explican el *por qué*.

## Firmado (DCO)

Cada commit se firma con **DCO** (*Developer Certificate of Origin*):
confirma que eres autor del cambio y tienes derecho a licenciarlo bajo
AGPL-3.0-or-later (ver [`docs/licencia.md`](docs/licencia.md)).

```sh
git commit -s -m "feat: ..."
```

El `-s` añade la línea `Signed-off-by:`. No se exige CLA en esta fase.

## Flujo de PR

1. Abre un issue primero para cambios grandes (cambios al contrato, a la
   semántica o nuevos ADRs). Para arreglos pequeños, va directo a PR.
2. Rama desde `main`, commits firmados con DCO.
3. Describe el *qué* y el *por qué*; enlaza el issue y, si aplica, el ADR.
4. `cargo test` verde.
5. Revisión por un mantenedor (ver [`GOVERNANCE.md`](GOVERNANCE.md)).

## Reporte de seguridad

Las vulnerabilidades **no** se reportan por issue público. Ver
[`SECURITY.md`](SECURITY.md). Ten en cuenta que Anvil opera **hardware
real**: un bug puede tener riesgo físico, no solo de software.