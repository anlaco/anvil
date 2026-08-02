# ADR-0010: Sequence call lo orquesta el motor; el cargador resuelve paths y valida; `paso.proto` no cambia

- **Estado:** Aceptada
- **Fecha:** 2026-08-02 (M4b)
- **Relaciona:** ADR-0005, ADR-0008, ADR-0009,
  [contrato-grpc.md](../contrato-grpc.md),
  [variables-y-alcances.md](../diseno/variables-y-alcances.md),
  [modelo-de-pasos.md](../diseno/modelo-de-pasos.md),
  [formato-de-secuencia.md](../diseno/formato-de-secuencia.md)

## Contexto

M3 (ADR-0008) y M4-núcleo (ADR-0009) sentaron el patrón: el motor evalúa
reglas declaradas como datos (límites, expresiones, precondiciones,
asignaciones) contra su entorno, **sin tocar el contrato `paso.proto`**. El
`statement` (RF-27) ya es un paso **local** (motor-side, sin gRPC).

Queda el último built-in de RF-27: **sequence call** — invocar otra secuencia
como un paso, con **Parameters de entrada/salida** reales (RF-31) y
**anidamiento del `ResultadoSecuencia`**. TestStand resuelve esto con
subsecuencias llamables; el roadmap (M4b) pide un modelo de subsecuencias
declarables **inline** (cuando sólo las usa una secuencia) o **en archivo
aparte** (para reutilizar).

Las preguntas: ¿quién conoce el sistema de ficheros para resolver los
paths?, ¿quién valida las subsecuencias y detecta ciclos?, ¿cómo se cablean
los Parameters de entrada/salida respetando la regla "sólo se muta Locals"
(ADR-0009) y sin tocar `paso.proto`?

## Decisión

El **sequence call es motor-side**, como el `statement`: el motor orquesta
la subsecuencia contra su propio `EntornoMotor`, sin gRPC. El contrato
`paso.proto` **no cambia** (RNF-05). Concretamente:

### Resolución y validación: el cargador, no el motor

- Un nuevo **`Programa`** (`crates/modelo`) agrupa la secuencia raíz + las
  subsecuencias de **archivos externos** (keyed por path normalizado).
- El **cargador** (`cargar_programa_de_archivo`) resuelve cada
  `sequence_call`:
  - **Por nombre** → subsecuencia **inline** del mismo archivo (campo
    `subsecuencias:`). Las inline son **privadas del archivo**.
  - **Por path** → secuencia **raíz** del archivo externo. El path se
    reescribe a su clave canónica en cada `DefinicionPaso.secuencia`, así el
    motor lo resuelve con `programa.archivos[clave]` **sin abrir ficheros**
    (ADR-0005: el motor no conoce `std::fs`).
  - Valida que cada argumento `locals.X` esté declarado en `locals` de la
    secuencia contenedora y que la **firma** encaje (claves de `parametros`
    == `parameters` de la subsecuencia). **Detecta ciclos** (DFS sobre el
    grafo de llamadas). Todo fail-fast al cargar.

### Parameters de entrada/salida **by reference** (como TestStand)

- El sequence call mapea cada `Parameter` de la subsecuencia a una
  **variable local del padre** (`locals.X`). Entrada: copia `locals.X` →
  `parameters.P`. La subsecuencia **escribe en `parameters.P`**. Salida:
  copia `parameters.P` (final) → `locals.X`. Un mismo `Parameter` es entrada
  y salida, como TestStand by-reference (default). Es el canal caller↔callee
  que reemplaza a FileGlobals/StationGlobals para devolver valores.

### Relajación acotada de "sólo se muta Locals" (ADR-0009)

La regla de M4-núcleo prohibía escribir fuera de `locals` para mantener el
**paso gRPC** aislado. Ese principio **se mantiene**: el paso gRPC sigue sin
tocar variables. La subsecuencia, en cambio, es lógica motor-side (como
`statement`/`asigna`, que ya mutan el entorno), y escribir en sus propios
`parameters` es su **contrato de retorno** con el llamador. Así:

- `escribe(Scope::Parameters)` se permite **sólo si el entorno pertenece a
  una subsecuencia** (flag `parameters_mutables`). La **raíz** no puede
  escribir en sus `parameters` (no tiene a quién devolver); sus `parameters`
  son de sólo lectura. `escribe(Scope::FileGlobals)` sigue prohibido siempre.

### Anidamiento y lifecycle

- El `ResultadoStep` del call lleva el `estado` agregado de la subsecuencia y
  sus `sub_pasos` anidados. La consola los indenta (extensión aditiva de
  RNF-08); JSON los anida; CSV los aplanea como `padre/hijo` sin columnas
  nuevas.
- La subsecuencia se ejecuta con `es_raiz=false`: **no** dispara
  `on_inicio/on_fin_secuencia` (los sinks de formato no duplican reporte),
  pero **sí** los hooks de paso (un futuro sink de streaming verá la
  subsecuencia en vivo).
- Profundidad máxima (64) como red de seguridad ante un ciclo que escapara al
  cargador.

### Testabilidad: `InvocaPasos`

La lógica del motor vive en métodos de `Motor`, que requiere un `Cliente`
gRPC (no construible sin conexión). Para probar el flujo completo —incluido
sequence call— sin red, se extrae un trait **`InvocaPasos`** que `Motor`
implementa (gRPC) y un mock implementa en tests. Es la materialización de
"motor genérico" (ADR-0005): la lógica de la secuencia no sabe si el paso
corre por gRPC o por un sustituto.

## Recortes MVP-parcial (señalados)

- Los argumentos son sólo `locals.X` (by-reference). **By-value** (entrada
  sin retorno, para aislar) y **by-reference transitivo** (pasar
  `parameters.X`/`file_globals.X` del padre) quedan post-MVP.
- Las inline son invocables por nombre desde la secuencia que las declara
  (la raíz o la secuencia del archivo externo); **no** se llaman entre sí.
  Para compartir entre subsecuencias, se saca a archivo externo (por path).
- Las subsecuencias no pueden declarar `reintentos > 1` ni `limite` (un
  sequence call no mide; su estado es el agregado de la subsecuencia).
- El sidecar de límites (property loader) aplica a la **raíz**; aplicar a
  subsecuencias externas es post-MVP.

## Consecuencias

- `paso.proto`/`proto.rs` sin cambios: el ejecutor gRPC no sabe que vive en
  una subsecuencia (RNF-05).
- El motor no gana dependencia de `cargador` para resolver paths (los recibe
  ya resueltos en el `Programa`); sí la mantiene como dependencia del crate
  por el bin y por `es_path`.
- "Sólo se muta Locals" se refina: el paso gRPC aislado (intacto); la
  subsecuencia muta además sus `parameters` (retorno).
- `ResultadoStep` gana `sub_pasos` (campo opcional, no viaja en el cable);
  el reporte congelado (RNF-08) crece de forma aditiva (indentación de
  sub-pasos), los pasos sin sub_pasos producen la misma línea de siempre.