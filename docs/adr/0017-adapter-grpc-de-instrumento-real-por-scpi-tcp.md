# ADR-0017: Adapter gRPC de instrumento real por SCPI/TCP

- **Estado:** Aceptada
- **Fecha:** 2026-08-03 (M5)
- **Relaciona:** ADR-0001, ADR-0003, ADR-0005, ADR-0011,
  [integracion-instrumentos.md](../diseno/integracion-instrumentos.md),
  [contrato-grpc.md](../contrato-grpc.md)

## Contexto

Hasta M5 el ejecutor despachaba sólo `pasos_demo`: pasos **simulados** que
devuelven valores fijos en código (p. ej. `medir_voltaje` → 4.2 V). La
frontera gRPC motor↔paso era real (ADR-0003/0006), pero la integración con
el instrumento era simulada. RF-36 pide "pulir el adapter": acercar el paso
al hardware real, capa 1 de `integracion-instrumentos.md` — un paso que
hable de verdad con un instrumento por SCPI/TCP. Sin hardware disponible
para testear, la verificación debe ser determinista.

## Decisión

Un paso Rust real que abre un `std::net::TcpStream`, envía un comando
SCPI (`MEASURE:VOLTAGE?\n`) y parsea la respuesta numérica. Vive en un
**crate nuevo `pasos_scpi`** (separado de `pasos_demo`), y se expone como
`pasos_scpi::medir_voltaje_scpi(intento) -> ResultadoStep` y
`pasos_scpi::despacha(nombre, intento) -> Option<ResultadoStep>`.

El ejecutor (`crates/ejecutor_pasos`) se convierte en raíz de composición:
despacha consultando `pasos_scpi` primero (adapter real) y `pasos_demo`
después (simulados); un nombre desconocido en ambos cae a `error` en
`pasos_demo::despacha` (RF-12, no pánico).

`paso.proto` **no cambia** (RNF-05): el paso se invoca por nombre como
cualquier `grpc`; el motor no sabe que habla SCPI ni TCP (ADR-0005). El
aislamiento motor↔paso se preserva (ADR-0003).

La dirección del instrumento se toma de la env var `ANVIL_SCPI_ADDR`
(default `127.0.0.1:5025`). Los tests levantan un servidor TCP mock en
`127.0.0.1:0` (puerto efímero) y usan `medir_voltaje_scpi_en(addr, intento)`
— la variante parametrizable — para no competir por la env var entre tests
paralelos y no cuelgar (el mock hace `accept` con timeout).

## Por qué esta forma

- **Mantiene el adapter gRPC** (ADR-0003/0006): no se reescribe el
  transporte; el paso habla `wasi:sockets` como ya lo hacía el ejecutor
  que bindea 9100.
- **Compila a `wasm32-wasip2`** (`std::net` vía `wasi:sockets`, ADR-0001)
  y a nativo (tests), sin dependencias externas. El host embebido
  restringe los sockets a loopback (`socket_addr_check → is_loopback()`,
  ADR-0011): el instrumento TCP al que se conecta el paso debe ser
  loopback en el sandbox; se documenta.
- **Crate separado** (`pasos_scpi`) para no ensuciar `pasos_demo` (que
  sigue siendo simulación pura, útil como referencia y tests sin red) y
  para que el adapter real crezca a más instrumentos en su propio crate.
- **Determinismo en tests sin hardware**: el mock responde `4.8\n`
  (numérico → `paso`), `OVEN_COLD\n` (no numérico → `error`) y
  sin servidor (connect falla → `error`). Cubre los tres caminos del paso.

## Recortes MVP-parcial

- **Sólo TCP, sólo `MEASURE:VOLTAGE?`.** Sin serial/VISA ni otros comandos.
- **Sin `wasi-visa`** (Apache, capa 2 estilo PyVISA): post-MVP.
- **Sin perfiles YAML de instrumento** con `SimBackend` determinista +
  **record/replay** estricto (`ReplayMismatchError`) para CI sin hardware:
  post-MVP (el mock de tests cubre la verificación hoy, pero es local al
  test, no un backend reutilizable).
- **El host embebido no plubea env vars** al guest: `ANVIL_SCPI_ADDR`
  rige el default en el binario único; el override es útil en el path de
  dos terminales con wasmtime y en tests nativos. Plumbear env al guest
  (`WasiCtxBuilder::envs`) es post-MVP.
- **Sin `--port` en el ejecutor**: el ejecutor bindea 9100 hardcoded;
  `--port` (RF-40) apunta el motor a un ejecutor en otro puerto.

## Consecuencias

- ADR-0001 se **refuerza**: el sandbox WASM sigue; `std::net` vía
  `wasi:sockets` demuestra que un paso real de instrumento compila y corre
  bajo WASI P2 sin romper el aislamiento.
- ADR-0003 se **realiza**: "el instrumento vive detrás del paso, opaco al
  motor" deja de ser teórico — hay un paso que toca (un mock de) un
  instrumento real por red, y el motor no se entera.
- El ejecutor pasa de "un despachador simulado" a "raíz de composición de
  adaptadores": añadir un adapter es añadir un `despacha` y consultarlo en
  el orden deseado. El motor no cambia.