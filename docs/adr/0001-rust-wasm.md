# ADR-0001: Rust compilado a WASM

- **Estado:** Aceptada (decisión pre-existente, formalizada aquí)
- **Fecha:** pre-prototipo

## Contexto

Anvil debe correr en la planta: estaciones heterogéneas (Linux/Windows),
deployment sin instalador y, sobre todo, **aislar** el secuenciador de los
pasos que tocan hardware real. Un bug en un paso no debe tumbar el motor ni
corromper estado compartido. TestStand sufre lo opuesto: DLLs compartidas
serializan sockets, VIs no-reentrant degradan a secuencial en silencio, y
dependencias ocultas que van bien en Dev se rompen en runtime
([investigación](../investigacion/TestStand-y-competencia.md) §2).

El lenguaje de implementación del secuenciador no tiene por qué ser el de los
pasos: estos últimos se invocan por gRPC en cualquier lenguaje (ADR-0003). Lo
que necesitamos es un runtime **portátil, aislado y determinista** para el
host.

## Decisión

Implementar Anvil en **Rust compilado a WASM** (`wasm32-wasip2`, WASI
Preview 2) bajo **wasmtime**.

- `rust-toolchain.toml` fija `stable` + target `wasm32-wasip2`.
- Se corre con `wasmtime -S cli -S tcp=y -S inherit-network=y` (los flags de
  red no son opcionales: sin ellos el guest no toca la red).

## Consecuencias

**Positivas:**

- **Aislamiento real**: el secuenciador corre en un sandbox; el interior de
  cada paso es opaco al motor. Responde directamente al dolor de TestStand
  con paralelismo que no aísla.
- **Portabilidad**: un mismo `anvil.wasm` corre en cualquier SO con wasmtime.
  Deployment = copiar un `.wasm` + una secuencia YAML, sin instalador ni
  recompilar todo (frente a las builds de 30–70 min de TestStand).
- **Determinismo**: sin hilos con data races implícitos; base para
  reintentos reproducibles (RNF-03).

**Negativas:**

- `wasm32-wasip2` no soporta la pila gRPC habitual: `tonic`/`tokio` no
  compilan a WASM → obliga a una pila propia (ADR-0006).
- Ecosistema WASI aún en Preview: algunas APIs std limitadas.
- Sin codegen gRPC en wasi-grpc v0.1 → structs `prost` espejados a mano
  (`crates/modelo/src/proto.rs`).

**Neutras:**

- El coste de una llamada gRPC local es irrelevante frente al tiempo de un
  instrumento real (RNF-04), así que el overhead del sandbox no es cuello de
  botella.

## Alternativas descartadas

- **Rust nativo (sin WASM):** portable y rápido, pero sin aislamiento del
  sandbox; un paso pánico contaminaría el proceso.
- **Python (como OpenHTF/Litmus/pytestlab):** ecosistema de test maduro,
  pero mono-lenguaje y sin sandbox; pierde la tesis de aislamiento.
- **.NET (como OpenTAP):** corre in-process full-trust; no aísla.

## Enlaces

- [arquitectura.md](../arquitectura.md), [requisitos.md](../requisitos.md)
  (RNF-01, RNF-02).