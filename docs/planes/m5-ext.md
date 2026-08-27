# Plan: M5-ext — Routing multi-endpoint y cargador de `.wasm` host-side

> **Alcance acordado (2026-08-03):** M5-ext.1 (routing `grpc` + relajación
> acotada del loopback + demo Python en loopback) **implementado**.
> M5-ext.2 (cargador `.wasm` por path host-side) **implementado** (ADR-0014),
> independiente del origen del `.wasm` (C a mano, Rust, Zig, un editor
> visual, un tercero) — Anvil es agnóstico al generador. LID se aplaza a
> M5-ext.3. `paso.proto` no cambia (RNF-05).

## Decisiones de diseño (acordadas)

1. **El cargador de `.wasm` por path lo hace el host, no el ejecutor
   embebido** (ADR-0013, ADR-0014, ADR-0015). El ejecutor embebido es él
   mismo un guest WASM: no puede instanciar wasmtime dentro de sí mismo
   (wasmtime es una lib nativa del host). El host lee los
   `TipoEjecutor::Wasm { path }`, spawnea un **puente** por path
   (`anvil-puente-wasm`, embebido en `anvil`) y los expone como endpoints
   gRPC en loopback (puerto efímero; el motor los ve como overrides
   `--ejecutor` — nunca como `Wasm`).
2. **El `.wasm` del usuario es una función, no un servidor gRPC**
   (ADR-0015): componente WASM con interfaz WIT `anvil:paso` (`run`). Se
   compila con `cargo component` + `wit-bindgen` (público). El puente
   (nativo: wasmtime + tonic + wit-bindgen) traduce gRPC↔función. Sin
   `wasi-grpc`, sin `modelo`, sin `ANVIL_PORT` para el usuario. El motor
   nunca ejecuta `Wasm`; correrlo sin host da `Error::EjecutorWasmSinHost`.
3. **Routing nombre→endpoint**: `ejecutores:` en el YAML (embebido/wasm/grpc),
   `ejecutor:` por paso (sólo `grpc`), tabla de conexiones en el motor
   (`Motor::desde_programa`), override CLI `--ejecutor nombre=host:puerto`
   (patrón `--limits`). Sin `ejecutores:` → todo al embebido (compat M4b).
4. **Relajación acotada del loopback** (ADR-0011): IPs no-loopback sólo si se
   declaran en `ejecutores:` (un `grpc` con `host` no-loopback). El host
   recolecta esas IPs del YAML y las permite en el `socket_addr_check` del
   sandbox del motor; el ejecutor embebido sigue loopback-only. Flag
   `--solo-loopback` en el host (CI/paranoia).
5. **WASM es la tesis, Python el añadido.** El cargador `.wasm` por path es
   la pieza central de Anvil (WASM es el lenguaje de serie; el modelo `.vi`
   de TestStand), no un extra. El ejecutor Python
   (`executors/python/`) se mantiene como demo del routing multi-endpoint,
   sin Docker.
6. **Anvil es agnóstico al generador del `.wasm`.** Anvil expone un contrato
   (`paso.proto` por gRPC en loopback) y un mecanismo de carga (`.wasm` por
   path). Lo que hay detrás es opaco. El roadmap avanza por los requisitos
   de Anvil, no por los de un producto externo.
7. **LID aplazado a M5-ext.3** (primero moderno, después legacy). La demo
   LID con Docker del commit `b8371e2` se **revirtió** (se había adelantado
   sin el routing que la justificaba). La tecnología de aislamiento se
   decide al construir el primer LID real.

## Requisitos cubiertos

- **RF-36.1** — ejecutor gRPC remoto; despacho nombre→endpoint. ✅ M5-ext.1.
- **RF-36.2** — paso WASM propio cargado por path en runtime, `Store` propio.
  ✅ M5-ext.2 (agnóstico al origen del `.wasm`).
- **RF-36.3** — routing en YAML + override `--ejecutor`. ✅ M5-ext.1.
- **RF-36.4** — LID (SO legacy aislado). → M5-ext.3 (aplazado).

## Arquitectura (M5-ext.1, implementada)

