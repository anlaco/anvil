# Diseño: Proceso de test (process model)

> **Prioridad:** MVP-parcial. **Implementado en M5** (Sequential simple,
> PM envoltorio YAML, plug-ins `grpc`, sin callbacks; ver
> [ADR-0016](../adr/0016-process-model-sequential-como-secuencia-envoltorio.md)).
> Parallel/Batch post-MVP. **No** se replica el process model de TestStand 1:1.

La idea **especial** de TestStand: separar "el test" (la secuencia) de
"cómo se corre en producción" (identificar UUT, notificar pass/fail,
loguear, reportar) — la misma secuencia va de R&D a la fábrica cambiando
solo el *process model* (investigación §1.2). Anvil **respeta la separación**
y **no** hereda el modelo monolítico de TestStand (frágil: tocar callbacks
rompe todas las secuencias existentes, investigación §2).

## Qué se respeta

- Una secuencia describe **qué tests** correr; el *process model* describe
  **cómo** se corre en producción (setup de la línea, identificación del
  UUT, notificación, reporte final).
- La misma secuencia puede correrse con distintos process models (R&D vs.
  fábrica) y un process model puede servir a varias secuencias.
- Frase rectora de NI: *"the test process can change but the tests executed
  remain the same."*

## Qué NO se replica 1:1

- El process model **no** es una secuencia editable con callbacks y entry
  points como en TestStand (complejo y frágil).
- MVP = **Sequential simple**: un UUT, una secuencia, sin maquinaria de
  callbacks.

## MVP: Sequential simple + plug-ins (implementado en M5)

```
[identificar UUT] → [correr secuencia] → [notificar] → [loguear/reportar]
```

- Las operaciones comunes (identificar, notificar, reportar) son **pasos
  plug-in** que el motor corre alrededor de la secuencia del usuario, no
  un process model editable oculto.
- Extensión por **plug-ins/ResultSinks** (investigación §2, preferencia de
  foro NI: plug-in > tocar el modelo), no por customización del núcleo.

### Cómo se materializa en Anvil (ADR-0016)

El PM es **una secuencia YAML envoltorio** (`process_models/sequential.yaml`)
cuyo `main` lleva un `sequence_call` a la secuencia del usuario (nombre
reservado `secuencia_usuario`, que el cargador reescribe al path de la
secuencia pasada por `--process-model`), con `identificar_uut` en `setup`
y `notificar_resultado` en `cleanup`. El motor **no se toca** (ADR-0005):
ve un `Programa` con raíz = PM y un archivo externo = secuencia del
usuario, y lo orquesta como cualquier `sequence_call` (ADR-0010). El
resultado del usuario queda anidado en `sub_pasos`. Sin `--process-model`,
la secuencia corre tal cual (R&D). Así la misma secuencia va de R&D a
fábrica cambiando un flag, sin recompilar y sin callbacks frágiles.

## Post-MVP: paralelismo con cancelación jerárquica

- **Parallel**: varios UUTs a la vez en fixtures independientes.
- **Batch**: varios UUTs en el mismo fixture.
- Ambos con **cancelación jerárquica** (un `CancellationToken` que aborta en
  cascada, estilo OpenTAP TapThreads — investigación §6).
- **Crítico:** el paralelismo no puede romper el **Cleanup garantizado**: si
  una rama se cancela, su Cleanup corre. Es la diferencia con el
  paralelismo de TestStand que "no aísla" (DLLs compartidas, sockets en
  conflicto — investigación §2).

## Out-of-scope

- Callbacks overrideables desde la secuencia cliente (mecánica de
  TestStand).
- Configuration entry points auto-poblados como menús.
- Batch/Parallel con sincronización implícita opaca (las opciones de sync
  del Batch aparecen en la UI del Parallel en TestStand y **no hacen nada**
  — investigación §2). Un solo modelo honesto primero.