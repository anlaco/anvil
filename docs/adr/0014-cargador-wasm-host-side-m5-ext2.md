# ADR-0014: Cargador de `.wasm` por path host-side (M5-ext.2)

- **Estado:** Aceptada (parcialmente superseded por ADR-0015)
- **Fecha:** 2026-08-03 (M5-ext.2)
- **Superseded en parte por:** [ADR-0015](0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md)
  (M5-ext.2 rework): el contrato del `.wasm` del usuario cambia de
  "servidor gRPC que bindea `ANVIL_PORT`" a "componente WIT (`anvil:paso`)
  que exporta la función `run`". El host ya no instancia el `.wasm` como
  guest WASM: spawnea el puente `anvil-puente-wasm` (embebido en `anvil`),
  que carga el componente y traduce gRPC↔función. El resto de este ADR —
  host como cargador, puerto efímero, deduplicación por path, overrides
  `--ejecutor` sintéticos — se mantiene vigente.
- **Relaciona:** ADR-0001, ADR-0005, ADR-0011, ADR-0013 (superseded en la
  parte del "el motor entiende `Wasm`"),
  [arquitectura.md](../arquitectura.md),
  [contrato-grpc.md](../contrato-grpc.md),
  [diseno/executores-lenguaje.md](../diseno/executores-lenguaje.md),
  [planes/m5-ext.md](../planes/m5-ext.md)

## Contexto

ADR-0013 dejó el cargador de `.wasm` por path (RF-36.2, modelo `.vi` de
TestStand) como M5-ext.2, pendiente: el `TipoEjecutor::Wasm { path }` se
definía y validaba al cargar (el path debe existir), pero ejecutarlo daba
`Error::EjecutorWasmNoImplementado`. El host es quien instancia wasmtime
(ADR-0011); un guest WASM no puede instanciar wasmtime dentro de sí mismo.

Tres preguntas abiertas al arrancar M5-ext.2:

1. **¿Cómo el motor aprende los puertos de los `.wasm` que el host
   instancia?** ADR-0013 decía "el motor entiende `Wasm`". Pero el guest
   motor **re-parsea el YAML él mismo** (ADR-0005: el motor no recibe un
   `Programa` en memoria; `crates/motor/src/bin/anvil.rs` vuelve a llamar a
   `cargar_programa_de_archivo`). El host no puede reescribirle el modelo en
   memoria — no tiene el modelo, tiene el YAML.
2. **¿Qué puerto usa cada `.wasm`?** Un guest WASM arbitrario no puede
   tener un puerto fijo (el embebido hardcodea 9100).
3. **¿Cuántos Stores por módulo?** El caso de uso real es 50+ módulos en
   una secuencia larga; dos ejecutores con el mismo `.wasm` no deben
   duplicar Store.

## Decisión

### 1. El motor nunca ejecuta `Wasm`: el host lo traduce a `grpc` al arrancar

- `TipoEjecutor::Wasm { path }` queda en el modelo como **directiva de
  carga para el host** (el cargador la valida: path existe, coherencia de
  campos). El motor **no** la ejecuta: si un `Wasm` llega al motor sin
  traducir (sólo posible corriendo `anvil.wasm` suelto con wasmtime CLI sin
  el host), da `Error::EjecutorWasmSinHost` con mensaje claro que apunta al
  host.
- El host, al arrancar, instancia cada `.wasm` declarado y **compone un
  override `--ejecutor nombre=127.0.0.1:<puerto>` sintético** que se añade
  a los args del guest motor. El motor lo aplica con el mecanismo ya
  existente de M5-ext.1 (`aplicar_override_ejecutores`, que **convierte
  `wasm` → `grpc`**; testeado). El motor termina viendo sólo `embebido` y
  `grpc`, como siempre.
- **Por qué overrides y no reescritura del YAML:** el motor no recibe un
  `Programa` en memoria (ADR-0005: todo lo que el motor conoce le llega por
  args/preopens, no por un objeto inyectado por el host). Los overrides son
  el mecanismo ya existente para re-apuntar ejecutores; reusarlo evita un
  canal nuevo (env var, fichero sidecar, RPC de control) que no aporta
  nada al contrato.
