# Gobernanza

Cómo se toman las decisiones en Anvil, quién tiene derecho de merge y qué
pasa si el proyecto cambia de manos. Ligero a propósito; se endurecerá
cuando haya más de un mantenedor.

## Modelo: BDFL (hoy), camino a equipo mantenedor

Anvil es un proyecto de [ANLACO](https://github.com/anlaco) iniciado por su
autor principal. Hoy opera como **BDFL** (*Benevolent Dictator For Life*):
el autor mantiene la última palabra sobre dirección y diseño.

La meta es migrar a un **equipo mantenedor** cuando la comunidad crezca: un
grupo pequeño con derechos de merge y voto en decisiones de diseño, con el
BDFL como desempate. La sucesión del BDFL se decide por consenso del equipo
mantenedor en ese momento.

## Derechos

| Rol | Qué puede |
|---|---|
| **Contribuidor** | Abrir issues y PRs (firmados con DCO). |
| **Mantenedor** (futuro) | Revisar y aprobar PRs; merge a `main`. |
| **BDFL** | Decisión final; creación/destitución de mantenedores; ADRs fundacionales. |

Hoy los merges los decide el BDFL.

## Decisiones de diseño: ADRs

Los cambios arquitectónicos se documentan en **ADRs inmutables** (ver
[`docs/adr/`](docs/adr/)). Un ADR nuevo reemplaza al anterior (Estado:
*Superseded por ADR-00NN*); nunca se edita el histórico.

- Cambios al **contrato** (`paso.proto`) o a la **semántica de ejecución**
  exigen un ADR (y, cuando se active, un RFC — ver
  [`docs/roadmap.md`](docs/roadmap.md), diferido).
- Decisiones menores viven en `docs/diseno/` marcadas como *propuesta*.

## Cambios al contrato

`paso.proto` es superficie pública crítica. Política: cambios **aditivos**
permitidos; cambios **rupturistas** exigen ADR y, idealmente, un proceso
RFC. Ver [`docs/contrato-grpc.md`](docs/contrato-grpc.md).

## Conducta

Toda participación se rige por el [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## Abandono

Si el BDFL deja el proyecto sin sucesor designado, el equipo mantenedor
(uno existente o uno formado por los contribuidores más activos) asume la
dirección. La licencia AGPL-3.0-or-later permite un fork si la dirección se
estanca.