# ADR-0006: Pila gRPC propia (wasi-grpc)

- **Estado:** Aceptada (decisión pre-existente, formalizada aquí)
- **Fecha:** pre-prototipo

## Contexto

ADR-0001 fija el runtime en `wasm32-wasip2` (WASI Preview 2) bajo wasmtime, y
ADR-0003 necesita gRPC para invocar los pasos. El problema: la pila gRPC
estándar del ecosistema Rust (`tonic` + `tokio`) **no compila a WASM**:
dependen de I/O y runtime que WASI Preview 2 no ofrece. Sin gRPC, la tesis
de "pasos por gRPC en cualquier lenguaje" se cae.

## Decisión

Construir una **pila gRPC propia**, `wasi-grpc`: gRPC (HTTP/2 + protobuf)
sobre sockets WASI nativos, sin `tokio`/`tonic`.

- Vive en un **repo aparte** (`../wasi-grpc`), licencia **Apache-2.0** (se
  linka en código ajeno, ADR-0004).
- Se referencia por **ruta** en `Cargo.toml` porque Anvil la dogfoodea: los
  dos repos se desarrollan a la vez y un cambio en la pila se prueba aquí
  al momento. Cuando se estabilice y publique, pasará a una versión.
- Anvil es su primer consumidor.

## Consecuencias

**Positivas:**

- Desbloquea ADR-0003 en WASM sin abandonar el sandbox (ADR-0001).
- La pila es Apache → contagia a nadie; puede adoptarse como referencia
  fuera de Anvil.
- Dogfoodear la pila aquí la endurece con uso real antes de publicarla.

**Negativas:**

- **Coste de mantener una pila propia**: gRPC/HTTP/2 no es trivial; es
  deuda de ingeniería sostenida.
- wasi-grpc v0.1 **no trae codegen** → los structs `prost` de
  `crates/modelo/src/proto.rs` se espejan a mano desde `paso.proto`. Si se
  toca uno, hay que tocar el otro.
- Atado a un repo privado por ruta hoy: hasta que se publique, un
  colaborador externo necesita acceso a `wasi-grpc` para construir Anvil.

**Neutras:**

- `wasi-grpc` **no es de Anvil**: se menciona en
  [arquitectura.md](../arquitectura.md) pero no se documenta como propio.

## Alternativas descartadas

- **`tonic`/`tokio`:** no compilan a `wasm32-wasip2`. Descartadas por
  imposibilidad técnica, no por preferencia.
- **Esperar a una pila gRPC WASI madura de terceros:** habría bloqueado la
  tesis del producto de forma indefinida.

## Enlaces

- [ADR-0001](0001-rust-wasm.md), [ADR-0003](0003-pasos-por-grpc-por-nombre.md),
  [contrato-grpc.md](../contrato-grpc.md), [arquitectura.md](../arquitectura.md).