# ADR-0003: Cada paso se invoca por gRPC por su nombre

- **Estado:** Aceptada (decisión pre-existente, formalizada aquí)
- **Fecha:** pre-prototipo

## Contexto

TestStand ataca el "pasos en cualquier lenguaje" con *adapters* (LabVIEW,
C/C++, .NET, Python) — pero en producción, con el adapter en runtime, **se
lanza el LabVIEW Dev System igual**, un bug sin arreglar de TS2020 a
TS2023Q4. Es decir, "cualquier lenguaje" en TestStand cuesta un runtime de
vendor atado y dependencias ocultas que se resuelven distinto en runtime
que en Dev ([investigación](../investigacion/TestStand-y-competencia.md) §2,
Lock-in LabVIEW y Dependencias compiladas ocultas).

Los secuenciadores open-source son mono-lenguaje (OpenHTF, Litmus, Flojoy en
Python; OpenTAP en C#). Nadie ofrece pasos en **cualquier lenguaje** tras un
contrato tipado (white-space nº2, investigación §4).

## Decisión

Cada paso se invoca **por gRPC por su nombre**, nunca con una llamada
directa. El motor (`crates/motor`) pide `PeticionPaso{nombre, intento}` al
ejecutor (`crates/ejecutor_pasos`) por la ruta `/EjecutorPasos/Invoca`
(`crates/modelo/paso.proto`); el ejecutor ata ese nombre a una función
concreta (hoy `pasos_demo::despacha`).

El *adapter* **es gRPC**: cualquier lenguaje que hable gRPC es un adapter,
sin código de pegamento en el motor y sin runtime de vendor.

## Consecuencias

**Positivas:**

- **Lenguaje-agnóstico de verdad**: un paso puede escribirse en Rust, Python,
  C++, Go… mientras hable el contrato protobuf.
- **Aislamiento por contrato**: el interior del paso es opaco al motor; las
  dependencias ocultas de TestStand no pueden colarse porque el paso es un
  proceso tras un contrato.
- **Extensible sin tocar el motor**: añadir un paso = añadir un nombre al
  despacho, no recompilar el secuenciador.
- **Base para paralelismo aislado**: un proceso/instancia por socket
  evita que DLLs compartidas serialicen (dolor de TestStand, post-MVP).

**Negativas:**

- Cada llamada gasta un stream HTTP/2 y un salto local → overhead. Aceptado:
  es despreciable frente al tiempo de un instrumento real (RNF-04).
- Requiere una pila gRPC que compile a WASM → ADR-0006.
- El contrato (`paso.proto`) se vuelve superficie pública crítica: hay que
  versionarlo y no romperlo (RNF-05, [contrato-grpc.md](../contrato-grpc.md)).

**Neutras:**

- El despacho por nombre es el **único** punto donde el nombre del cable se
  ata a código; un nombre desconocido es `error`, no pánico (RF-12).

## Alternativas descartadas

- **Adapters de vendor (estilo TestStand):** atan a un runtime y arrastran
  dependencias ocultas.
- **Pasos in-process (estilo OpenTAP/Python):** mono-lenguaje y sin
  aislamiento.

## Enlaces

- [ADR-0005](0005-motor-generico-dirigido-por-datos.md),
  [ADR-0006](0006-wasi-grpc-propio.md),
  [contrato-grpc.md](../contrato-grpc.md).