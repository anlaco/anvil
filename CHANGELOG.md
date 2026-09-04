# Changelog

Notable changes to Anvil. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versioning is
[SemVer](https://semver.org/).

Anvil is in 0.x: the public surface — the YAML sequence format, the
`paso.proto` contract and the textual report (RNF-08) — may change between
minors, with the change written down here.

## [Unreleased]

### Fixed

- **The CLI accepts absolute paths again** (#40): `--json`, `--csv`,
  `--limits`, `--process-model` and the sequence path itself used to fail
  with `os error 44` whenever the path was absolute, even for a file that
  existed and was inside the current directory. Only the cwd was preopened
  into the engine's WASI sandbox; an absolute path fell outside that
  preopen's prefix. The host now also preopens each absolute path argument's
  parent directory.

- **A WASM component whose interface does not match `anvil:step` now says
  why** (#24): loading it used to fail with wasmtime's bare `failed to
  convert function to given type`, naming neither the signature the bridge
  expected nor the one the component exports. The bridge (`anvil-exec-wasm`)
  now inspects the component's type and prints both `run`/`describe` lines
  side by side.

## [0.4.0] — 2026-09-03

### Breaking

- **The `path` of a `wasm` executor is the executor's binary**
  ([ADR-0027](docs/adr/0027-a-sequence-names-the-executor-not-the-module.md)),
  not a `.wasm` and not the folder of modules:

  ```yaml
  executors:
    - name: instrumentos
      type: wasm
      path: departamento/anvil-exec-wasm     # was: the path to the modules
  ```

  Where its modules are is the executor's own business: it serves the `.wasm`
  it finds **next to its own binary**. So a sequence stops carrying anybody's
  build tree inside it, and **a department becomes a copyable folder** — the
  binary and its modules — that you move to another machine and reach by
  swapping `type: wasm` for `type: grpc`.

  Pointing `path` at a `.wasm` is stopped with a message that explains it,
  instead of failing with "Exec format error".

  Updated in this repo: `ejemplos/demo_wasm.yaml` and
  `ejemplos/demo_departamento.yaml`. `make build`/`make release` assemble the
  example department in `ejemplos/departamento/dist/`.

  **Amends [ADR-0023](docs/adr/0023-the-bridge-ships-as-a-file-next-to-anvil.md):**
  `anvil` no longer looks `anvil-exec-wasm` up next to itself. It still ships
  there, but from there it gets copied into whatever folder is to be a
  department. And a consequence worth knowing: **a sequence can now make Anvil
  launch any binary** — the same deal TestStand gives a DLL.

- **The WASM bridge is now `anvil-exec-wasm`** (was `anvil-puente-wasm`).
  `anvil` looks for it next to itself by that name, so an installation that
  still carries the old file gets the error naming the path and the new name.
  Rename the file, or take the pair from the release again. Nothing else
  changes: same CLI (`--wasm <path> [--port <n>] [--bind <ip>]`), same
  contract, same components — a `.wasm` built for the old bridge runs
  untouched.

  The name is now a **family**: every executor a user launches is
  `anvil-exec-<language>`, with the hole reserved for the ones to come
  (`anvil-exec-labview`, `anvil-exec-native`). The scheme, and the three
  things it deliberately is not, are written down in
  [diseno/executores-lenguaje.md](docs/diseno/executores-lenguaje.md#naming-anvil-exec-language).
  `puente` was the last Spanish word left in a public file name;
  [ADR-0023](docs/adr/0023-the-bridge-ships-as-a-file-next-to-anvil.md)
  §Alcance had left the rename open on purpose.

- **`anvil:step@0.3.0` → `@0.4.0`: every step component must be rebuilt.** The
  WIT travels stuck to the artifact and there is no compatibility shim by
  decision ([ADR-0020 §4d](docs/adr/0020-parametros-del-paso-en-la-peticion.md)),
  so wasmtime refuses to instantiate a component built against 0.3.0 — nobody
  finds out the wrong way. Rebuild with
  `cargo build --target wasm32-wasip2`. `paso.proto` is untouched and the
  contract number stays at 4: a gRPC executor is not affected.

- **A step of the Python executor is named `<module>/<step>`**
  ([ADR-0026](docs/adr/0026-the-python-executor-is-a-department-too.md)). Every
  `.py` under `--steps` is a module, named after its file, and a step is
  addressed with both:

  ```yaml
  main:
    - name: instrument/medir_simulador   # was: medir_simulador
      executor: python
  ```

  The module is derived from the file and never declared: renaming or moving a
  module does not force an edit on the steps inside it. A package takes the
  name of its folder.

  **Sequences calling a Python step by its bare name have to be qualified.**
  Anvil catches them before the unit is touched: `--validate --with-executors`
  says what the executor serves, already qualified.

  In exchange, **two modules can each serve a `medir_voltaje`** — which used to
  be a startup failure. What must be unique is a name within its module. And
  two files with the same name under two different `--steps` make the executor
  refuse to start, naming both.

  Updated in this repo: `ejemplos/demo_ejecutores.yaml`,
  `ejemplos/referencia.yaml` and `docs/qa/referencia/run.sh`.

- **`paso.proto` goes to contract 4, and this is a flag day.** `Value`'s
  `oneof` gains a branch and `ValueType` a value (ADR-0022). The contract echo
  is gated on the **integer**, not on functionality (ADR-0020 §4a), so **every
  step with `inputs:` against an executor still speaking 3 becomes `error`,
  whether it uses references or not**. Third-party executors have to be
  updated; this repo's Python executor and the WASM bridge already speak 4.
  It is accepted because Anvil is pre-v1 and promises no backward
  compatibility — and it is said here in those words, not as a footnote.

### Added

- **A WASM executor serves several modules**
  ([ADR-0025](docs/adr/0025-the-executor-is-a-department-modules-by-logical-name.md)).
  The `.wasm` stops being the executor and becomes a **module** inside it: one
  executor serves several, each under the logical name of its file, and a step
  is called `<module>/<step>`.

  ```yaml
  executors:
    - name: instrumentos
      type: wasm
      path: departamento/anvil-exec-wasm
  main:
    - name: multimetro/medir_voltaje
      executor: instrumentos
    - name: plc/medir_voltaje       # same name, another instrument
      executor: instrumentos
  ```

  Neither the extension nor the path appears in the sequence, so the executor
  can reorganise its folders — or rewrite a module in another language —
  without editing any YAML. Worked example in
  [`ejemplos/demo_departamento.yaml`](ejemplos/demo_departamento.yaml).

  The qualified name travels inside `StepRequest.name`, which for `paso.proto`
  is an opaque string: **no contract change, no engine change and no WIT
  change**.

- **`anvil-exec-wasm --list`**: prints the modules an executor serves, their
  SHA-256 and the signature of every step, and exits without listening. It is
  how to answer "what steps do you serve?" without setting up a bench, and the
  door an editor needed.

- **`anvil-exec-python`**, the Python executor's launcher
  ([`executors/python/`](executors/python/)). Same product, the family's name,
  and it puts the executor's own directory on `sys.path` so nobody has to
  export `PYTHONPATH` to start it:

  ```sh
  ./anvil-exec-python --steps mis_pasos
  ```

  `server.py` is untouched and still runs exactly as before, with the same
  flags — the launcher hands it the command line.

- **Writing a step in Rust is annotating a function** (issue #39,
  [ADR-0024](docs/adr/0024-the-signature-is-the-catalog-in-rust-too.md)). The
  Rust step SDK, [`executors/rust/`](executors/rust/), the sibling of the
  Python one:

  ```rust
  use anvil_step::{step, Ctx, Outcome};

  /// Measures the voltage on a channel.
  #[step(outputs(channel_used: f64))]
  fn measure_voltage(channel: f64, scale: Option<String>) -> Outcome {
      Outcome::measured(read(channel, scale)).output("channel_used", channel)
  }

  anvil_step::export!();
  ```

  ```sh
  cargo build --target wasm32-wasip2
  ```

  Gone from the author's project: the hand-written `run` that looked for its own
  parameters, the copied `wit/` directory, the 507-line generated `bindings.rs`,
  and `cargo install cargo-component`. One dependency and the plain toolchain.

  **The signature is the catalog**, as in Python: names, types and which inputs
  are required come from the function, so a `channell` in the YAML is caught by
  `--validate --with-executors` before the unit is on the bench. A parameter of a
  type a sequence cannot send does not compile.

- **WASM steps are no longer *unchecked***. `anvil:step@0.4.0` adds `describe`,
  the component publishes its catalog and the bridge translates it. Until now
  the bridge answered `describes = false` because there was nothing to publish.

- `make example` builds the reference component, and `make build` and CI now do
  too. Before, nothing built it: it needed `cargo component` and was a manual
  acceptance criterion.

- **The object reference: a fourth type, and it names a slot** (issue #55,
  [ADR-0022](docs/adr/0022-la-referencia-a-objeto-es-un-cuarto-tipo-y-nombra-una-ranura.md)).
  A test sequence needs several steps to work on the same bench state — the
  *rack* of TestStand. That object cannot cross the wire: it carries open
  sockets and vendor driver locks. So it stays in the executor and what travels
  is a **reference** to it.

  The mechanism already worked with the three types — a handle is opaque text.
  What was missing was the type, and the type exists so the engine can
  **refuse**:

  - a reference in an arithmetic operation, a comparison, a limit or a verdict
    is an error, not a `false`;
  - there is no literal form, so one cannot be written into a sequence by hand;
  - a handle passed to a step of **another** executor is rejected **before the
    run starts**, with nothing connected — the check that TestStand cannot even
    pose, because everything there is one process.

  What is new on each side:

  - **`locals:` can declare one**, and it is the only variable with no initial
    value: `rack: { type: reference, executor: bench }`. The executor is part of
    the declaration because it is what makes the cross-executor check decidable
    without following the handle back through `assign`, subsequence `args` and
    the process model.
  - **The engine stamps the executor's name** on every reference a step mints;
    the executor mints the opaque payload. Neither claims anything about the
    other's half.
  - **A reference names a slot, not an object**: mutating the bench does not
    change its identity, so a step that reconfigures it answers the reference it
    was given. This is what keeps retries working — the engine evaluates the
    parameters once and re-sends the same ones on every attempt.
  - **`Catalog` carries a lifetime**, minted by the executor on every start. If
    the process holding the references dies and is born again mid-run, Anvil
    finds out **before invoking** the next step that carries a handle, the step
    does not measure, and it comes out `error` rather than aborting the run —
    which is what lets the `cleanup` still close the rack. An executor that
    publishes no lifetime is said to be unchecked, never assumed fine.
  - **The report keeps it**: the JSON writes a reference as an object
    (`{"type": "reference", "executor", "lifetime", "payload"}`), and the CSV as
    a percent-encoded `ref:…` token, so an opaque payload carrying `;` or `=`
    cannot silently split the cell.
  - **The Python executor is the worked example**: `ctx.objects` keeps the
    slots, `Reference` is a parameter type like any other, and `steps/instrument.py`
    ships `open_bench` / `configure_bench` / `measure_bench` / `close_bench`.
    See `ejemplos/referencia.yaml` and `docs/qa/referencia/run.sh`.

  **Not covered**: a reference reaching a WASM component. `anvil:step` is a
  function with no state between calls, so the component has nowhere to keep
  the object. That is refused explicitly — at load if the executor is declared
  `type: wasm`, and again at the bridge — and never in silence. Giving WASM
  state is a decision with its own ADR.

### Changed

- **`server.py --list` groups by module** and shows the SHA-256 of the file
  serving each one, alongside the signatures. The startup line lists the
  qualified names too.

- **`executores/` is now `executors/`.** It was not a word in either language:
  Spanish spells it *ejecutores* and English *executors*. With the code already
  in English, the hybrid only confused. The Makefile target goes from
  `make test-executores` to `make test-executors`. Nothing about the contents
  changes — not the paths inside the executor, not the contract, not sequences.

- **The WASM executor is a product now: it lives in `executors/wasm`, under
  Apache-2.0, and ships as a file next to `anvil`** (issue #57,
  [ADR-0023](docs/adr/0023-the-bridge-ships-as-a-file-next-to-anvil.md)).
  The bridge `anvil-puente-wasm` — the process that serves a user's `.wasm`
  step component over gRPC — used to live in `packaging/` and went embedded
  inside the `anvil` binary, extracted to a temp file at startup. It was a
  sibling of the Python executor in everything but its placement, and
  placement read as plumbing; it also could not be copied to another
  machine, which is what ADR-0015 promised for the remote (Raspberry Pi)
  case and never shipped.
  What changes: `anvil` no longer carries the bridge inside. The package now
  ships **two files** — `anvil` and `anvil-exec-wasm` — and the bridge can be
  copied elsewhere and launched by hand. An executor older than the binary
  still fails with both contract versions named (ADR-0020 §4b). Sequences that
  declare no `type: wasm` executor never notice the change.

  *(Superseded within this same release by ADR-0027, above: `anvil` no longer
  looks the bridge up beside itself — the sequence names the binary to spawn.
  Shipping it next to `anvil` is still how it arrives.)*

### Fixed

- **A `println!` in a WASM step no longer kills the executor.** The blocking
  wasmtime call ran on the tokio runtime's driver thread, and the WASI bindings
  block on the runtime from inside to serve the component's stdout: the first
  print in a step died with *"Cannot start a runtime from within a runtime"* and
  cut the sequence mid-unit. A `panic!` in a step still cuts the run — the
  instance is gone and the bridge does not reinstantiate it — and that is now
  written down as a known limitation in the quick-start guide.

## [0.3.0] — 2026-08-27

### Añadido

- **Un ejecutor sabe decir qué pasos ofrece y con qué firma** (issue #45,
  [ADR-0021](docs/adr/0021-el-ejecutor-describe-su-catalogo.md)). `paso.proto`
  gana un RPC, `Describe(CatalogRequest) → Catalog`, que devuelve el catálogo
  entero del ejecutor: nombre de cada paso, entradas (nombre, tipo,
  obligatorio, valor por defecto) y salidas. Anvil lo pregunta **una vez por
  endpoint al arrancar**, nunca antes de cada paso, y comprueba contra él:
  - que el paso exista en el ejecutor al que se despacha;
  - que sus `inputs` sean parámetros que el paso admite, y que no falte
    ninguno obligatorio;
  - que un literal sea del tipo declarado;
  - que `assign: result.outputs.<nombre>` lea una salida que el paso devuelve.

  Esa última era **la excepción declarada en ADR-0020 §3** a la regla de
  detección de ADR-0019: un `result.outputs.tensionn` sólo se notaba
  ejecutando, con la unidad ya en el banco. Ahora un hallazgo detiene la
  corrida **antes del primer paso**, con exit 1.
- **`--validate --with-executors`**: comprueba las firmas conectando a los
  ejecutores, sin ejecutar un solo paso. `--validate` a secas sigue sin
  conectar —es su razón de existir en CI sin hardware—, así que esto es un
  opt-in explícito de quien los tiene levantados. Fuera de `--validate` es un
  error de uso: al correr, las firmas se comprueban siempre.
- **Un ejecutor puede no describirse, y se nota.** Un ejecutor de terceros que
  no implemente `Describe` deja sus pasos como *sin comprobar*, con el motivo y
  el recuento (`aviso: 2 step(s) unchecked on 'python': …`). Ni error —cerraría
  la puerta a terceros— ni silencio, que sería el verde falso de ADR-0019. El
  puente WASM es hoy el caso real: `anvil:step` exporta un único `run`, así que
  desde fuera no hay lista de nombres que publicar (issue #39).
- **El ejecutor Python: un paso propio ya no exige editar `server.py`**
  (issue #54). Se escribe una función, se decora con `@step` y se deja el
  fichero donde apunte `--steps PATH` (o `ANVIL_PYTHON_STEPS`, o `./steps`).
  **La firma es el catálogo**: nombres, tipos y obligatoriedad salen de la
  propia función, así que no se escriben dos veces y no pueden divergir. Los
  tres pasos que venían de serie se han mudado a `executors/python/steps/` y
  se descubren como los de cualquiera. Trae además `--list` (ver el catálogo
  sin levantar un banco), `--option clave=valor` (configuración de despliegue,
  vía `ctx.options`) y tests que **no necesitan gRPC** (`make test-executores`,
  renombrado a `make test-executors` justo después de publicar la 0.3.0).

**Añadir `Describe` no sube el número de contrato** (sigue en 3), y el cambio
de `paso.proto` es **aditivo**: ningún tag se mueve. Un ejecutor que no conozca
el RPC funciona exactamente igual que antes — la regla de ADR-0020 §4c es que
sube el contrato lo que pueda alterar un veredicto en silencio, y no describirse
no altera ninguno.

### Arreglado

- **El ejecutor Python estaba roto desde la traducción al inglés** (`579f468`):
  leía `request.nombre` y `request.intento`, campos que el contrato ya no
  tiene, así que toda invocación moría. Reproducido con
  `ejemplos/demo_ejecutores.yaml` antes de tocar nada. Que pasara inadvertido
  es el mejor argumento de ADR-0021: un ejecutor que no se puede interrogar
  tampoco se puede comprobar.
- **`ejemplos/variables.yaml` llamaba a un paso que no existe**
  (`verificar_frecuencia`). Nunca dio guerra porque Main corta en el primer
  fallo y no se llegaba a invocar — hasta que alguien tocase el límite del paso
  anterior. Lo encontró la comprobación de catálogos el día que se escribió,
  que es justamente para lo que está.
- **El ejecutor embebido dejaba colgado al motor ante una ruta gRPC
  desconocida**: la ignoraba sin responder, y `wasi-grpc` v0.1 no tiene
  deadline, así que el cliente esperaba para siempre. Ahora contesta con un
  cuerpo vacío.
- **El binario no ejecutaba ninguna secuencia si el host y el motor no
  compartían cargador** (issue #52). El host pre-escanea el YAML por su cuenta
  para recolectar los `executors:` declarados; cuando ese parseo fallaba por
  esquema, además **deducía** que el motor tampoco iba a poder cargarlo y se
  saltaba el ejecutor de pasos embebido. La deducción sólo vale mientras las
  dos mitades compartan cargador —y el host es un workspace aparte con los
  guests embebidos, así que un build a medias basta para romperlo—. Cuando no
  lo compartían, el motor cargaba la secuencia, no encontraba a nadie
  escuchando, caía al puerto `9100` por defecto (el host tampoco le pasaba
  `--port` en esa rama) y moría con un `connection-refused` que no nombraba ni
  la causa ni el puerto. Cuarenta segundos para no decir nada. Ahora la
  decisión de arrancar el ejecutor sale **sólo de los argumentos**: el host no
  predice el veredicto del motor. Un YAML inválido paga el arranque del
  ejecutor (medido: +6 s en debug, sub-segundo en release), que es lo que
  cuesta no volver a tener este fallo.
- **El error de conexión del motor ya dice contra qué endpoint lo intentó.**
  Sin el puerto en el mensaje, un `connection-refused` no distingue «el
  ejecutor no llegó a tiempo» de «nadie arrancó un ejecutor».

**La interfaz de Anvil pasa a inglés.** El formato de secuencia aspira a ser
un estándar, y ninguno de los que lo son —HTML, ODF, OOXML— se escribió en
otra lengua. El vocabulario de este dominio (*pass*, *fail*, *limit*, *step*,
*setup*, *cleanup*) ya está en inglés en la cabeza de quien viene de TestStand
o de OpenTAP: escribirlo en castellano no ahorraba una traducción, la añadía.

Y el formato ya estaba a medias: `main`, `setup`, `cleanup`, `locals`,
`file_globals`, `disable`, `pause_on_fail` y `statement` conviviendo con
`nombre`, `limite`, `precondicion` y `asigna`.

La traducción completa está en [`GLOSSARY.md`](GLOSSARY.md), y la regla de
qué va en cada idioma, en [`CONTRIBUTING.md`](CONTRIBUTING.md): **lo que ve
quien usa Anvil, en inglés; el código, los comentarios y los ADRs, en
español.**

**Ninguna secuencia escrita hasta hoy carga, y ningún ejecutor o componente
anterior sirve.** Es una rotura limpia y deliberada: no hay capa de
compatibilidad ni alias, porque no existe ningún consumidor externo.
Un fichero viejo no falla con un error opaco — el cargador reconoce los
nombres del schema anterior y responde «¿querías `name`?».

### Cambiado

- `--validate --with-executors` es la única situación en la que el host levanta
  el ejecutor embebido sin ir a ejecutar pasos (excepción explícita al issue
  #22, que sigue valiendo para `--validate` a secas).
- **BREAKING — las claves del YAML.** `nombre`→`name`,
  `reintentos`→`retries`, `limite`→`limit`, `tipo`→`type`,
  `precondicion`→`precondition`, `asigna`→`assign`, `condicion`→`condition`,
  `secuencia`→`sequence`, `ejecutor(es)`→`executor(s)`,
  `subsecuencias`→`subsequences`, `puerto`→`port`, `esperado`→`expected`.
  Valores: `rango`→`range`, `comparacion`→`comparison`,
  `embebido`→`embedded`. El sidecar de `--limits` usa el mismo schema.
- **BREAKING — `parametros` se parte en dos**, y esto no es una traducción
  sino una decisión que la traducción obligó a tomar. Significaba dos cosas
  —by-value en un paso `grpc` (ADR-0020) y by-reference en un `sequence_call`
  (ADR-0010)— y chocaba además con el scope `parameters` de la secuencia.
  Pasan a ser **`inputs`** y **`args`**. Con nombres distintos, copiar un
  bloque de un sitio al otro deja de poder cambiar el significado en
  silencio: da error de campo desconocido.
- **BREAKING — el lenguaje de expresiones.** El scope `resultado`→`result` y
  sus campos: `estado`→`status`, `mensaje`→`message`,
  `valor_medido`→`measured_value`, `salidas`→`outputs`.
- **BREAKING — los estados.** `paso`→`pass`, `fallo`→`fail`,
  `saltado`→`skipped`, `inconcluso`→`inconclusive`. `error` no cambia.
  Afecta a lo que devuelve un ejecutor, al reporte de texto y a los informes.
- **BREAKING — el contrato gRPC sube a 3.** `PeticionPaso`→`StepRequest`,
  `ResultadoPasoProto`→`StepResult`, `Valor`→`Value`, y el servicio
  `EjecutorPasos/Invoca`→`StepExecutor/Invoke` (la ruta pasa a
  `/StepExecutor/Invoke`). No es cosmético y por eso sube el número: un
  ejecutor que hable el 2 leería tags con otro tipo, no sólo con otro
  nombre, y el eco lo rechaza.
- **BREAKING — el WIT pasa a `anvil:step@0.3.0`**, con `run(name, attempt,
  inputs)`. Cambia el nombre del paquete además de la versión, así que hay
  que **recompilar todo componente**. `record step-result` y no `result`
  porque `result` es palabra reservada de WIT.
- **BREAKING — los informes.** JSON: `secuencia`→`sequence`,
  `pasos`→`steps`, `sub_pasos`→`sub_steps`, `pasos_saltados`→`skipped_steps`,
  `pasos_totales`→`total_steps`, `secuencia_usuario`→`user_sequence`, y los
  campos de cada paso. CSV: las trece columnas, **en el mismo orden** — quien
  leyera por índice sigue igual, quien leyera por nombre no.
- **BREAKING — los flags.** `--ejecutor`→`--executor`,
  `--solo-loopback`→`--loopback-only`.

### Sin cambios

Los **mensajes de error y el reporte de texto** siguen en español, y se irán
traduciendo por la regla del *Boy Scout*. Los **ADRs y los informes de beta**
tampoco se tocan: son registro fechado, y reescribir sus ejemplos los haría
mentir sobre lo que se decidió entonces.

**Un paso ya puede recibir parámetros y devolver valores con nombre**
(ADR-0020, issue #46). Hasta ahora un paso sólo recibía su nombre y el número
de intento, así que todo lo que necesitaba para medir iba grabado dentro: un
`4.2` a fuego en `pasos_demo`, una variable de entorno en `pasos_scpi`, un
flag de proceso en el ejecutor Python. Tres formas distintas, y ninguna
llegaba al informe: **dos corridas de la misma secuencia con distinto canal
producían informes idénticos.**

### Añadido

- **`parametros:` en un paso `grpc`.** Un mapa de literales o de expresiones
  `${...}` que evalúa el motor **antes** de llamar (ADR-0009: el paso no ve
  `locals`, se le pasan valores). El tipo es el del escalar YAML —`canal: 2`
  es un número y `canal: "2"` es texto— y es el que viaja por el cable.
  Una expresión que falla deja el paso en `error` y **no llega a invocar al
  ejecutor**: nunca hay valor por defecto, porque medir con un parámetro
  inventado da un número que parece bueno y no lo es.
- **Salidas con nombre.** Un paso puede devolver N valores además de la
  medida, y `asigna` los lee como `resultado.salidas.<nombre>`.
  `valor_medido` no cambia: sigue siendo lo único contra lo que el motor
  evalúa el `limite` (ADR-0008). Sin `inout`: entra por `parametros`, sale
  por `salidas`.
- **Los parámetros enviados y las salidas van al JSON y al CSV**, con su tipo
  en JSON (un número es un número, no una cadena). Es la Regla 3 de ADR-0019
  por la puerta que aquel ADR no miró: no altera el criterio el límite, lo
  altera la condición en la que se midió. En CSV son dos columnas nuevas
  **al final**, como se hizo con `fase`, para no mover las que ya había.
  El reporte de texto no se toca (RNF-08).
- **Número de contrato en el cable, con eco.** El motor manda el contrato que
  habla (2) y el ejecutor devuelve el que ha entendido. Si un paso declara
  `parametros` —o lee `salidas`— y el ejecutor responde un contrato menor, el
  paso es **`error`**, nombrando el endpoint y las dos versiones. Sin esto, un
  ejecutor antiguo ignoraría los parámetros en silencio, **mediría otra cosa y
  diría `paso`**. Un paso que no pide nada nuevo sigue corriendo contra un
  ejecutor de contrato 1 exactamente igual que antes.

### Cambiado

- **BREAKING — el WIT pasa a `anvil:paso@0.2.0` y hay que recompilar los
  componentes.** `run` cambia de firma (`run(nombre, intento, parametros)`) y
  el `record resultado` gana `salidas`. **No hay capa de compatibilidad en el
  puente**: la versión viaja pegada al artefacto y wasmtime falla al
  instanciar si no casa (ADR-0020 §4d, que es la respuesta al issue #39).
  Afecta a `ejemplos/hola-paso` y a cualquier `.wasm` de paso ya compilado.
  Un componente sigue sin saber de gRPC ni de versiones de contrato: **el eco
  lo responde el puente por él** (ADR-0015).
  Por gRPC no se rompe nada: los ejemplos, `pasos_demo`, `pasos_scpi` y el
  ejecutor Python siguen funcionando.
- **`parametros:` deja de estar reservado a `sequence_call`.** Sigue
  significando allí lo mismo (argumentos by-reference, ADR-0010) y significa
  lo nuevo en un paso `grpc` (by-value). Son mutuamente excluyentes por
  `tipo`, y para que copiar un bloque de un sitio al otro no cambie el
  significado en silencio, **un valor como `locals.canal` sin `${}` en un
  paso `grpc` es error de carga**, con el mensaje diciendo la forma correcta.
  En un `statement` o un `pass_fail` sigue sin admitirse.
- **`pasos_demo::medir_voltaje` acepta `canal` y `offset`** y devuelve las
  salidas `canal_usado` y `temperatura`. Sin parámetros mide los 4,2 V de
  siempre, así que `ejemplos/basica.yaml` no cambia.

**`--validate` deja de decir «válida» a secuencias que no lo son.** El manual
promete que el flag «carga la secuencia, valida el schema, resuelve
subsecuencias y detecta ciclos — sin ejecutar nada ni levantar el ejecutor».
Validaba menos de lo que prometía y conectaba más. Las cinco entradas de abajo
son la misma idea: lo que se puede decidir sin hardware se decide al cargar, no
a mitad de la corrida ni en silencio.

**Hay secuencias que hoy cargan y pasarán a fallar al cargar.** Cada una de
ellas es una definición que ya estaba rota: todas morían en ejecución, sólo que
más tarde y con la unidad medio probada. Un caso concreto del propio repo:
`ejemplos/medir_fuentes.yaml` escribe `parameters.canal`, legítimo porque
`ejemplos/subsecuencia.yaml` la invoca — pero corrida **como raíz** ahora se
rechaza al cargar en vez de morir a mitad.

### Cambiado

- **BREAKING — leer una variable no declarada es error de carga** (issue
  anlaco/Anvil-Test#19). Cualquier lectura de `locals.X` / `parameters.X` /
  `file_globals.X` en `precondicion`, en la `condicion` de un `pass_fail`, o en
  el lado derecho de un `statement` o de un `asigna`, se valida contra las
  declaraciones de **su propia** secuencia. Antes `--validate` decía «válida» y
  la corrida moría a mitad con «no existe 'locals.X'». Es lo que el manual ya
  prometía: los tres scopes son estrictos.
  **No se comprueban tipos** (`bool * número` y compañía): eso no es decidible
  sin evaluar y sigue siendo error de ejecución (ADR-0019, Regla 2).
- **BREAKING — escribir donde no se puede es error de carga** (issue #17).
  `file_globals` es de sólo lectura en cualquier secuencia; `parameters` sólo
  es escribible **desde una subsecuencia**, que es el modo documentado de
  devolver un valor al llamador (ADR-0010) — en la secuencia raíz no hay
  llamador al que devolver nada. Las dos se comprobaban ya en runtime, con
  «sólo locals», y ninguna al cargar.
- **BREAKING — `asigna` desde `resultado.valor_medido` en un `sequence_call` es
  error de carga** (issue anlaco/Anvil-Test#20). Una subsecuencia no mide:
  agrega el veredicto de sus pasos. Ese campo valía siempre `nothing` y borraba
  el destino sin avisar — el mismo fallo silencioso que ADR-0019 arregló para
  los campos inexistentes. `resultado.estado` y `resultado.mensaje` siguen
  siendo válidos.
- **BREAKING — una subsecuencia que declara `ejecutores:` es error de carga**
  (issue anlaco/Anvil-Test#21). Anvil nunca leyó esa sección fuera de la raíz:
  la descartaba en silencio, también cuando contradecía a la de la raíz — el
  caso peor pasaba `--validate` en verde y mandaba los pasos a un ejecutor que
  no era el que su autor había escrito. La tabla se declara una sola vez, en la
  secuencia raíz (con `--process-model`, también en el process model). Aplica a
  subsecuencias externas e inline, y el mensaje de «ejecutor no declarado» dice
  ahora dónde declararlo.

### Arreglado

- **`--validate` ya no levanta los puentes `.wasm`** (issue
  anlaco/Anvil-Test#22). Validar una secuencia con un ejecutor `tipo: wasm`
  spawneaba el proceso `anvil-puente-wasm`, que abría un puerto de loopback e
  imprimía dos líneas por delante del veredicto — para no ejecutar nada, y en
  CI, que es donde el flag existe. El guard que ya evitaba arrancar el ejecutor
  embebido bajo `--validate`, `-h` y `-V` cubre ahora también los puentes. Que
  el `.wasm` declarado **exista** se sigue comprobando: es una comprobación de
  fichero y no requiere instanciar nada.

## [0.2.0] — 2026-08-15

**Anvil deja de dar verde donde no puede juzgar.** Toda la versión sale del
[ADR-0019](docs/adr/0019-que-hace-anvil-cuando-no-puede-juzgar.md), y las tres
entradas de abajo son la misma idea aplicada en tres sitios: un secuenciador de
test existe para emitir un veredicto auditable, así que cuando no puede juzgar
tiene que decirlo, no aprobar.

Es la versión contra la que se corren las rondas de betatesting a partir de
hoy: hasta ahora iban contra un binario del 11/08, anterior a estos arreglos.

### Cambiado

- **BREAKING — el veredicto de una secuencia se agrega por severidad, y hay un
  quinto estado, `inconcluso`** (ADR-0019 Regla 1, issue #31). La escala es
  `paso < inconcluso < fallo < error`; `saltado` sigue siendo neutral y fuera
  de ella. La secuencia agrega **al más severo de sus pasos**, en vez de a
  «`paso` si nadie falló».

  Esto cambia la **superficie pública** en dos sitios, y por eso se anota como
  incompatible:

  - **El vocabulario de estados**: quien consuma el JSON o el CSV tiene que
    contar con `inconcluso` en el estado de la secuencia. No aparece nunca como
    estado de un paso: lo produce el motor al agregar, y sólo él — un ejecutor
    no puede devolverlo ni una secuencia escribirlo.
  - **El reporte textual** (RNF-08), como extensión aditiva igual que el
    `saltado` de M4: la línea de cabecera puede decir ahora
    `=== secuencia: inconcluso ===`. Las líneas de paso no cambian, y una
    corrida sin el caso nuevo produce exactamente los mismos bytes de siempre.

  **Hay secuencias que hoy salen en `paso` y pasarán a `inconcluso`, con exit 1
  en vez de 0.** Es el objetivo, no un efecto colateral: cada una de ellas es
  una unidad que se aprobó sin comprobar. Concretamente, las que declaran al
  menos un paso `tipo: pass_fail` en `main` y no evalúan ninguno —porque se
  saltó por precondición, por `disable`, o porque el Main no llegó a él—. Una
  secuencia cuyo criterio son los `limite` de sus pasos **no cambia de
  comportamiento**: ahí el veredicto sí se evaluó, paso a paso.

  El código de salida sigue siendo binario (0 = `paso`, 1 = todo lo demás): el
  std de `wasm32-wasip2` aplana cualquier `exit(n≠0)` a `I32Exit(1)` al cruzar
  `wasi:cli/run`. La distinción vive en el estado y en el informe.

- **BREAKING — `fallo` es del DUT; `error` es de Anvil** (ADR-0019 Regla 2,
  issues #28 y #27). Lo que antes salía `paso` o `fallo` porque Anvil no supo
  interpretar algo, ahora sale `error`. Son tres cambios de comportamiento:

  - **El vocabulario de estados de un ejecutor pasa a ser cerrado**: `paso`,
    `fallo`, `error` y `saltado`, y nada más. Cualquier otra cadena —`"Paso"`,
    `"PASS"`, y también `"inconcluso"`, que sólo produce el motor al agregar—
    convierte el paso en `error`, con un mensaje que nombra el valor recibido y
    enumera los cuatro válidos. Antes esto era un `fallo` mudo (#28); desde la
    Regla 1 y hasta aquí fue un `paso` mudo, que era peor. Un ejecutor de
    terceros que escribiera mal el estado dejaba pasar unidades sin medir.

    Es **extensión aditiva de RNF-08**, con el precedente del `saltado` de M4:
    el formato de línea no cambia, sólo qué estado aparece en él. Los tests que
    congelan el reporte siguen pasando sin tocarse.

  - **`asigna` no escribe si el paso dio `error`**: sin resultado no hay nada
    que volcar, y lo que hacía antes era machacar con un `nothing` una variable
    con valor bueno que el `cleanup` iba a leer para decidir (#27). Si el paso
    dio `fallo`, la `asigna` sí corre: hay medida, y es justo la que interesa.

  - **Un campo inexistente de `resultado` deja de valer `nothing`**: los campos
    son tres y cerrados (`estado`, `mensaje`, `valor_medido`), así que
    `resultado.valor_meddio` es un typo, no un dato ausente. Al ser comprobable
    sin ejecutar, **lo rechaza el cargador** —y por tanto `--validate`—, no la
    unidad en el banco; en ejecución, la `asigna` falla y el paso queda en
    `error`.

- **`--validate` rechaza `asigna` sobre un paso `statement`** (ADR-0019, regla
  de detección, issue #27). Un `statement` no produce `resultado.*`, así que su
  `asigna` era un no-op silencioso que el validador aprobaba. El caso hermano
  (`pass_fail` con `asigna`) ya se rechazaba desde ADR-0018; ahora los dos dan
  el mismo diagnóstico. **Hay secuencias que hoy cargan y dejarán de cargar**:
  la `asigna` que se les quita nunca hizo nada.

## [0.1.0] — 2026-08-10

Primer release. Cierra el MVP completo (M0 → M5-ext.2) y los hallazgos de la
primera campaña de betatesting externa.

### Añadido

- **Motor de ejecución** genérico dirigido por datos (ADR-0005): fases
  Setup → Main → Cleanup con corte al primer fallo y Cleanup garantizado,
  reintentos por paso, estados `paso`/`fallo`/`error`/`saltado` y agregado
  `error > fallo > paso`.
- **La secuencia es un dato**: schema YAML con validación fail-fast al cargar
  (RF-20). El motor no abre ficheros; el cargador resuelve paths y valida.
- **Pasos por gRPC, por nombre** (`paso.proto`): el motor nunca llama a un
  paso directamente, lo que aísla los pasos y deja escribirlos en cualquier
  lenguaje.
- **ResultSinks desacoplados** con lifecycle: consola (formato congelado,
  RNF-08), JSON y CSV, con reintento de escritura (RF-21/22/23). SQLite
  aplazado (ADR-0007).
- **Step types**: `pass_fail` por expresión (ADR-0018), *limit test* con el
  límite como dato evaluado por el motor (ADR-0008), `action`, `statement`
  local y `sequence_call`.
- **Límites como datos**, embebidos en el YAML o inyectados por un sidecar
  `--limits` por nombre de paso (RF-29/30).
- **Variables y expresiones**: scopes Locals / Parameters / FileGlobals,
  motor de expresiones con sintaxis Julia (`crates/expr`, sin dependencias),
  precondiciones por paso, `disable` y `pause_on_fail` (RF-31/33/34/35).
- **Subsecuencias** (`sequence_call`) inline o en archivo aparte, con
  `parameters` de entrada/salida by-reference, detección de ciclos al cargar
  y resultados anidados (ADR-0010).
- **Process model Sequential** como secuencia envoltorio, sin tocar el motor
  (ADR-0016): `--process-model`, con plug-ins de identificación y
  notificación como pasos gRPC.
- **Adapter SCPI/TCP** para instrumentos reales (`pasos_scpi`, ADR-0017).
- **Binario único** que hospeda wasmtime y los guests WASM en sandbox
  (ADR-0011): `./anvil secuencia.yaml`, sin instalar nada.
- **Routing multi-endpoint** (`ejecutores:`, `--ejecutor`, `--solo-loopback`)
  con relajación acotada del loopback (ADR-0013).
- **Cargador de `.wasm` por path**: un componente con interfaz WIT
  `anvil:paso` se carga como ejecutor; el host levanta un puente que traduce
  gRPC ↔ función (ADR-0014/0015). El `.wasm` del usuario es una función pura,
  sin dependencias de Anvil.
- **CLI headless**: `--json`, `--csv`, `--validate`, `--port`, `--quiet`,
  `--help`/`-h`, `--version`/`-V`.

### Añadido — tras la beta

- La **fase** (`setup`/`main`/`cleanup`) de cada paso en el JSON y como
  última columna del CSV (#8). Un fallo de Setup, uno de Main y uno de
  Cleanup piden respuestas operativas distintas, y eran indistinguibles.
- La **secuencia del operador** como campo propio del JSON bajo
  `--process-model` (`secuencia_usuario`, #9): antes el resultado archivado
  no registraba qué test se corrió.
- El **recuento de pasos saltados** en el resultado (`pasos_saltados` /
  `pasos_totales` en JSON, línea de cierre en consola) (#13). `saltado` sigue
  siendo neutral en el agregado, pero ahora un verde dice cuánto no corrió.
- **Puerto efímero** por proceso para el ejecutor embebido (#15): varios
  `anvil` pueden correr en paralelo. Con `--port`, el puerto fija tanto el
  ejecutor como el motor.

### Corregido — tras la beta

- `resultado.*` fuera del `asigna` de su paso es **error de carga** (#12).
  Antes valía `nothing` en silencio: una precondición que lo usara era un
  `false` constante, el paso se saltaba y **la secuencia terminaba en verde**.
- El sidecar `--limits` llega a la secuencia del operador bajo
  `--process-model` (#2), y avisa cuando no afecta a ningún paso (#6).
- El cargador rechaza `asigna`/`statement` sobre un destino no declarado, y
  `asigna` no puede nombrar un `parameter` (#4).
- La primera columna del CSV lleva el nombre de la secuencia (#3).
- Un `path` absoluto de ejecutor `wasm` explica el sandbox del cargador en
  vez de afirmar que el fichero no existe (#5).
- Diagnósticos que apuntan al campo correcto (#10): sidecar envuelto, campo
  desconocido con ubicación y sugerencia, `.wasm` que es módulo core, y un
  YAML inválido que se reportaba dos veces.
- `--port` fijaba sólo el puerto del motor mientras el host bindeaba 9100,
  así que no servía para lo que la documentación decía (#15).

### Documentación

- `SECURITY.md` y `CODE_OF_CONDUCT.md` publican un canal privado real:
  *private vulnerability reporting* de GitHub con correo de respaldo, y
  correo para conducta (#11). Antes ordenaban no abrir un issue público y
  remitían a un contacto que no existía.
- `docs/diseno/variables-y-alcances.md` documenta el alcance de `resultado.*`
  (#14), que sólo estaba en el plano de implementación.
- Informe de la primera beta y suite de regresión ejecutable en
  `docs/qa/` — 17 casos que afirman el comportamiento correcto.

### Conocido

- **`--strict`** para tratar un salto inesperado como fallo sigue pendiente
  (#13): exige decidir qué cuenta como inesperado.
- **Sin paralelismo** dentro de un proceso (Parallel/Batch es post-v1); la
  vía soportada es lanzar varios `anvil`.
- **Sin UI**: headless por CLI.
- *Private vulnerability reporting* no puede activarse mientras el
  repositorio sea privado; hasta entonces vale el correo de `SECURITY.md`.

[0.4.0]: https://github.com/anlaco/anvil/releases/tag/v0.4.0
[0.3.0]: https://github.com/anlaco/anvil/releases/tag/v0.3.0
[0.2.0]: https://github.com/anlaco/anvil/releases/tag/v0.2.0
[0.1.0]: https://github.com/anlaco/anvil/releases/tag/v0.1.0
