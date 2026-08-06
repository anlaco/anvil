# Documentación de Anvil

Anvil es un secuenciador de test de producción **open-source** que compite con
NI TestStand: corre secuencias de pasos contra equipo real (instrumentos),
reintenta los que fallan y reporta. La secuencia es **datos, no código**, y
cada paso se invoca **por gRPC por su nombre** — nunca con una llamada
directa —, lo que aísla los pasos entre sí y deja la puerta abierta a
escribirlos en cualquier lenguaje.

> Esta es la documentación **pre-desarrollo**: la fase de planificación que
> fija *qué* es Anvil, *por qué* y *cómo* antes de construir el producto.
> Las docs operativas y de usuario vienen después, cuando haya producto.

## Para empezar ya

**[guia-inicio-rapido.md](guia-inicio-rapido.md)** — de cero a correr una
secuencia con subsecuencias en 5 minutos (build, tests, smoke end-to-end).

## Cómo leer esta documentación

Un ingeniero de test que viene de TestStand puede entender qué es Anvil, qué
hace, qué no hace y por qué leyendo solo tres archivos, en este orden:

1. **[vision.md](vision.md)** — qué es Anvil, contra qué compite, para quién,
   qué entra en v1 y qué no.
2. **[requisitos.md](requisitos.md)** — qué debe hacer, verificable y trazable
   al contrato (`crates/modelo/paso.proto`).
3. **[arquitectura.md](arquitectura.md)** — cómo se construye (C4 niveles 1–3).

El resto profundiza por área. Si un término no queda claro, está en el
[glosario](glosario.md).

Lo que se ha encontrado al usar el producto de verdad está en
**[qa/informe-beta-2026-08.md](qa/informe-beta-2026-08.md)**, con las
reproducciones ejecutables en [`qa/regresion/`](qa/regresion/).

## Mapa del árbol

```
docs/
├─ README.md                  este índice
├─ glosario.md                vocabulario del dominio (TestStand + Anvil)
├─ vision.md                   visión, propuesta de valor, competitivo, alcance MVP
├─ requisitos.md              requisitos funcionales y no funcionales (SRS ligero)
├─ arquitectura.md            arquitectura C4 niveles 1–3
├─ contrato-grpc.md           semántica del contrato del paso sobre paso.proto
├─ licencia.md                estrategia de licencia dual AGPL / Apache
├─ roadmap.md                 hitos M0 → M4+ con MVP vs. post-MVP
├─ adr/                       decisiones ya tomadas (inmutables, plantilla Nygard)
│  ├─ 0001-rust-wasm.md
│  ├─ 0002-secuencia-como-datos.md
│  ├─ 0003-pasos-por-grpc-por-nombre.md
│  ├─ 0004-licencia-dual-agpl-apache.md
│  ├─ 0005-motor-generico-dirigido-por-datos.md
│  └─ 0006-wasi-grpc-propio.md
│  (0007-0012 en la misma carpeta; 0013 reemplaza 0012 en el cargador y el routing)
├─ planes/                    planes de hito (m4-nucleo, m4b, m5-ext)
├─ qa/                        informes de campo y regresión de defectos
│  ├─ informe-beta-2026-08.md  hallazgos de la 1ª beta externa (4 defectos + diagnóstico)
│  └─ regresion/               reproducciones mínimas ejecutables (`run.sh`)
└─ diseno/                    diseño del dominio, un doc por área funcional
   ├─ motor-de-ejecucion.md
   ├─ limites-y-estados.md
   ├─ modelo-de-pasos.md
   ├─ formato-de-secuencia.md
   ├─ reportes.md
   ├─ variables-y-alcances.md
   ├─ integracion-instrumentos.md
   ├─ motor-de-expresiones.md
   ├─ proceso-de-test.md
   └─ ui-vs-headless.md
```

En la raíz del repo, los archivos de comunidad:
[`CONTRIBUTING.md`](../CONTRIBUTING.md),
[`GOVERNANCE.md`](../GOVERNANCE.md),
[`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md),
[`SECURITY.md`](../SECURITY.md).

## Convenciones

- **Markdown en español**, consistente con el `README.md` del repo.
- Cada requisito y decisión se **ancla a su fuente real** del repo
  (`crates/modelo/paso.proto`, `crates/modelo/src/lib.rs`, `crates/motor/src/lib.rs`…).
- Los ADR son **inmutables**: si una decisión cambia, se añade un ADR nuevo
  que la reemplaza (Estado: *Superseded por ADR-00NN*).
- Cada área de diseño marca su alcance: **MVP** / **MVP-parcial** /
  **post-MVP** / **out-of-scope**.
- Los datos y citas sobre TestStand y competidores se referencian a
  [`investigacion/TestStand-y-competencia.md`](investigacion/TestStand-y-competencia.md),
  que tiene las fuentes; no se reponen las URLs en cada doc.

## Estado del producto hoy

Existe un **prototipo** (motor demo + contrato gRPC en
`crates/modelo/paso.proto`) y la **especificación viva** (semántica
Setup→Main→Cleanup, reintentos por paso, estados paso/fallo/error, agregado
error>fallo, reporte textual congelado). Lo que ya está decidido se
**formaliza** en estos docs; lo que no, se **propone** como decisión de diseño
marcada como propuesta.