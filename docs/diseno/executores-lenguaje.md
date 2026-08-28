# Diseño: Executores de lenguaje y cargador de `.wasm`

> **Prioridad:** MVP extendido. El ejecutor WASM embebido ya existe; el
> routing nombre→endpoint está **implementado en M5-ext.1** (ADR-0013); el
> cargador de `.wasm` por path está **implementado en M5-ext.2** (ADR-0014,
> agnóstico al origen del `.wasm`); LID es un patrón de despliegue
> **aplazado a M5-ext.3**.

Cómo Anvil llama a pasos en **cualquier lenguaje** y a **módulos WASM
propios** sin recompilar. Trazable a [ADR-0015](../adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md),
[ADR-0014](../adr/0014-cargador-wasm-host-side-m5-ext2.md) (superseded en el
contrato del `.wasm`), [ADR-0013](../adr/0013-cargador-wasm-host-side-y-routing.md),
[ADR-0012](../adr/0012-executores-de-lenguaje-como-modulos.md) (superseded
en el cargador y el routing),
[ADR-0003](../adr/0003-pasos-por-grpc-por-nombre.md) y
[ADR-0011](../adr/0011-distribucion-un-binario-hospeda-wasmtime.md).

## El modelo completo

```
                    ┌──────────────────────────────────────────────┐
                    │  anvil-host (bin nativo, ADR-0011)           │
                    │  ┌────────────────┐    ┌──────────────────┐  │
Motor (WASM) ─gRPC─▶│  │ ejecutor.wasm  │◀──▶│  módulos .wasm   │  │
 nombre→endpoint    │  │  (embebido)    │    │  cargados por     │  │
                    │  │  · pasos_demo  │    │  path (modelo .vi)│  │
                    │  │  · built-in    │    │  · Store propio   │  │
                    │  └────────────────┘    └──────────────────┘  │
                    └───────────┬──────────────────────────────────┘
                                │ gRPC (mismo contrato)
                    ┌───────────▼──────────────────────────────┐
                    │  ejecutores/  (módulos Apache-2.0)       │
                    │  python/  ·  labview/ (futuro)  ·  ...    │
                    │  └─ (opcional) en LID: SO legacy (Win7)   │
                    │     con puertas declaradas                │
                    └───────────┬──────────────────────────────┘
                                │ TCP/SCPI/etc.
                                ▼
                        Instrumento / simulador
```

- El motor despacha por **nombre→endpoint**: no sabe ni le importa si el
  paso lo atiende el ejecutor embebido, un `.wasm` cargado, o un ejecutor
  Python en otra máquina.
- Todos hablan el **mismo `paso.proto`**. El contrato no cambia (RNF-05).

## El ejecutor WASM embebido (de serie)

- **Zero-install**: va dentro de `anvil-host` (ADR-0011). WASM/Rust es el
  **lenguaje de serie** de un ejecutor de pruebas.
- Atiende los pasos **built-in**: `pasos_demo` compilado dentro (pass/fail,
  limit test, action, conectar/medir/desconectar simulados). Siempre
  disponibles, en `127.0.0.1:9100`.
- **No** carga `.wasm` por path: un guest WASM no puede instanciar wasmtime
  dentro de sí mismo (ADR-0013). Eso lo hace el **host** (ver abajo).

### Routing nombre→endpoint (M5-ext.1, implementado)

El YAML declara `ejecutores:` y cada paso `grpc` puede declarar `ejecutor:`.
El motor despacha por **nombre→endpoint** (tabla de conexiones en
`Motor::desde_programa`); sin declaración, todo va al embebido (compat M4b).

```yaml
executors:
  - name: embebido        # el ejecutor WASM de serie (127.0.0.1:9100)
    type: embedded
  - name: python          # ejecutor de lenguaje aparte
    type: grpc
    host: 127.0.0.1         # o 192.168.x.y (LID futuro) — solo si se declara
    port: 9101
main:
  - name: verificar_led   # embebido (default)
  - name: medir_simulador
    executor: python
```

Override por CLI: `--executor python=192.168.1.50:9101` (patrón `--limits`).
IPs no-loopback solo si se declaran (relajación acotada del loopback,
ADR-0011); flag `--loopback-only` en el host para rechazarlas.

### Cargador de `.wasm` por path (modelo `.vi`, M5-ext.2, implementado)

Como en TestStand con un `.vi`: tú compilas el módulo, lo guardas en un
archivo, y la secuencia lo referencia por path. **No se recompila nada.**

```yaml
executors:
  - name: mi_paso_wasm      # clave libre para la secuencia
    type: wasm                # componente cargado por el HOST (ADR-0015)
    path: ./pasos/mi_paso.wasm  # relativo al YAML
```