```
anvil-host (bin nativo, ADR-0011)
  ├─ lee el YAML (cargador, M5-ext.1) → IPs no-loopback declaradas
  ├─ Store 1: motor.wasm          (sandbox: loopback + IPs declaradas)
  └─ Store 2: ejecutor_pasos.wasm (sandbox: loopback-only, 127.0.0.1:9100)
motor (dentro de Store 1)
  └─ tabla de conexiones por nombre de ejecutor (Motor::desde_programa)
       ├─ embebido → 127.0.0.1:9100
       └─ <nombre> → Grpc{host,puerto} declarado (loopback o IP declarada)
secuencia.yaml
  ejecutores: [embebido, python{grpc}]
  main: [verificar_led (default), medir_simulador (ejecutor: python), …]
```

Modelo/cargador/motor: `TipoEjecutor` (Embebido/Wasm/Grpc),
`DefinicionEjecutor`, `DefinicionPaso.ejecutor`, `Programa.ejecutores`,
`EjecutorYaml`, `PasoYaml.ejecutor`, `aplicar_override_ejecutores`.
`paso.proto`/`proto.rs` **sin cambios**.

## Piezas (todas completadas)

0. `git revert b8371e2` (demo LID con Docker adelantada).
1. **Modelo**: `TipoEjecutor`, `DefinicionEjecutor`, `DefinicionPaso.ejecutor`,
   `Programa.ejecutores`. Defaults sin rotura de M4b.
2. **Cargador**: `EjecutorYaml` (embebido/wasm/grpc, deny_unknown_fields),
   `PasoYaml.ejecutor`, validaciones fail-fast (path `wasm` existe, campos
   coherentes con `tipo`, nombres únicos, nombre reservado
   `__anvil_embebido__` rechazado, `ejecutor` sólo en pasos `grpc`,
   referencias a ejecutores no declarados → error), `aplicar_override_ejecutores`.
3. **Motor**: `Motor::desde_programa` (tabla de conexiones), `resolver_endpoint`,
   `Error::EjecutorNoDeclarado/NoConectado/WasmSinHost` (M5-ext.2 reemplazó
   `WasmNoImplementado`). `conecta` legacy preservado.
4. **CLI** (`anvil.rs`): flag `--ejecutor`; conecta con `desde_programa`.
5. **Host** (`anvil-host`): `wasi_loopback_con_declaradas` (IPs del YAML),
   flag `--solo-loopback`, deps de `cargador`+`modelo` (nativo, sin wasmtime).
6. **Ejemplo** `ejemplos/demo_ejecutores.yaml` (embebido + Python, sin
   Docker) + smoke end-to-end verificado.
7. **Docs**: ADR-0013 (nuevo, supersede ADR-0012 en cargador/routing),
   ADR-0012 marcado superseded, roadmap (M5-ext.1/2/3/4), requisitos
   (RF-36.x), diseno/executores-lenguaje.md e integracion-instrumentos.md,
   guía de inicio rápido, este plan.

## Verificación (M5-ext.1)

- `cargo test` — modelo (23), cargador (60), motor (35), sinks (37), resto:
  todo verde.
- `cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos` — los
  guests compilan a WASI P2 sin deps nuevas.
- `cargo build --manifest-path packaging/anvil-host/Cargo.toml` — el host
  con `wasi_loopback_con_declaradas` compila.
- **Smoke manual**: simulador TCP + `server.py` (Python, 9101) + ejecutor
  embebido (9100) + `anvil-guest.wasm ejemplos/demo_ejecutores.yaml` →
  tres pasos en dos ejecutores distintos, JSON/CSV correctos.

### Verificación (M5-ext.2)

- `cargo test` — todo verde (motor: `resolver_endpoint_wasm_es_error_sin_host`).
- `cargo build --target wasm32-wasip2 -p motor -p ejecutor_pasos` — guests
  compilan a WASI P2.
- `cargo component build --manifest-path ejemplos/hola-paso/Cargo.toml` — el
  componente demo compila (requiere `cargo component` instalado).
- `cargo build --manifest-path packaging/anvil-puente-wasm/Cargo.toml` — el
  puente compila (wasmtime + tonic + wit-bindgen, workspace aparte).
- `cargo build --manifest-path packaging/anvil-host/Cargo.toml` — el host
  con `instanciar_wasm` (spawn del puente) compila sin warnings.
- **Smoke manual**: `./anvil ejemplos/demo_wasm.yaml --json out.json` → el
  host spawnea el puente (log: `ejecutor 'mi_paso_wasm' cargado (... →
  127.0.0.1:<puerto>)`), el puente carga el componente y llama a su `run`,
  el motor despacha los tres pasos (embebido + componente), límite y
  reintentos evaluados por el motor, JSON correcto, exit 0. Al salir, el
  puente no queda huérfano (EOF en stdin).

