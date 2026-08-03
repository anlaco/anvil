# ADR-0013: Cargador `.wasm` host-side y routing nombre→endpoint

- **Estado:** Aceptada
- **Fecha:** 2026-08-03 (M5-ext.1)
- **Reemplaza:** ADR-0012 (en la parte del cargador de `.wasm` por path y en
  la del routing; el resto — ejecutores de lenguaje como módulos Apache-2.0,
  LID como patrón — se mantiene, con LID aplazado a post-M5-ext)
- **Relaciona:** ADR-0001, ADR-0003, ADR-0005, ADR-0008, ADR-0009, ADR-0010,
  ADR-0011, ADR-0012 (superseded),
  [arquitectura.md](../arquitectura.md),
  [contrato-grpc.md](../contrato-grpc.md),
  [diseno/executores-lenguaje.md](../diseno/executores-lenguaje.md),
  [diseno/integracion-instrumentos.md](../diseno/integracion-instrumentos.md),
  [diseno/ui-vs-headless.md](../diseno/ui-vs-headless.md)

## Contexto

ADR-0012 decía que "el ejecutor embebido carga el `.wasm` por path en su
propio `Store`". Dos problemas:

1. **Es técnicamente imposible como está escrito.** El ejecutor embebido es
   él mismo un guest WASM (`ejecutor_pasos.wasm` corriendo dentro de
   wasmtime). wasmtime es una **lib nativa** enlazada al `anvil-host`
   (ADR-0011): **no existe dentro del sandbox WASM**, y WASI P2 no tiene una
   API estándar "un módulo WASM instancia otro módulo WASM". Un guest no
   puede invocar wasmtime dentro de sí mismo.
2. **El motor conecta a un único endpoint fijo** (`127.0.0.1:9100`). El
   routing nombre→endpoint estaba descrito pero sin implementar: no existía
   `ejecutores:` en el YAML, ni `ejecutor:` por paso, ni override por flag.

A la vez, el otro equipo de Anlaco (Telekino) genera `.wasm` desde un editor
visual (formato `.qvi`): una secuencia larga puede usar 50+ módulos `.wasm`
distintos (como TestStand usa 50+ VIs). El **formato de salida de Telekino
no está decidido** (un `.wasm` por QVI vs. un único `.wasm` que despacha por
etiqueta); la decisión es de Telekino, no de Anvil.

## Decisión

### 1. El cargador de `.wasm` por path lo hace el **host**, no el ejecutor embebido

`anvil-host` es quien instancia wasmtime (tiene la lib nativa). El cargador
de `.wasm` por path (RF-36.2, modelo `.vi`) vive **en el host**: lee los
`TipoEjecutor::Wasm { path }` de la secuencia, instancia un `Store` por
módulo y los expone como endpoints gRPC en loopback. El ejecutor embebido
**no cambia**: sigue despachando built-in (`pasos_demo`) en `127.0.0.1:9100`.

### 2. Routing nombre→endpoint (M5-ext.1, implementado)

- El YAML gana `ejecutores:` (a nivel de secuencia raíz) y `ejecutor:` por
  paso (sólo para pasos `grpc`).
- `TipoEjecutor` tiene tres variantes: `Embebido` (default), `Wasm { path }`
  y `Grpc { host, puerto }`.
- El **motor** despacha cada paso al endpoint de su `ejecutor:` (o al
  embebido si no declara). Tabla de conexiones por nombre en `Motor`
  (`desde_programa`).
- Override por CLI `--ejecutor nombre=host:puerto` (mismo patrón que
  `--limits`).
- **En M5-ext.1 un `Wasm` se valida al cargar (el path debe existir) pero no
  se instancia**: ejecutarlo da `Error::EjecutorWasmNoImplementado` con
  mensaje claro ("requiere anvil-host con soporte M5-ext.2"). La
  instanciación real queda para M5-ext.2, condicionada al formato de
  Telekino.