- **El `.wasm` del usuario es un componente WASM que exporta una función
  `run`** (interfaz WIT `anvil:paso`, ADR-0015). No es un servidor gRPC:
  no sabe de gRPC ni de protobuf ni de Anvil. El autor del paso escribe una
  función Rust de ~15 líneas con `wit-bindgen` (público, crates.io) y la
  compila con `cargo component` — sin `wasi-grpc`, sin `modelo`, sin clonar
  el repo.
- **El host spawnea el puente `anvil-puente-wasm`** (embebido en el binario
  `anvil`, extraído a temp) con `--wasm <path> --port <efímero>`. El puente
  (nativo: wasmtime + tonic + wit-bindgen) carga el componente en un Store
  con sandbox WASI **vacío** (sin ficheros ni red: el componente es una
  función pura — aislamiento real) y traduce gRPC↔función: por cada
  `Invoca` del motor llama a `run(nombre, intento)` y devuelve el resultado
  como `ResultadoPasoProto`. `paso.proto` no cambia (RNF-05); la
  traducción vive dentro del ejecutor.
- **Un puente por path** (deduplicado: dos ejecutores con el mismo `.wasm`
  → un puente). Preload al arrancar, readiness por polling, puerto efímero
  (`bind 127.0.0.1:0`).
- **El motor nunca ejecuta `Wasm`** (ADR-0014/0015): el host compone un
  override `--executor nombre=127.0.0.1:<puerto>` sintético (M5-ext.1, que
  ya convierte `wasm` → `grpc`), así el motor sólo ve `embebido`/`grpc`,
  como siempre. Correr `anvil.wasm` suelto con wasmtime CLI (sin host)
  contra un ejecutor `wasm` da `Error::EjecutorWasmSinHost` con mensaje
  claro.
- **Caso remoto (Raspberry Pi, futuro)**: el mismo puente se distribuye
  suelto y se corre con `--bind 0.0.0.0`; el YAML declara `tipo: grpc,
  host: 192.168.x.y`. Anvil no distingue: el puente interno y el de la Pi
  son el mismo binario.
- **Rendimiento (50+ módulos)**: wasmtime compila **JIT a nativo** (no
  interpreta). AOT precompile a `.cwasm` + `StoreLimitsBuilder` son
  **post-M5-ext.2** (cuando se mida RSS). Detalle en
  `docs/planes/m5-ext.md`.

> **Patrón soportado desde M5-ext.1** (sin hito propio): un **único `.wasm`
> que despacha por nombre** (un módulo que atiende N nombres internamente)
> es un ejecutor `grpc` más — 1 Store, N llamadas. Anvil no distingue si
> detrás hay un `.wasm` suelto por path (M5-ext.2) o un módulo que fusiona
> varios pasos. Es el análogo del Run-Time Engine de TestStand: si un
> generador produce ese formato, funciona sin nada especial.

## Executores de lenguaje (`executors/`)

Módulos aparte, uno por sistema, distribuidos con Anvil, licencia
**Apache-2.0** (adoptables, ADR-0012):

```
executors/
  python/    # gRPC server en Python (M5)
  labview/   # futuro
  matlab/    # futuro
```

- Son **alternativas**: se arranca el que haga falta; pueden correr a la vez
  y mezclarse en la misma secuencia.
- Hablan el mismo `paso.proto` con **gRPC nativo de su ecosistema**
  (`grpcio`, `tonic`, …), no `wasi-grpc` (esa es solo para WASM, ADR-0006).
- El motor no necesita ese runtime instalado (ADR-0003); quien corre el
  ejecutor sí lo instala en su máquina — su elección, no un requisito de
  Anvil.
- Cada módulo es autocontenido y versionable → **descargable desde la UI**
  cuando exista (post-MVP).

### Objects that stay in the executor (ADR-0022)

A bench session, an instrument connection, a driver handle: a thing with open
sockets and vendor locks that **cannot cross the wire and must not be reopened
per step**. It stays in the executor's process, and the sequence carries a
`Reference` to it — which is the one thing a language executor can offer that
the embedded WASM one cannot, since `anvil:step` is a function with no state
between calls.

Two duties fall on whoever writes an executor, and **Anvil cannot check either
from outside**. An executor that breaks one is a broken executor:

1. **Never recycle a payload within one lifetime.** If a closed bench's key
   came back for the next open, an old reference would resolve cleanly to a
   live, *different* object: same executor, same lifetime, everything green,
   measuring against the wrong bench. A monotonic counter is what makes this
   impossible; a free list is what makes it happen.
2. **Mint a different lifetime on every start**, and publish it in
   `Catalog.lifetime`. A process that came back on the same lifetime would make
   its own restart undetectable, for Anvil and for itself.

And one it should do because it is the only one that can: **reject a reference
whose lifetime is not its own**. Anvil knows this only by comparison; the
executor knows it with certainty.