- `Error::EjecutorWasmNoImplementado` desaparece (ya no hay "pendiente de
  implementar").

### 2. Convención `ANVIL_PORT`: el `.wasm` lee su puerto del env

- Un guest WASM de paso de Anvil bindea **`127.0.0.1:$ANVIL_PORT`** (env
  inyectado por el host en su `WasiCtx`), con default `9100` para compat
  con `wasmtime run` sin host.
- El ejecutor embebido (`ejecutor_pasos`) migra a la misma convención
  (default 9100): **un `.wasm` cargado por path es igual al embebido** —
  mismo contrato `paso.proto`, mismo sandbox loopback-only, mismo env.
- El puerto lo **reserva el host** con `bind 127.0.0.1:0` (efímero) antes de
  lanzar el guest; el guest bindea ese puerto al arrancar (readiness por
  polling, como `esperar_ejecutor`). Ventana mínima entre el drop del
  listener de reserva y el bind del guest — suficiente para el MVP.
- **Por qué efímero y no rango fijo:** robusto sin configuración (sin
  races con otros procesos, sin elegir puertos a mano); el host loguea el
  mapeo `nombre → 127.0.0.1:puerto` al cargar. Para debug, el log es el
  mapa. Un rango fijo con 50 módulos sería frágil.
- **Por qué env y no argv:** `ANVIL_PORT` llega limpio al guest sin
  depender de que éste parsee la línea de comandos; un `.wasm` de paso sólo
  necesita "el puerto donde escuchar" y el contrato `paso.proto` — nada
  más de Anvil.

### 3. Un `Store` por path (deduplicación), preload al arrancar

- Dos ejecutores con el **mismo path** comparten un único Store (un `.wasm`
  = un Store = un puerto). Es el patrón "1 Store, N llamadas" (el RTE de
  TestStand) materializado por el host.
- **Preload** de todos los `.wasm` al arrancar (como TestStand por defecto):
  se instancian y se espera su readiness antes de lanzar el motor; quedan
  cacheados hasta que el proceso termina. Sin lazy loading en el MVP.
- Sandbox de cada `.wasm`: **loopback-only, sin relajación** (sólo recibe
  del motor, nunca de la red exterior) — igual que el ejecutor embebido.

### 4. Sin cambios al contrato ni al motor (salvo el error)

- `paso.proto` no cambia (RNF-05).
- `Motor::desde_programa` no cambia: abre conexiones `Grpc`, que es lo
  único que verá.
- `resolver_endpoint` cambia el caso `Wasm`: de error "no implementado" a
  error "sin host" (defense in depth; el flujo normal nunca lo dispara).

## Por qué esta forma

- **Es la única técnicamente posible**: el host es el único con wasmtime
  (ADR-0011); el guest motor no puede instanciar wasmtime (ADR-0013); y el
  motor no recibe un `Programa` en memoria (ADR-0005), así que la
  traducción tiene que pasar por un mecanismo que el motor ya entienda —
  los overrides de ejecutores de M5-ext.1.
- **Uniformidad**: un `.wasm` cargado por path es indistinguible del
  embebido para el motor; el ejecutor embebido y un `.wasm` por path usan
  la misma convención (`paso.proto` + `ANVIL_PORT` + loopback-only). La
  única diferencia es quién los arranca (embebido en el host, por path el
  usuario).
- **Agnóstico al generador**: si habla `paso.proto` por gRPC en loopback y
  lee `ANVIL_PORT`, el `.wasm` funciona — C a mano, Rust, Zig, un editor
  visual, un tercero. El roadmap de Anvil no depende del generador.
- **Seguridad conservada**: nada sale de loopback sin declararlo (los
  `.wasm` ni siquiera pueden salir: loopback-only estricto).

## Recortes y compromisos

- **Sin AOT precompile a `.cwasm`** ni `StoreLimitsBuilder` (post-M5-ext.2,
  cuando se mida RSS/threads con 50+ módulos; JIT de wasmtime basta hoy).
- **Sin lazy loading** (preload al arrancar, como TestStand por defecto).
- **Sin modo Debug** (`Config::debug_info(true)` + LLDB) en M5-ext.2.
- **Readiness por polling** (mismo patrón que el embebido), no un canal
  `thread → main` estructurado. Post-MVP si el polling se nota lento.
- **Sin shutdown ordenado** de los Stores: los threads se abortan al salir
  el proceso (igual que el embebido hoy).
- El **ejecutor standalone** (desplegar un `.wasm` en otra máquina, p. ej.
  una Raspberry Pi, con un runtime mínimo) **no** está en M5-ext.2: se
  discutirá aparte.

## Consecuencias

- ADR-0013 queda **superseded** en la parte "el motor entiende `Wasm`" (el
  punto 1 de su Decisión y el `Error::EjecutorWasmNoImplementado`); el
  resto (routing nombre→endpoint, relajación acotada del loopback, host
  como cargador) se mantiene.
- `TipoEjecutor::Wasm` es ahora una **directiva declarativa**: el cargador
  la valida (fail-fast al cargar), el host la materializa (instancia +
  traduce a `grpc`), el motor no la ejecuta.
- La convención `ANVIL_PORT` es el "formato Anvil de `.wasm` de paso": el
  contrato del cable (`paso.proto`) + la forma de arranque (bind al puerto
  del env, loopback-only). Un futuro ejecutor standalone la reusaría.
- La demo `ejemplos/demo_wasm.yaml` verifica el flujo end-to-end: el host
  carga `ejecutor_pasos.wasm` por path, le asigna puerto, el motor despacha
  los tres pasos (embebido + `.wasm`), límite y reintentos evaluados por el
  motor.