- `paso.proto` no cambia (RNF-05). El motor no sabe qué hay detrás de cada
  endpoint: embebido, `.wasm` cargado por el host (futuro), o Python en otra
  máquina.

### 3. Relajación acotada del loopback (ADR-0011, implementada)

IPs no-loopback sólo si se **declaran** en `ejecutores:` (un `Grpc` con
`host` no-loopback). Sin declaración, loopback-only (como antes). El
`anvil-host` recolecta esas IPs del YAML y las permite en el
`socket_addr_check` del sandbox del motor; el ejecutor embebido sigue
loopback-only. Flag `--solo-loopback` en el host para CI/paranoia: rechaza
cualquier `grpc` no-loopback al arrancar.

### 4. LID aplazado a post-M5-ext

El patrón LID (ADR-0012 punto 7) se mantiene como **patrón de despliegue**,
pero su implementación (la tecnología de aislamiento: Docker, VM, …) queda
**aplazada**: primero moderno (todo en loopback), después legacy. La demo
LID con Docker del commit `b8371e2` se revirtió (se había adelantado sin el
routing que la justificaba).

### 5. Arquitectura a la larga (contexto, no implementada)

Como Telekino no ha decidido su formato, Anvil deja la puerta abierta a
**tres fases** (ver `docs/planes/m5-ext.md`):

- **M5-ext.1 (este ADR)**: routing `grpc` + relajación loopback. Hecho.
- **M5-ext.2 (cuando Telekino cierre formato)**: cargador `.wasm` host-side
  (un `.wasm` por QVI = un `Store`). AOT precompile a `.cwasm` +
  `StoreLimitsBuilder` + lazy loading + preload (como TestStand). Modo Debug
  con `Config::debug_info(true)` + LLDB attach.
- **M5-ext.3 (modo Run)**: soporte para el `.wasm` fusionado de Telekino
  (un único `.wasm` que despacha por etiqueta): Anvil lo consume como un
  endpoint `grpc` más. La fusión es responsabilidad de Telekino.

## Por qué esta forma

- **Lo único técnicamente posible**: el host es el único que tiene wasmtime;
  moverle la carga del `.wasm` no es una opción, es la única forma de que
  exista.
- **Agnóstico al lenguaje y al formato**: el contrato es `paso.proto`; lo
  que hay detrás (C, Rust, Zig, Python, `.qvi` de Telekino) es opaco al
  motor.
- **No bloquea a Telekino**: M5-ext.1 funciona sin depender de su formato;
  el cargador espera a que Telekino decida (un `.wasm` por QVI vs. fusionado).
- **Seguridad conservada**: sin declaración, nada sale de loopback; el
  sandbox WASM y el aislamiento motor↔ejecutor se mantienen.
- **TestStand como referencia**: el "modelo `.vi`" se replica (cargar por
  path en runtime, preload, debug vs. run), sin replicar el process model
  1:1.

## Recortes y compromisos

- En M5-ext.1, un `TipoEjecutor::Wasm` no se instancia (error claro al
  ejecutar). Es un placeholder validado, no una función.
- LID: patrón documentado, tecnología a definir al construir el primer LID
  real (post-M5-ext).
- `anvil-host` ahora depende de `cargador`+`modelo` (compilados a nativo,
  sin wasmtime) para re-parsear el YAML. El workspace standalone del host se
  mantiene; el core (`cargo build`/`cargo test`) no arrastra wasmtime.

## Consecuencias

- ADR-0012 queda **superseded** por este ADR en las partes del cargador y
  del routing; el resto (ejecutores de lenguaje como módulos Apache-2.0,
  `paso.proto` reusado) se mantiene.
- `Motor::conecta` (un único endpoint) se conserva como API legacy; el flujo
  real es `Motor::desde_programa` (tabla de conexiones).
- El ejecutor Python (`executores/python/`) queda operativo como demo del
  routing multi-endpoint en loopback, sin Docker.
- El cargador `.wasm` host-side (M5-ext.2) es un incremental del host: el
  modelo, el cargador y el motor ya entienden `TipoEjecutor::Wasm`.
