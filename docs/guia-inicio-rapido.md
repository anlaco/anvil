# Guía de inicio rápido

Anvil es **un binario**: lo descargas y corres. Por dentro hospeda
`wasmtime` y los dos guests WASM (motor + ejecutor) en sandbox, hablando
gRPC por loopback. No necesitas instalar `wasmtime` ni ningún runtime — va
embebido. Ver [ADR-0011](adr/0011-distribucion-un-binario-hospeda-wasmtime.md)
para el porqué.

## Para el usuario final

Descarga el binario `anvil` y corre:

```sh
./anvil <secuencia.yaml> [--process-model <pm.yaml>] [--json <ruta>] [--csv <ruta>] [--limits <ruta>] [--ejecutor nombre=host:puerto]
```

Ejemplos (con los del repo en `ejemplos/`):

```sh
./anvil ejemplos/subsecuencia.yaml --json ./out.json --csv ./out.csv
./anvil ejemplos/basica.yaml
./anvil ejemplos/limites.yaml
./anvil ejemplos/variables.yaml
./anvil ejemplos/basica.yaml --limits ejemplos/limites.limits.yaml
./anvil ejemplos/demo_ejecutores.yaml      # routing: embebido + Python en loopback
./anvil ejemplos/demo_ejecutores.yaml --ejecutor python=127.0.0.1:9200
./anvil --process-model ejemplos/process_model_sequential.yaml ejemplos/basica.yaml  # process model (M5)
```

La consola imprime el reporte textual por **stdout** (los diagnósticos van a
stderr, no lo ensucian). `--json`/`--csv` vuelcan a fichero. No hay
dependencias que instalar.

> **Process model (M5, RF-38, ADR-0016):** `--process-model pm.yaml` corre
> la secuencia del operador **envuelta** en un process model: el PM es la
> raíz (identificar UUT → invocar la secuencia del operador → notificar) y
> la secuencia del operador se inyecta como subsecuencia usuario
> (`secuencia_usuario: true` en el PM). La misma secuencia va de I&D a
> fábrica cambiando solo el PM. Ver
> [diseno/proceso-de-test.md](diseno/proceso-de-test.md).

> **Routing de ejecutores (M5-ext.1, ADR-0013):** `ejemplos/demo_ejecutores.yaml`
> demuestra el despacho por nombre→endpoint: `verificar_led` lo atiende el
> ejecutor embebido (de serie) y `medir_simulador`/`conectar_equipo` un
> ejecutor Python en `127.0.0.1:9101` (arranca `simulador_tcp.py` y
> `server.py` de `executores/python/` en otras dos terminales). El flag
> `--ejecutor nombre=host:puerto` re-apunta un ejecutor sin tocar el YAML
> (patrón `--limits`). Sin `ejecutores:` declarado, todo va al embebido.

## Para desarrolladores (build desde source)

### Prerrequisitos

- Rust toolchain con el target `wasm32-wasip2` (`rust-toolchain.toml` lo
  fija).
- Sin `wasmtime` necesario: el host lo embebe como librería. (El CLI de
  `wasmtime` sólo hace falta si quieres correr los guests sueltos para
  depurar — ver abajo.)

### Compilar

El host embebe los dos `.wasm`, así que se construyen primero:

```sh
# 1. Guests WASM (motor + ejecutor) — workspace del core
cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos

# 2. Host nativo (workspace aparte; wasmtime se compila aquí, no en el core)
cargo build --manifest-path packaging/anvil-host/Cargo.toml          # → packaging/anvil-host/target/debug/anvil
cargo build --release --manifest-path packaging/anvil-host/Cargo.toml # → packaging/anvil-host/target/release/anvil
```

> El host vive en `packaging/anvil-host`, **fuera** del workspace del core,
> para que `cargo build` / `cargo test` del core no arrastren wasmtime
> (decisión ADR-0011). Por eso se compila con `--manifest-path` (o `cd
> packaging/anvil-host && cargo build`), no con `-p anvil-host`.

