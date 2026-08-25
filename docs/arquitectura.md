# Arquitectura

Arquitectura de Anvil en **C4 niveles 1–3** (Contexto, Container, Component).
Sustituye al SDD IEEE 1016 (12 viewpoints) —exceso aquí— conservando lo
que importa: qué se construye, cómo se aísla, dónde vive el estado y dónde
está la frontera de licencia.

Las decisiones de fondo están en los [ADRs](adr/); este doc es el *cómo*.

## Nivel 1 — Contexto del sistema

```
                  ┌───────────────┐
   Ingeniero      │   Anvil       │      ResultSinks
   de test ──────▶│  secuenciador │─────▶ (consola/JSON/CSV/
  (authora YAML)  │  (WASM host)  │      SQLite/STDF post-MVP)
                  └───────┬───────┘
                          │ gRPC (por nombre)
                          ▼
                  ┌───────────────┐         ┌──────────────────┐
   Operador ─────▶│  Ejecutor de  │────────▶│  Pasos / code     │
  (corre en       │  pasos (WASM) │ gRPC    │  modules (cual-   │
   planta, CLI)   │               │         │  quier lenguaje)  │
                  └───────────────┘         └────────┬─────────┘
                                                      │ SCPI/Visa (post-MVP)
                                                      ▼
                                               ┌──────────────┐
                                               │ Instrumentos │
                                               │  (hardware)  │
                                               └──────────────┘
```

- **Ingeniero de test** authora la secuencia como YAML (datos) y la
  versiona en Git.
- **Operador** corre la secuencia en planta, headless/CLI en el MVP.
- **Anvil (secuenciador)** = el motor WASM. Recorre la secuencia y pide cada
  paso por gRPC.
- **Ejecutor de pasos** = servidor gRPC que despacha pasos por nombre. En el
  MVP hospeda los pasos en el mismo `.wasm`; el objetivo es que los pasos
  puedan ser **servicios gRPC en cualquier lenguaje**.
- **Pasos / code modules** = la lógica de medición, en cualquier lenguaje.
  Tocan los **instrumentos** (SCPI/VISA post-MVP).
- **ResultSinks** reciben los resultados como dato abierto (hoy `println!`).
- **wasi-grpc** (lib externa, Apache-2.0, `../wasi-grpc`) es la pila de
  transporte: no es de Anvil pero es su base ([ADR-0006](adr/0006-wasi-grpc-propio.md)).

## Nivel 2 — Containers

Cosas que se despliegan o existen de forma independiente:

```
┌─────────────────────────────────────────────────────────────┐
│  Motor  (crates/motor → motor.wasm, wasmtime)                 │
│  Cliente gRPC. Recorre DefinicionSecuencia, aplica semántica. │
│  Contiene TODO el estado de la ejecución en memoria.          │
└─────────────────────────────────────────────────────────────┘
        │ gRPC  /EjecutorPasos/Invoca   (wasi-grpc, un stream/call)
        ▼
┌─────────────────────────────────────────────────────────────┐
│  Ejecutor de pasos  (crates/ejecutor_pasos → .wasm, wasmtime)  │
│  Servidor gRPC en 127.0.0.1:9100. Despacha por nombre.         │
│  MVP: hospeda pasos_demo en el mismo .wasm. Stateless entre    │
│  llamadas.                                                    │
└─────────────────────────────────────────────────────────────┘
        │ llamada directa en proceso (MVP)  ──── futuro: gRPC a
        ▼                                      servidores de paso
┌─────────────────────────────────────────────────────────────┐
│  Pasos  (crates/pasos_demo hoy; cualquier lenguaje mañana)   │
│  Code modules: medir_voltaje, verificar_led, conectar…        │
└─────────────────────────────────────────────────────────────┘

┌──────────────┐   ┌──────────────────────┐   ┌─────────────────┐
│ Secuencia    │   │ ResultSink (post-MVP) │   │ wasi-grpc (lib)  │
│ YAML (datos) │   │ consola/JSON/CSV/      │   │ Apache-2.0,      │
│ (RF-20)      │   │ SQLite/STDF (RF-21+)  │   │ repo aparte      │
└──────────────┘   └──────────────────────┘   └─────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  modelo  (crates/modelo, lib)  — compartido, NO despliega solo │
│  DefinicionSecuencia, ResultadoStep, mensajes proto.rs.       │
│  Licencia: AGPL (parte del producto).                         │
└─────────────────────────────────────────────────────────────┘
```

