# Diseño: UI vs. headless

> **Prioridad:** MVP-parcial. **Headless/CLI en el MVP**; Operator UI web +
> UIMsgs + editor visual son post-MVP.

Anvil nace **headless primero**: se corre con `wasmtime run anvil.wasm
secuencia.yaml`. Sin UI gráfica en v1. La UI llega después, cuando el
núcleo sea estable. Esto también evita el dolor de TestStand: una UI
acoplada al motor que se queda atrás (Sequence Editor dev vs. Operator
Interfaces prod desincronizados).

## MVP: headless/CLI

- El motor corre como un `.wasm` bajo wasmtime; la salida es el ResultSink
  de consola (ver [reportes.md](reportes.md)).
- Las "opciones" de ejecución (pause-on-fail, step, disable) son flags CLI o
  campos del YAML (ver [motor-de-ejecucion.md](motor-de-ejecucion.md)).
- Determinismo: sin UI, la ejecución es reproducible (RNF-03).

## Desacoplo motor ↔ UI: UIMsgs (post-MVP)

TestStand desacopla el motor de la UI de operador con *User Interface
Messages*: el motor postea mensajes (trace, estado, errores) que cualquier
Operator Interface consume; los no soportados se ignoran (investigación
§1.6). Anvil adopta la misma idea **post-MVP**:

- El motor emite eventos; una UI web los consume.
- Así una UI corre cualquier secuencia y viceversa, sin acoplar el motor a
  un toolkit gráfico (coherente con WASM, ADR-0001: el motor sigue siendo un
  `.wasm` sin toolkit).

## Operator UI web (post-MVP)

- UI web de operador (copiar de Litmus/OpenHTF, investigación §3/§6).
- **Roles**: admin / engineer / technician / operator con login separado
  del SO (estándar en ATE comercial: Astronics/Advantest, ProDSP —
  investigación §5, Could).
- Lee el estado del motor por los UIMsgs; no lo acopla.

## Editor visual (post-MVP) — con drag-and-drop e introspección de firma

Cuando Anvil tenga editor visual, el objetivo es:

1. **Drag-and-drop del archivo** del code module (`.vi`/`.dll`/`.py`/
   `.scilab`) sobre el editor.
2. El editor **auto-descubre y actualiza los parámetros y el valor de
   retorno** del paso a partir de la firma del módulo, como hace TestStand
   al añadir un code module.

Esto exige que un paso **exponga su firma** (parámetros: nombre, tipo,
dirección in/out; tipo de retorno). Hoy `paso.proto` solo describe *cómo
invocar* y *qué devuelve* a nivel de mensaje, no la firma tipada. Hay que
añadir un **mecanismo de introspección** (p. ej. un RPC `Describe` o un
sidecar de metadatos) — extensión futura del contrato, detallada en
[contrato-grpc.md](../contrato-grpc.md) y ligada al registro de pasos
([modelo-de-pasos.md](modelo-de-pasos.md)).

> **Tensión resuelta:** la introspección de firma vive en el **lado del
> ejecutor** (que provee el catálogo de pasos y sus firmas), no en el
> núcleo del motor (que sigue genérico, ADR-0005). El editor y el ejecutor
> hablan firmas; el motor sigue hablando solo `nombre`/`estado`.

## Por qué headless primero

- El núcleo (semántica, reintentos, contrato, ResultSinks) es lo que
  diferencia a Anvil; la UI no (Flojoy ya tiene editor visual AGPL,
  OpenTAP editor comercial — investigación §3). La UI no es la tesis
  (ver [vision.md](../vision.md)).
- Headless permite CI sin hardware (record/replay, ver
  [integracion-instrumentos.md](integracion-instrumentos.md)) y
  determinismo desde el día 1.

## Out-of-scope

- Editor visual en el MVP (es post-MVP, ligado a introspección de firma).
- Debugger visual completo.
- UI atada a un toolkit de escritorio (la UI es web, no nativa).