The Python executor is the worked example — `ctx.objects` is the store, and
`executors/python/steps/instrument.py` ships the shape any object steps take:
one opens and mints, several use, one closes. What it does *not* do is mint a
new handle when a step merely changes the bench: the reference names a slot,
and answering a new one would break retries.

### LID: despliegue en SO legacy (patrón, no componente — aplazado a post-M5-ext)

Cuando el paso exige DLLs/drivers de un SO que Anvil no ofrece (Windows 7/10,
Ubuntu antiguo), **cualquier** ejecutor de lenguaje puede desplegarse en ese
SO legacy con **aislamiento declarado** — es un *Legacy Isolation Domain*:

- Solo salen las **puertas declaradas** (instrumentos por red, ficheros
  pactados); el resto está aislado.
- Anvil ve un endpoint gRPC más: `192.168.x.y:9100` (PC en red) o una
  VM/contenedor local. No sabe ni le importa el SO.
- **Aplazado a post-M5-ext** (primero moderno, después legacy): el patrón es
  fijo, pero el **mecanismo de aislamiento a definir al construir**
  (contenedor / VM / firewall de SO). La investigación exhaustiva de
  opciones (QEMU/KVM, Hyper-V, Sandboxie-Plus, Docker, systemd-nspawn,
  namespaces, Windows Sandbox, Firecracker, gVisor, WSL2, …) con fuentes
  verificadas y recomendación por topología está en
  [investigacion/aislamiento-lid.md](../investigacion/aislamiento-lid.md).

## Configuración del routing

Patrón embebido primero, sidecar después (igual que los límites, RF-30):

1. **Embebido en el YAML de la secuencia** (MVP): sección `ejecutores:`
   versionable con la secuencia.

   ```yaml
   executors:
     - name: embebido        # el ejecutor WASM de serie
       type: embedded
     - name: mi_paso_wasm    # módulo .wasm cargado por path
       type: wasm
       path: ./pasos/mi_paso.wasm
     - name: python          # ejecutor de lenguaje aparte
       type: grpc              # mismo contrato, otro proceso/host
       host: 127.0.0.1         # o 192.168.x.y (LID) — solo si se declara
       port: 9101
   ```

   Y cada paso referencia su ejecutor: `ejecutor: python` en
   `DefinicionPaso` (o un ejecutor por defecto si no se declara).

2. **Override por flag CLI** (MVP): `--executor python=192.168.1.50:9100`
   para apuntar un ejecutor a otro endpoint sin tocar el YAML (R&D vs.
   fábrica), como ya hace `--limits`.

3. **Sidecar reutilizable** (post-MVP): fichero de configuración compartido
   entre varias secuencias.

Sin `ejecutores:` declarado, todo va al ejecutor embebido en loopback —
comportamiento idéntico al de M4b (compatibilidad con ADR-0011).

## Demo M5-ext.1 (hecha, sin Docker)

La demo real es `ejemplos/demo_ejecutores.yaml`: **embebido + Python en
loopback** (sin Docker, sin LID).

```yaml
name: demo_ejecutores
executors:
  - { name: embebido, type: embedded }
  - { name: python, type: grpc, host: 127.0.0.1, port: 9101 }
main:
  - name: verificar_led        # embebido (default)
  - name: medir_simulador, executor: python
  - name: conectar_equipo, executor: python
```

Verificación: la secuencia pasa/falla según cada paso, y el reporte muestra
pasos atendidos por dos ejecutores distintos sin que el motor supiera nada
del lenguaje. La demo con un paso `.wasm` propio (`tipo: wasm`) es
`ejemplos/demo_wasm.yaml` (M5-ext.2, ADR-0015): el host spawnea el puente,
que carga el componente `ejemplos/hola-paso` (el "hola mundo") y llama a su
`run`; el motor despacha los tres pasos (embebido + componente) con límite
y reintentos evaluados por el motor. Ver
[ADR-0015](../adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md).

## Recortes MVP extendido

- Cache AOT de módulos `.wasm` (post-M5-ext.2, cuando se mida RSS/threads
  con 50+ módulos).
- Sidecar de `ejecutores:` (post-MVP).
- Descubrimiento automático / balanceo / reconnect por endpoint (post-MVP;
  solo reintento por paso existente, RF-07).
- Descargables desde la UI (post-MVP; la estructura lo permite).
- LID: patrón documentado, aplazado a M5-ext.3; tecnología a definir al
  construir.

## Out-of-scope

- Ejecutores de lenguaje distintos de Python en el MVP (LabVIEW/MATLAB:
  futuros).
- WASM dentro del LID (imposible con DLLs nativas; su aislamiento es de
  red/FS declarados).
- Cambios a `paso.proto` (RNF-05).