El `build.rs` del host copia los `.wasm` ya compilados (del `target/` del
core) a `OUT_DIR`; si faltan, falla indicando el comando del paso 1.

### Tests (sin red)

```sh
cargo test                 # modelo, cargador, motor, sinks (155)
cargo test -p motor        # sequence call con mock (sin gRPC)
```

### Probar el binario

```sh
./packaging/anvil-host/target/debug/anvil ejemplos/subsecuencia.yaml --json ./out.json --csv ./out.csv
```

Mismo reporte anidado/JSON/CSV que el smoke. Los logs del ejecutor
("paso pedido: …") van a stderr; stdout queda limpio para el reporte.

## Qué mirar

**Por consola** (reporte textual anidado, M4b):

```
=== basica: paso ===
  [paso] preparar: sequence call 'init_comun' → paso
    [paso] preparar_canal: statement ok
  [paso] test_fuentes: sequence call 'ejemplos/medir_fuentes.yaml' → paso
    [paso] ajustar_canal: statement ok
    [paso] medir_voltaje: medido: 4.2 V
    [paso] desconectar_equipo: equipo desconectado
```

**`out.json`**: `sub_pasos` anidados; `medir_voltaje` con `valor_medido: 4.2`
y `limite_min/max`.

**`out.csv`**: filas aplanadas `test_fuentes/medir_voltaje` (cabecera sin
columnas nuevas).

## Variaciones de M4b (subsecuencias)

Edita `ejemplos/subsecuencia.yaml` y vuelve a correr (no hace falta
recompilar: el YAML se lee en tiempo de ejecución):

- **Firma que no encaiza**: añade un parámetro de más al call externo →
  `secuencia inválida: el sequence call 'test_fuentes' no encaja con la
  firma…` (fail-fast al cargar; no se ejecuta nada).
- **Lvalue no declarado**: `parametros: { canal: locals.inventado }` →
  `…usa 'locals.inventado', no declarado en locals de su secuencia`.
- **Ciclo**: `a.yaml` → `./b.yaml`, `b.yaml` → `./a.yaml` → `ciclo de
  subsecuencias: A → B → A`.

El **by-reference** (la hija muta `parameters.canal` y el padre lo recoge)
no se ve en el reporte — los sinks no exponen `locals`. Lo compruebas en el
unitario: `cargo test -p motor sequence_call_by_reference`.

## Uso del CLI

```
anvil <secuencia.yaml> [--process-model <pm.yaml>] [--json <ruta>] [--csv <ruta>] [--limits <ruta>] [--ejecutor nombre=host:puerto]
```

- La secuencia es el primer argumento posicional (obligatorio); los flags
  pueden ir antes o después.
- `--process-model <pm.yaml>` corre la secuencia envuelta en un process
  model (M5, RF-38): el PM es la raíz y la secuencia del operador se
  inyecta como subsecuencia usuario (ADR-0016). Sin él, la secuencia es la
  raíz (compat).
- Consola siempre; `--json`/`--csv` opcionales (fichero).
- `--limits` inyecta un sidecar de límites por nombre de paso (RF-30),
  sobreescribiendo los embebidos (sólo la secuencia raíz hoy).
- `--ejecutor nombre=host:puerto` re-apunta un ejecutor declarado en
  `ejecutores:` a otro endpoint sin tocar el YAML (R&D vs. fábrica, RF-36.3);
  puede repetirse. Si el nombre no está declarado, error al cargar.
- `--help`/`-h` muestra la ayuda; `--version`/`-V` la versión.
- Flag del host: `--solo-loopback` rechaza cualquier `grpc` no-loopback
  declarado (CI/paranoia).
- Los diagnósticos van a **stderr**; stdout queda limpio para el reporte.

## Depuración con wasmtime CLI (avanzado)

Para correr los guests **sueltos** (sin el host), hace falta el CLI de
`wasmtime` y dos terminales:

