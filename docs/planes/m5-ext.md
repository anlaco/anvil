# Plan: M5-ext — Routing multi-endpoint y cargador de `.wasm` host-side

> **Alcance acordado (2026-08-03):** M5-ext.1 (routing `grpc` + relajación
> acotada del loopback + demo Python en loopback) **se implementa ya**.
> M5-ext.2 (cargador `.wasm` por path host-side) y M5-ext.3 (modo Run con el
> `.wasm` fusionado de Telekino) quedan **condicionados al formato de salida
> de Telekino** (otro equipo de Anlaco; el formato `.qvi` aún no está
> decidido). LID se aplaza a post-M5-ext. `paso.proto` no cambia (RNF-05).

## Decisiones de diseño (acordadas)

1. **El cargador de `.wasm` por path lo hace el host, no el ejecutor
   embebido** (ADR-0013). El ejecutor embebido es él mismo un guest WASM:
   no puede instanciar wasmtime dentro de sí mismo (wasmtime es una lib
   nativa del host). El host lee los `TipoEjecutor::Wasm { path }`, instancia
   un `Store` por módulo y los expone como endpoints gRPC en loopback.
2. **M5-ext.1 no instancia `.wasm`**: `TipoEjecutor::Wasm` se define en el
   modelo y se valida al cargar (el path debe existir), pero ejecutarlo da
   `Error::EjecutorWasmNoImplementado` ("requiere anvil-host con soporte
   M5-ext.2; usa `grpc` o `embebido`"). Es un placeholder validado, no una
   función.
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
   la pieza central (telekino), no un extra. El ejecutor Python
   (`executores/python/`) se mantiene como demo del routing multi-endpoint,
   sin Docker.
6. **LID aplazado a post-M5-ext** (primero moderno, después legacy). La demo
   LID con Docker del commit `b8371e2` se **revirtió** (se había adelantado
   sin el routing que la justificaba). La tecnología de aislamiento se
   decide al construir el primer LID real.

## Requisitos cubiertos

- **RF-36.1** — ejecutor gRPC remoto; despacho nombre→endpoint. ✅ M5-ext.1.
- **RF-36.2** — paso WASM propio cargado por path en runtime, `Store` propio.
  → M5-ext.2 (condicionado a Telekino).
- **RF-36.3** — routing en YAML + override `--ejecutor`. ✅ M5-ext.1.
- **RF-36.4** — LID (SO legacy aislado). → post-M5-ext.

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
   `Error::EjecutorNoDeclarado/NoConectado/WasmNoImplementado`. `conecta`
   legacy preservado.
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

---

## M5-ext.2 — Cargador de `.wasm` por path host-side (condicionado a Telekino)

**Dependencia:** el formato de salida de Telekino (un `.wasm` por QVI vs. un
único `.wasm` fusionado que despacha por etiqueta). La decisión es de
Telekino, no de Anvil. Hasta entonces, M5-ext.1 ya valida los paths y el
motor ya entiende `TipoEjecutor::Wasm`; esta fase es un incremental del host.

**Qué será:**
- El host instancia un `Store` por `TipoEjecutor::Wasm { path }`, carga el
  `.wasm` (que habla `paso.proto` por gRPC en loopback, como `server.py` o
  `ejecutor_pasos`), le asigna un puerto loopback libre y lo registra en la
  tabla de routing que ve el motor (reusando el override `--ejecutor`
  internamente). Agnóstico al lenguaje: C, Rust, Zig, lo que sea.
- **Rendimiento para el caso Telekino (50+ `.wasm` en una secuencia larga,
  como los 50+ VIs de TestStand)**, verificado en docs de wasmtime:
  - 1 `Engine` compartido + 1 `Store` por módulo (el patrón soportado; el
    ejemplo oficial "Fast Instantiation" usa 100 Stores).
  - **AOT precompile**: `Engine::precompile_component` → `.cwasm` en disco;
    `Component::deserialize_file` en runtime (sin JIT por arranque). Cache
    automático de wasmtime como fallback.
  - **`StoreLimitsBuilder::memory_size`** (p. ej. 1-4 MB por Store) +
    bajar `memory_guard_size`/`memory_reservation_for_growth`: 50 Stores ≈
    50-200 MB en vez de GBs de address space.
  - **Lazy loading**: Store sólo al primer paso que lo use; compartir por
    path (dos pasos con el mismo `.wasm` → un Store).
  - **Preload al abrir la secuencia** (como TestStand): cargar todos los
    `.wasm` al inicio y mantenerlos cacheados hasta cerrar; `Load dynamically`
    para módulos de diagnóstico.
  - `wasmtime async` (epoll, pocos threads para muchos Stores) y pooling
    allocator: **post-M5-ext.2** si la medición RSS/threads lo pide.
- **Modo Debug** (replica TestStand Dev System): `Config::debug_info(true)`
  (DWARF en el JIT) + LLDB/GDB attach al host; breakpoints a nivel de
  función wasm, inspección best-effort. `guest_debug` (stepping por
  instrucción wasm) es **experimental e incompleto** en wasmtime 47/48:
  no depender de ello para producción. Aviso: `native_unwind_info` tiene
  comportamiento cuadrático al cargar/descargar muchos módulos; evaluar
  desactivarlo con 50+ módulos en debug.

## M5-ext.3 — Modo Run con `.wasm` fusionado de Telekino (condicionado a Telekino)

Si Telekino genera un **único `.wasm`** que despacha por etiqueta (modo Run
de producción, sin depuración), Anvil lo consume como un endpoint `grpc`
más (1 Store, N llamadas, despacho por `nombre` dentro del módulo). La
fusión es responsabilidad de Telekino; Anvil sólo carga el resultado. Es
incremental sobre M5-ext.1 (un endpoint más), sin cargador especial.

Junto con M5-ext.2 forma la **arquitectura a la larga** (ADR-0013): Debug
con `.wasm` sueltos por QVI (aislamiento, introspección, depuración) + Run
con el `.wasm` fusionado (rendimiento máximo). Es el análogo exacto de
TestStand: LabVIEW Dev System (depurable, más lento) vs. LabVIEW Run-Time
Engine (no depurable, rápido) — ver anexo.

## Post-M5-ext

- **LID** (RF-36.4): patrón de despliegue para SO legacy con "puertas
  declaradas"; la tecnología (Docker/VM/Sandboxie…) se define al construir
  el primer LID real. Investigación en
  [investigacion/aislamiento-lid.md](../investigacion/aislamiento-lid.md).
- Introspección de firma para el editor visual (post-MVP, ligado al
  drag-and-drop de QVIs — ver `diseno/ui-vs-headless.md`).
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

- **50 Stores es un caso de uso real** (Telekino: 50 QVIs = 50 `.wasm`, como
  50 VIs en TestStand), no un patológico: wasmtime lo soporta (ejemplo
  oficial de 100 Stores) y las mitigaciones (AOT + StoreLimits + lazy +
  preload) lo hacen cómodo. Se medirá RSS/threads al construir M5-ext.2.
- **El modelo mental a replicar es TestStand**: cargar módulos sueltos por
  path, preload con `Load dynamically` para raros, Debug (sueltos, depurable)
  vs. Run (fusionado, rápido). Nada de replicar el process model 1:1.