---

## M5-ext.2 — Cargador de `.wasm` por path host-side ✅ (hecho, ADR-0014/0015)

**Agnóstico al origen del `.wasm`.** Anvil expone un contrato (WIT
`anvil:paso`) y un mecanismo de carga (path). Lo que hay detrás —C a mano,
Rust, Zig, un editor visual, un tercero— es opaco. El roadmap avanza por
los requisitos de Anvil (el modelo `.vi` de TestStand + la tesis "WASM es
el lenguaje de serie" + el caso de uso 50+ módulos en una secuencia larga),
no por los de un generador externo.

**Qué se implementó (ADR-0014, rework ADR-0015):**
- El `.wasm` del usuario es un **componente WASM que exporta la función
  `run`** (interfaz WIT `anvil:paso`: `run(nombre, intento) -> resultado`).
  Se compila con `cargo component` + `wit-bindgen` (público, crates.io).
  Sin `wasi-grpc`, sin `modelo`, sin `ANVIL_PORT`, sin clonar el repo.
- El **host spawnea el puente** `anvil-puente-wasm` (nuevo binario nativo,
  workspace aparte; embebido en `anvil` y extraído a temp al arrancar) con
  `--wasm <path> --port <efímero>` (reservado con `bind 127.0.0.1:0`). El
  puente (wasmtime component API + wit-bindgen host + tonic gRPC) carga el
  componente en un Store con **sandbox WASI vacío** (función pura) y
  traduce gRPC↔función: por cada `Invoca` llama `run` y responde
  `ResultadoPasoProto` (`paso.proto` no cambia, RNF-05).
- **Deduplicación por path**: dos ejecutores con el mismo `.wasm` → un
  puente (1 Store, N llamadas, patrón RTE de TestStand). Preload al
  arrancar, readiness por polling, EOF-en-stdin → el puente sale solo.
- **El motor nunca ejecuta `Wasm`**: el host compone overrides
  `--ejecutor nombre=127.0.0.1:<puerto>` sintéticos (M5-ext.1, que ya
  convierte `wasm` → `grpc`). `Error::EjecutorWasmNoImplementado`
  desapareció en ADR-0014; correr un `Wasm` sin host da
  `Error::EjecutorWasmSinHost`.
- **Caso remoto (Pi, futuro)**: el mismo puente se distribuye suelto y se
  corre con `--bind 0.0.0.0`; el YAML declara `tipo: grpc`.
- Demo `ejemplos/demo_wasm.yaml` + componente `ejemplos/hola-paso` (el
  "hola mundo"), verificado end-to-end.
- **Post-M5-ext.2**: AOT precompile a `.cwasm`, `StoreLimitsBuilder`, lazy
  loading, modo Debug, pooling/async — si la medición de 50+ Stores lo pide.

> **Patrón soportado desde M5-ext.1** (sin hito propio): un **único `.wasm`
> que despacha por nombre** (un módulo que atiende N nombres internamente)
> es un ejecutor `grpc` más — 1 Store, N llamadas. Anvil no distingue si
> detrás hay un `.wasm` suelto por path (M5-ext.2) o un módulo que fusiona
> varios pasos. Es el análogo del Run-Time Engine de TestStand: si un
> generador produce ese formato, funciona sin nada especial.

## M5-ext.3 — LID (Legacy Isolation Domain) · aplazado

- **LID** (RF-36.4): patrón de despliegue para SO legacy con "puertas
  declaradas"; la tecnología (Docker/VM/Sandboxie…) se define al construir
  el primer LID real. Investigación en
  [investigacion/aislamiento-lid.md](../investigacion/aislamiento-lid.md).
- Introspección de firma para el editor visual (post-MVP, ligado al
  drag-and-drop de módulos — ver `diseno/ui-vs-headless.md`).
- Pooling allocator / wasmtime async / cache AOT persistente: si la medición
  de 50+ Stores lo pide.
- **Process model Sequential** (RF-38): sigue siendo M5-base, fuera de
  M5-ext.

---

## Anexo — Investigación que informó estas decisiones

### TestStand (fuentes: NI docs "Using Debugging Effectively with TestStand",
"Improving TestStand System Performance", "Code Module Development Best
Practices", "How TestStand Interacts with LabVIEW Application Instances")

- **Debug vs Run**: en Run la secuencia corre a velocidad máxima, sin
  actualización por paso; en Debug el motor activa *tracing* (unos ms/step
  de overhead), breakpoints y single-stepping. **Step Into delega en el
  depurador del adapter** (LabVIEW Dev System para `.vi`, depurador de
  C/C++, etc.): TestStand pausa y cede control; no depura el módulo él
  mismo. El LabVIEW Dev System (depurable) es más lento que el LabVIEW RTE
  (no depurable).
- **Editor de módulos**: al añadir un code module, el adapter
  **introspecciona la firma** automáticamente (parámetros, tipos, in/out,
  retorno) y rellena la tabla. Drag-and-drop de un `.vi` → el adapter lo
  carga y rellena la firma.
- **Carga de VIs**: por defecto **preload** al iniciar la secuencia (retardo
  inicial, ejecuciones siguientes sin recarga); `Load dynamically` para
  módulos raros de diagnóstico; `Unload After Step/Sequence Executes` para
  liberar memoria (a costa de recarga). Recomiendan 64-bit + más RAM antes
  que unload agresivo. No hay precompilación monolítica: los `.vi` se cargan
  sueltos por path; el Deployment Utility genera Packed Project Libraries
  (.lvlibp) para acelerar la carga, opcional. Con 50+ VIs: mantenerlos
  cargados (preload) es la estrategia recomendada.

### wasmtime (fuentes: docs.wasmtime.dev — Store, Engine, Config,
StoreLimitsBuilder, InstanceAllocationStrategy, examples pre-compiling /
multithreaded / fast-instantiation / async / debugging)