```sh
cargo build --target wasm32-wasip2 -p ejecutor_pasos -p motor
# Terminal 1 — ejecutor (gRPC en 127.0.0.1:9100)
wasmtime -S cli -S tcp=y -S inherit-network=y \
  target/wasm32-wasip2/debug/ejecutor_pasos.wasm
# Terminal 2 — motor
wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
  target/wasm32-wasip2/debug/anvil-guest.wasm ejemplos/subsecuencia.yaml
```

El ejecutor en este modo **no termina solo** (loop de aceptar); Ctrl-C al
acabar. Es sólo para depurar guests por separado; para uso normal, el
binario `anvil` (host) es lo recomendado.

## Escribir un paso propio en Rust (M5-ext.2, ADR-0015)

El "hola mundo" completo: escribe un paso en Rust, compílalo a `.wasm` y
ejecútalo con Anvil. **Sin clonar el repo, sin `wasi-grpc`, sin `modelo`.**
Referencia oficial: `ejemplos/hola-paso/`.

1. Instala la herramienta de componentes (una vez):
   ```sh
   cargo install cargo-component --locked
   ```
2. Proyecto del paso (`hola/Cargo.toml` con `[lib] crate-type = ["cdylib"]`,
   `hola/wit/anvil-paso.wit`, `hola/src/lib.rs`). El WIT es el contrato:
   ```wit
   package anvil:paso@0.1.0;
   interface paso {
     record resultado { estado: string, mensaje: string, valor-medido: option<f64> }
     run: func(nombre: string, intento: s32) -> resultado;
   }
   world anvil-paso { export paso; }
   ```
3. La implementación es una función (~15 líneas, con `wit-bindgen`):
   ```rust
   #[allow(warnings)]
   mod bindings;
   use bindings::exports::anvil::paso::paso::{Guest, Resultado};
   struct Component;
   impl Guest for Component {
       fn run(nombre: String, intento: i32) -> Resultado {
           Resultado {
               estado: "paso".to_string(),
               mensaje: format!("hola {nombre} (intento {intento})"),
               valor_medido: Some(4.2),
           }
       }
   }
   bindings::export!(Component with_types_in bindings);
   ```
4. Compila a componente:
   `cargo component build` → `target/wasm32-wasip1/debug/hola.wasm`.
5. Decláralo en el YAML (`ejecutores: [{ nombre: hola, tipo: wasm, path:
   ./hola.wasm }]`, paso con `ejecutor: hola`) y ejecuta
   `./anvil secuencia.yaml`. El host spawnea el puente, que carga tu
   componente (sandbox WASI vacío: sin ficheros ni red) y traduce
   gRPC↔función.

Ver [ADR-0015](adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md).

## Solución de problemas

- **`Falta el guest '…'`** al compilar el host → corre primero
  `cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos`.
- **`no se pudo cargar la secuencia`** → el path del YAML no existe o no es
  accesible (el host preopena el directorio actual).
- **`el ejecutor de pasos no empezó a escuchar`** → el guest ejecutor
  falló al arrancar; revisa stderr.

## Siguiente lectura

- [roadmap.md](roadmap.md) — qué hay hecho (M0→M5) y qué queda (LID
  aplazado, post-MVP).
- [diseno/formato-de-secuencia.md](diseno/formato-de-secuencia.md) — el
  schema YAML completo.
- [diseno/proceso-de-test.md](diseno/proceso-de-test.md) — el process model
  Sequential (M5, RF-38).
- [adr/0011-distribucion-un-binario-hospeda-wasmtime.md](adr/0011-distribucion-un-binario-hospeda-wasmtime.md)
  — por qué un binario hospeda wasmtime.
- [adr/0013-cargador-wasm-host-side-y-routing.md](adr/0013-cargador-wasm-host-side-y-routing.md)
  — el routing nombre→endpoint y el cargador `.wasm` host-side.
- [adr/0014-cargador-wasm-host-side-m5-ext2.md](adr/0014-cargador-wasm-host-side-m5-ext2.md)
  — el cargador de `.wasm` por path (M5-ext.2, implementado).
- [adr/0016-process-model-sequential-como-envoltorio.md](adr/0016-process-model-sequential-como-envoltorio.md)
  — el process model Sequential (M5, implementado).