**MVP actual vs. objetivo (honesto):**

| Container | Hoy (prototipo) | Objetivo |
|---|---|---|
| Motor | `motor.wasm` construye la secuencia en código (`basica_datos.rs`) | Lee secuencia YAML (RF-20) |
| Ejecutor + Pasos | Mismo `.wasm`; `pasos_demo` linkado en proceso | Pasos como servicios gRPC en cualquier lenguaje |
| Reporte | `println!` textual congelado | ResultSink desacoplado (RF-21) |
| UI | Sin UI (CLI) | Operator UI web post-MVP |
| Proceso de test | Sequential implícito (una secuencia) | Process model Sequential + plug-ins |

La **frontera gRPC motor↔ejecutor ya existe y es real** (aislamiento
motor-side). La frontera ejecutor↔paso es en-proceso hoy; el objetivo es
que sea gRPC para pasos en cualquier lenguaje (ADR-0003).

**M5-ext.1 (ADR-0013):** el ejecutor WASM embebido se mantiene como **de
serie** (zero-install, ADR-0011); el motor despacha por **nombre→endpoint**
(`ejecutores:` en el YAML + override `--executor`), con IPs no-loopback
solo si se declaran. A su lado, **executores de lenguaje** (`executores/`,
Apache-2.0) atienden pasos con gRPC nativo de su ecosistema.

**M5-ext.2 (ADR-0014/0015):** el **cargador de módulos `.wasm` por path**
(modelo `.vi` de TestStand) lo hace el **host**: para cada `tipo: wasm` del
YAML spawnea el **puente** `anvil-puente-wasm` (embebido en el binario,
extraído a temp), que carga el componente `.wasm` del usuario (interfaz WIT
`anvil:paso`: una función `run`, sin gRPC ni protobuf) y traduce
gRPC↔función por tonic. El puente corre con sandbox WASI vacío (el
componente es una función pura). El motor sólo ve overrides `--executor`
sintéticos — nunca un `Wasm`. El patrón **LID** para SO legacy queda
aplazado a post-M5-ext. Ver
[diseno/executores-lenguaje.md](diseno/executores-lenguaje.md),
[ADR-0013](adr/0013-cargador-wasm-host-side-y-routing.md),
[ADR-0014](adr/0014-cargador-wasm-host-side-m5-ext2.md) y
[ADR-0015](adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md).

## Nivel 3 — Componentes

### Dentro del Motor (`crates/motor/src/lib.rs`)

```
Motor
 ├─ desde_programa(programa)     → tabla de conexiones por ejecutor (M5-ext.1)
 ├─ conecta(host, puerto)        → Cliente wasi-grpc (legacy, embebido)
 ├─ ejecuta_paso(def, programa)  → resuelve endpoint por def.ejecutor, codifica
 │                                 PeticionPaso, llama RUTA_INVOCA, decodifica
 ├─ ejecuta_con_reintentos(def)  → reintenta mientras !paso() && intento<max
 └─ ejecuta_secuencia(def)       → Setup / Main(corta en 1er fallo) / Cleanup(siempre)
                                   + agrega en ResultadoSecuencia
```

- **Estado:** toda la ejecución vive en `ResultadoSecuencia` **en memoria**;
  no hay persistencia en el MVP. El ResultSink (post-MVP) la verterá.
- **Errores:** `Error::Red` (comunicación) / `Error::Protobuf` (respuesta
  ilegible). Un paso que *falla* **no** es error del motor (RF-11).

### Dentro del Ejecutor (`crates/ejecutor_pasos/src/main.rs`)