- **Store**: "objeto de vida corta"; sin GC interno (una instancia no se
  libera hasta droppear el Store). Límite por defecto 10.000 instancias por
  Store (irrelevante para 1 módulo por Store). `StoreLimitsBuilder` acota
  memoria/tablas/instancias.
- **Engine**: uno por proceso, clon barato (handle), compartido por todos
  los Stores. La compilación JIT de cada módulo ocurre una vez y se cachea
  en el Engine.
- **Pooling allocator**: NO reduce el número de Stores vivos; es sobre cómo
  se asignan recursos dentro de un Store. El ejemplo oficial "Fast
  Instantiation" usa 100 threads × 100 Stores con pooling — el patrón
  muchos-Stores está soportado y es el recomendado para "una instancia por
  Store".
- **AOT**: `precompile_component`/`deserialize_file` eliminan la compilación
  en runtime; `.cwasm` se mmap'ea perezosamente (páginas frías no consumen
  RAM). Cache automático de wasmtime como fallback.
- **Async vs sync**: sync = 1 thread OS por Store vivo (50 threads ≈ 25-100
  MiB de stacks); async = Future por Store con stack async de 2 MiB por
  default (~100 MiB para 50) + tokio. Para gRPC bloqueante, async es
  natural; para CPU puro, sync + thread pool + epoch interruption funciona.
- **Debug**: `debug_info(true)` + LLDB/GDB attach = viable hoy (breakpoints
  a nivel de función wasm, inspección best-effort). `guest_debug` (stepping
  por instrucción wasm) es **experimental e incompleto** ("Breakpoints,
  watchpoints, and stepping are not yet supported"). `native_unwind_info`
  puede tener comportamiento cuadrático al cargar/descargar muchos módulos.
- **Component model**: no encaja para "50 pasos que hablan gRPC al host":
  es composición estática WIT entre componentes; `paso.proto` ya da el
  tipado. Core WASM modules bastan.

### Conclusión del anexo

- **50 Stores es un caso de uso real** (como los 50+ VIs de TestStand en una
  secuencia larga: 50+ módulos `.wasm` distintos, cada uno con su Store), no
  un patológico: wasmtime lo soporta (ejemplo oficial de 100 Stores) y las
  mitigaciones (AOT + StoreLimits + lazy + preload) lo hacen cómodo. Se
  medirá RSS/threads al construir M5-ext.2.
- **El modelo mental a replicar es TestStand**: cargar módulos sueltos por
  path, preload con `Load dynamically` para raros, Debug (sueltos,
  depurable) vs. Run (un único `.wasm` que despacha por nombre — soportado
  desde M5-ext.1 como un endpoint `grpc` más, sin hito propio). Nada de
  replicar el process model 1:1.
