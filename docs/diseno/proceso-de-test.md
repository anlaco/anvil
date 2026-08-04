# Diseño: Proceso de test (process model)

> **Prioridad:** MVP-parcial. **Implementado (M5, ADR-0016)**: Sequential
> simple como envoltorio de secuencia. Parallel/Batch post-MVP. **No** se
> replica el process model de TestStand 1:1.

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

## MVP: Sequential simple + plug-ins (implementado, ADR-0016)

```
[identificar UUT] → [correr secuencia] → [notificar] → [loguear/reportar]
```

- El process model es un **YAML envoltorio** que se corre con
  `--process-model`:

  ```sh
  ./anvil --process-model pm.yaml secuencia_del_operador.yaml
  ```

  El PM es la raíz; la secuencia del operador se inyecta como subsecuencia
  usuario y se invoca con `secuencia_usuario: true` (reusa la maquinaria de
  sequence call de M4b, ADR-0010; ver ADR-0016). Ejemplo:
  `ejemplos/process_model_sequential.yaml`.
- Las operaciones comunes (identificar, notificar, reportar) son **pasos
  plug-in** que el motor corre alrededor de la secuencia del usuario, no
  un process model editable oculto.
- Extensión por **plug-ins/ResultSinks** (investigación §2, preferencia de
  foro NI: plug-in > tocar el modelo), no por customización del núcleo.
- Cada grupo/línea mantiene su propio PM como un YAML aparte y lo reutiliza
  con distintas secuencias sin tocarlas (patrón plug-in, no callback
  override).

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