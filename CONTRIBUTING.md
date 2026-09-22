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

En Windows, el mismo flujo aplica bajo Git Bash — el `Makefile` es
POSIX-portable y así es como lo ejerce el job `ci-windows` (ADR-0036). Para
el Sequence Editor, además de lo de arriba hace falta Node; Electron y
`electron-builder`, que son con lo que se abre la ventana y se construye el
instalador (ADR-0037), los trae `npm install` dentro de `editor/`.

En Linux, Chromium no arranca sin su sandbox y necesita que
`editor/node_modules/electron/dist/chrome-sandbox` sea de root con el bit
setuid (`sudo chown root:root …` y `sudo chmod 4755 …`). `npm run app` lo
comprueba y dice el comando exacto si falta; para saltarlo en una sesión de
desarrollo, `ANVIL_EDITOR_NO_SANDBOX=1 npm run app`.

Compilar (guest del motor, puente WASM, componentes de ejemplo y host, en
ese orden):

```sh
make release
./packaging/anvil-host/target/release/anvil ejemplos/basica.yseq
```

`anvil` no lleva ejecutor de pasos propio
([ADR-0041](docs/adr/0041-there-is-no-embedded-executor.md)) y **no arranca
ninguno** ([ADR-0046](docs/adr/0046-an-executor-is-an-address-and-nothing-brings-one-up.md)):
los ejemplos dicen dónde escucha el banco de demo y hay que levantarlo antes.

```sh
./ejemplos/arrancar-banco.sh &
./packaging/anvil-host/target/release/anvil ejemplos/basica.yseq
```

Para correr el guest del motor suelto con wasmtime (dos terminales), se
arranca el ejecutor a mano y se le pasa al motor:

```sh
# terminal 1 — el ejecutor del banco de demo
executors/wasm/target/release/anvil-exec-wasm --modules ejemplos/departamento/dist --port 9300

# terminal 2 — motor con la secuencia "basica"
wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
  target/wasm32-wasip2/release/anvil-guest.wasm ejemplos/basica.yseq \
  --executor demo=127.0.0.1:9300
```

Los flags `-S tcp=y -S inherit-network=y` **no son opcionales**: sin ellos el
guest no toca la red.

## Tests

```sh
cargo test              # tests unitarios del core (modelo, cargador, expr, motor, sinks)
make test               # todas las suites: core, puente, host, SDKs, editor
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

El `-s` añade la línea `Signed-off-by:`.

**Hoy el DCO es lo único que se pide.** Está previsto añadir un acuerdo de
contribución —del tipo del [FLA-2.0 de la FSFE][fla]— que permita a Anvil
conceder **licencias adicionales** a quien no pueda usar AGPL, y que a cambio
comprometa a Anvil a seguir publicando bajo AGPL, con **reversión de los
derechos a sus autores** si alguna vez dejara de hacerlo. La dirección está
decidida en [ADR-0032](docs/adr/0032-contributions-come-with-a-reversion-clause.md);
el texto no está adoptado ni revisado, así que **no se pide firmar nada**. Esta
nota está aquí para que nadie contribuya sin saber hacia dónde va.

Tu autoría es tuya en cualquier caso: tu nombre queda en la historia y en el
`Signed-off-by`.

[fla]: https://fsfe.org/activities/fla/fla.en.html

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