```
Ejecutor
 ├─ Servidor::escuchar(127.0.0.1:9100) → aceptar() una conexión
 ├─ loop: siguiente_peticion()         → valida path == RUTA_INVOCA
 │   ├─ decodifica PeticionPaso
 │   ├─ pasos_demo::despacha(nombre, intento)   ← único punto nombre→función
 │   └─ responde(stream, ResultadoPasoProto)
 └─ Stateless entre llamadas
```

- **Despacho por nombre:** `pasos_demo::despacha` es el **único** sitio donde
  el nombre del cable se ata a una función. Nombre desconocido → `error`
  (RF-12).
- Cada llamada gasta un **stream HTTP/2 nuevo** (gestionado por wasi-grpc).

## Por qué WASM

Ver [ADR-0001](adr/0001-rust-wasm.md). En resumen: **aislamiento** (sandbox
del secuenciador; el interior de cada paso es opaco al motor) +
**portabilidad** (un `.wasm` corre en cualquier SO con wasmtime, sin
instalador) + **determinismo** (base para reintentos reproducibles). El
coste (sin `tonic`/`tokio` → pila propia, sin codegen → structs a mano) se
paga en [ADR-0006](adr/0006-wasi-grpc-propio.md) y `crates/modelo/src/proto.rs`.

**Rendimiento (ADR-0012):** wasmtime compila WASM **JIT a código nativo**
(no lo interpreta): ~1.5–2× de C/Rust nativo y muy por delante de Python
puro; frente a una DLL nativa paga ~10–30% por el sandbox, despreciable
frente al tiempo de un instrumento real (RNF-04). No hay razón para "lo
rápido en DLL, lo lento en WASM".

## Dónde vive el estado

- **La ejecución** (resultado en curso): en memoria, en el Motor, como
  `ResultadoSecuencia`. No persiste en el MVP.
- **La definición** (qué correr): datos (`DefinicionSecuencia`), hoy
  construidos en código, mañana YAML. Es **inerte**: no muta al ejecutarse.
- **El ejecutor**: stateless entre llamadas. No guarda nada de la secuencia.
- **Variables de secuencia** (Locals/Parameters/FileGlobals, post-MVP):
  vivirán en el Motor, ligadas al alcance de la ejecución
  ([diseno/variables-y-alcances.md](diseno/variables-y-alcances.md)).

## Modelo de concurrencia

- **MVP: secuencial.** Una conexión, un stream por llamada, una secuencia a
  la vez. No hay hilos en el motor; el determinismo de reintentos depende de
  eso (RNF-03).
- **Post-MVP: paralelismo con cancelación jerárquica** (Parallel/Batch), con
  un *CancellationToken* (estilo OpenTAP TapThreads) para abortar en cascada.
  Ver [diseno/proceso-de-test.md](diseno/proceso-de-test.md).

## Frontera de licencia (Apache / AGPL)

```
AGPL-3.0-or-later                     Apache-2.0
─────────────────────                ─────────────────────
anvil (el producto):                  wasi-grpc   (lib, linkable)
  motor, ejecutor_pasos,              wasi-visa   (lib, linkable, post-MVP)
  modelo, pasos_demo                  interfaces WIT
   (se USAN, no se linkan)
```

- Quien **usa** Anvil (lo corre) no recibe contagio AGPL.
- Quien **linka** las libs (escribe un paso que enlaza `wasi-grpc`/`wasi-visa`)
  está bajo Apache: su código le pertenece.
- Las **secuencias** son datos: no obra derivada, no contagian
  ([ADR-0004](adr/0004-licencia-dual-agpl-apache.md), [licencia.md](licencia.md)).

## Determinismo y rendimiento

- **Determinismo:** para la misma secuencia y los mismos pasos, el número de
  intentos y el orden son reproducibles porque no hay concurrencia implícita
  en el MVP (RNF-03). Verificar en CI con los pasos simulados de `pasos_demo`
  (p. ej. `conectar` falla el intento 1 y pasa el 2).
- **Rendimiento:** el overhead de una llamada gRPC local es despreciable
  frente al tiempo de un instrumento real (RNF-04). No es cuello de botella.