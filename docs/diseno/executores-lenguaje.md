# Diseño: Executores de lenguaje y cargador de `.wasm`

> **Prioridad:** MVP extendido. El ejecutor WASM embebido ya existe; el
> routing nombre→endpoint está **implementado en M5-ext.1** (ADR-0013); el
> cargador de `.wasm` por path es **M5-ext.2** (agnóstico al origen del
> `.wasm`); LID es un patrón de despliegue **aplazado a M5-ext.3**.

Cómo Anvil llama a pasos en **cualquier lenguaje** y a **módulos WASM
propios** sin recompilar. Trazable a [ADR-0013](../adr/0013-cargador-wasm-host-side-y-routing.md),
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
ejecutores:
  - nombre: embebido        # el ejecutor WASM de serie (127.0.0.1:9100)
    tipo: embebido
  - nombre: python          # ejecutor de lenguaje aparte
    tipo: grpc
    host: 127.0.0.1         # o 192.168.x.y (LID futuro) — solo si se declara
    puerto: 9101
main:
  - nombre: verificar_led   # embebido (default)
  - nombre: medir_simulador
    ejecutor: python
```

Override por CLI: `--ejecutor python=192.168.1.50:9101` (patrón `--limits`).
IPs no-loopback solo si se declaran (relajación acotada del loopback,
ADR-0011); flag `--solo-loopback` en el host para rechazarlas.

### Cargador de `.wasm` por path (modelo `.vi`, M5-ext.2)

Como en TestStand con un `.vi`: tú compilas el módulo, lo guardas en un
archivo, y la secuencia lo referencia por path. **No se recompila nada.**

```yaml
ejecutores:
  - nombre: mi_paso_wasm      # clave libre para la secuencia
    tipo: wasm                # módulo cargado por el HOST (ADR-0013)
    path: ./pasos/mi_paso.wasm  # relativo al YAML
```

- **El host** (no el ejecutor embebido) carga el `.wasm` en su propio
  `Store` (sandbox separado por módulo): un paso defectuoso no bloquea al
  ejecutor ni a otros módulos.
- En **M5-ext.1** el path se valida al cargar (debe existir) pero el módulo
  **no se instancia**: ejecutarlo da `Error::EjecutorWasmNoImplementado`
  ("requiere anvil-host con soporte M5-ext.2"). La instanciación real queda
  para M5-ext.2.
- El contrato de entrada/salida del módulo es el mismo `PeticionPaso` /
  `ResultadoPasoProto` (reusado; ver
  [modelo-de-pasos.md](modelo-de-pasos.md) para cómo se despacha por nombre
  dentro del módulo). **Agnóstico al lenguaje y al generador del `.wasm`**:
  C a mano, Rust, Zig, un editor visual, un tercero — si habla `paso.proto`
  por gRPC en loopback, Anvil lo atiende. El roadmap avanza por los
  requisitos de Anvil, no por los de un producto externo.
- **Rendimiento (50+ módulos)**: wasmtime compila **JIT a nativo** (no
  interpreta). Para el caso de uso real (50+ módulos `.wasm` en una
  secuencia larga, como los 50+ VIs de TestStand): AOT precompile a `.cwasm`
  + `StoreLimitsBuilder` + lazy loading + preload al abrir la secuencia.
  Detalle en `docs/planes/m5-ext.md`.

> **Patrón soportado desde M5-ext.1** (sin hito propio): un **único `.wasm`
> que despacha por nombre** (un módulo que atiende N nombres internamente)
> es un ejecutor `grpc` más — 1 Store, N llamadas. Anvil no distingue si
> detrás hay un `.wasm` suelto por path (M5-ext.2) o un módulo que fusiona
> varios pasos. Es el análogo del Run-Time Engine de TestStand: si un
> generador produce ese formato, funciona sin nada especial.

## Executores de lenguaje (`executores/`)

Módulos aparte, uno por sistema, distribuidos con Anvil, licencia
**Apache-2.0** (adoptables, ADR-0012):

```
executores/
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
   ejecutores:
     - nombre: embebido        # el ejecutor WASM de serie
       tipo: embebido
     - nombre: mi_paso_wasm    # módulo .wasm cargado por path
       tipo: wasm
       path: ./pasos/mi_paso.wasm
     - nombre: python          # ejecutor de lenguaje aparte
       tipo: grpc              # mismo contrato, otro proceso/host
       host: 127.0.0.1         # o 192.168.x.y (LID) — solo si se declara
       puerto: 9101
   ```

   Y cada paso referencia su ejecutor: `ejecutor: python` en
   `DefinicionPaso` (o un ejecutor por defecto si no se declara).

2. **Override por flag CLI** (MVP): `--ejecutor python=192.168.1.50:9100`
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
nombre: demo_ejecutores
ejecutores:
  - { nombre: embebido, tipo: embebido }
  - { nombre: python, tipo: grpc, host: 127.0.0.1, puerto: 9101 }
main:
  - nombre: verificar_led        # embebido (default)
  - nombre: medir_simulador, ejecutor: python
  - nombre: conectar_equipo, ejecutor: python
```

Verificación: la secuencia pasa/falla según cada paso, y el reporte muestra
pasos atendidos por dos ejecutores distintos sin que el motor supiera nada
del lenguaje. La demo con un paso `.wasm` propio (`tipo: wasm`) llegará con
el cargador host-side (M5-ext.2).

## Recortes MVP extendido

- Cargador `.wasm` por path (M5-ext.2): en M5-ext.1 el path se valida pero
  no se instancia. Agnóstico al origen del `.wasm`.
- Cache AOT de módulos `.wasm` (con M5-ext.2; post-MVP para el caso 50+).
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
