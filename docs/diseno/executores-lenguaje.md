# Diseño: Executores de lenguaje y cargador de `.wasm`

> **Prioridad:** MVP extendido. El ejecutor WASM embebido ya existe; el
> cargador de `.wasm` por path y los ejecutores de lenguaje son **MVP
> extendido** (M5). LID es un patrón de despliegue, no un componente.

Cómo Anvil llama a pasos en **cualquier lenguaje** y a **módulos WASM
propios** sin recompilar. Trazable a [ADR-0012](../adr/0012-executores-de-lenguaje-como-modulos.md),
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
- Atiende dos clases de pasos:
  1. **Built-in**: `pasos_demo` compilado dentro (pass/fail, limit test,
     action, conectar/medir/desconectar simulados). Siempre disponibles.
  2. **Módulos `.wasm` cargados por path**: tu paso compilado a WASM,
     referenciado en la secuencia, cargado en runtime.

### Cargador de `.wasm` por path (modelo `.vi`)

Como en TestStand con un `.vi`: tú compilas el módulo, lo guardas en un
archivo, y la secuencia lo referencia por path. **No se recompila el
ejecutor.**

```yaml
ejecutores:
  - nombre: mi_paso_wasm      # clave libre para la secuencia
    tipo: wasm                # módulo cargado por el ejecutor embebido
    path: ./pasos/mi_paso.wasm  # relativo al YAML
```

- El ejecutor carga el `.wasm` **en su propio `Store`** (sandbox separado por
  módulo): un paso defectuoso no bloquea al ejecutor ni a otros módulos.
- El contrato de entrada/salida del módulo es el mismo `PeticionPaso` /
  `ResultadoPasoProto` (reusado; ver
  [modelo-de-pasos.md](modelo-de-pasos.md) para cómo se despacha por nombre
  dentro del módulo).
- **Rendimiento**: wasmtime compila **JIT a nativo** (no interpreta). La
  primera invocación paga la compilación una sola vez; cache AOT post-MVP.

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

### LID: despliegue en SO legacy (patrón, no componente)

Cuando el paso exige DLLs/drivers de un SO que Anvil no ofrece (Windows 7/10,
Ubuntu antiguo), **cualquier** ejecutor de lenguaje puede desplegarse en ese
SO legacy con **aislamiento declarado** — es un *Legacy Isolation Domain*:

- Solo salen las **puertas declaradas** (instrumentos por red, ficheros
  pactados); el resto está aislado.
- Anvil ve un endpoint gRPC más: `192.168.x.y:9100` (PC en red) o una
  VM/contenedor local. No sabe ni le importa el SO.
- **Mecanismo de aislamiento a definir al construir** (contenedor / VM /
  firewall de SO). El patrón es fijo; la tecnología se decide por caso. La
  investigación exhaustiva de opciones (QEMU/KVM, Hyper-V, Sandboxie-Plus,
  Docker, systemd-nspawn, namespaces, Windows Sandbox, Firecracker, gVisor,
  WSL2, …) con fuentes verificadas y recomendación por topología está en
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

## Demo M5 (prueba mínima real)

Tres piezas, una secuencia, la misma tesis:

1. **Paso built-in**: `ejecutores.embebido` sirve `verificar_led` (pasos_demo).
2. **Paso `.wasm` propio**: un módulo compilado aparte (p. ej. Rust →
   `mi_paso.wasm`) cargado por path en su propio `Store`.
3. **Paso en ejecutor Python**: `executores/python/` arrancado en
   `127.0.0.1:9101`, que para un nombre de paso abre **TCP al simulador de
   instrumento** (el que está desarrollando otro equipo) y devuelve
   `ResultadoPasoProto`.

```yaml
nombre: demo_ejecutores
ejecutores:
  - { nombre: embebido, tipo: embebido }
  - { nombre: mi_paso_wasm, tipo: wasm, path: ./pasos/mi_paso.wasm }
  - { nombre: python, tipo: grpc, host: 127.0.0.1, puerto: 9101 }
main:
  - nombre: verificar_led        # embebido (default)
  - nombre: medir_simulador, ejecutor: python
  - nombre: mi_paso_wasm, ejecutor: mi_paso_wasm
```

Verificación: la secuencia pasa/falla según cada paso, y el reporte muestra
pasos atendidos por tres ejecutores distintos sin que el motor supiera nada
del SO ni del lenguaje. Para el escenario LID, la misma secuencia con
`python` apuntando a `192.168.x.y:9100`.

## Recortes MVP extendido

- Cache AOT de módulos `.wasm` (post-MVP).
- Sidecar de `ejecutores:` (post-MVP).
- Descubrimiento automático / balanceo / reconnect por endpoint (post-MVP;
  solo reintento por paso existente, RF-07).
- Descargables desde la UI (post-MVP; la estructura lo permite).
- Aislamiento del LID: patrón documentado, tecnología a definir al construir.

## Out-of-scope

- Ejecutores de lenguaje distintos de Python en el MVP (LabVIEW/MATLAB:
  futuros).
- WASM dentro del LID (imposible con DLLs nativas; su aislamiento es de
  red/FS declarados).
- Cambios a `paso.proto` (RNF-05).
