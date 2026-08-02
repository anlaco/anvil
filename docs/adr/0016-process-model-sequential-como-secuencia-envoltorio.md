# ADR-0016: Process model Sequential como secuencia envoltorio YAML

- **Estado:** Aceptada
- **Fecha:** 2026-08-03 (M5)
- **Relaciona:** ADR-0005, ADR-0010, ADR-0011,
  [proceso-de-test.md](../diseno/proceso-de-test.md),
  [formato-de-secuencia.md](../diseno/formato-de-secuencia.md),
  [motor-de-ejecucion.md](../diseno/motor-de-ejecucion.md)

## Contexto

TestStand separa "el test" (la secuencia) de "cómo se corre en producción"
(identificar el UUT, notificar pass/fail, loguear/reportar): la misma
secuencia va de R&D a la fábrica cambiando sólo el *process model*. Anvil
**respeta la separación** y **no** hereda el modelo monolítico de TestStand
(frágil: un process model editable con callbacks overrideables que, al
tocarse, rompe todas las secuencias existentes — investigación §2).

El diseño (`proceso-de-test.md`) fija el MVP en **Sequential simple + plug-ins**:
`[identificar UUT] → [correr secuencia] → [notificar] → [loguear/reportar]`,
donde las operaciones comunes son **pasos plug-in** que el motor corre
**alrededor** de la secuencia del usuario. Faltaba decidir cómo se
materializa ese "alrededor" sin reintroducir la fragilidad.

## Decisión

El process model (PM) es **una secuencia YAML envoltorio**: una
`DefinicionSecuencia` más, cuyo `main` lleva un `sequence_call` a la
secuencia del usuario, con pasos plug-in `grpc` en `setup` (identificar
UUT) y `cleanup` (notificar/reportar). El PM canónico vive en
`process_models/sequential.yaml`.

El PM es genérico y no sabe qué secuencia va a correr — la ruta la da el
CLI dinámicamente. Convención: el PM autora el `sequence_call` con
`secuencia: secuencia_usuario` (un **nombre reservado**, no un path:
`es_path()` es falso). El cargador, en la nueva entrada
`cargar_programa_con_pm(ruta_pm, ruta_usuario)`:

1. Valida que la raíz del PM tenga **exactamente un** `sequence_call` a
   `secuencia_usuario` en `main`, y que `secuencia_usuario` no aparezca
   en `subsecuencias` de la raíz (reservado).
2. Carga la secuencia del usuario y la registra en `programa.archivos`
   bajo su path canónico (`normalizar(dir_de(ruta_usuario), ruta_usuario)`).
3. Procesa paths externos del PM (relativos a su dir) y del usuario
   (relativos al suyo) con el pipeline M4b (`procesar_secuencia`).
4. **Reescribe** el placeholder `secuencia_usuario` → clave canónica del
   usuario **después** de `procesar_secuencia` (si se hiciera antes, éste
   renormalizaría la clave relativa al directorio del PM y la rompería).
5. Visita el programa entero (`visitar`): ciclos, firma (`validar_call`) y
   lvalues.

El motor **no se toca** (ADR-0005): tras la reescritura el call es un path
normal que `ejecuta_sequence_call` resuelve en `programa.archivos` como
cualquier subsecuencia externa (ADR-0010). El motor no sabe que vive en un
PM; ve un `Programa` con una raíz (el PM) y un archivo externo (el
usuario). El CLI selecciona el PM con `--process-model <ruta>`; sin el
flag, la secuencia del usuario corre tal cual (sin envoltorio).

El PM canónico declara **sin `parametros`**, así exige que la raíz del
usuario **no declare `parameters`** (firma vacía == vacía, validado por
`validar_call`). El resultado del usuario queda anidado en `sub_pasos` y
su estado agregado se captura en una local del PM vía `asigna`.

## Por qué esta forma

- **Todo es datos** (ADR-0002): el PM es YAML, intercambiable sin
  recompilar. La misma secuencia corre en R&D (sin PM) y en fábrica (con
  PM) cambiando un flag. Es la buena idea de TestStand sin su modelo
  monolítico.
- **Reusa M4b entero** (ADR-0010): `sequence_call`, `parameters`
  by-reference, resolución de paths, anidación de `ResultadoSecuencia`,
  detección de ciclos, validación de firma/lvalues. Cero código nuevo en
  el motor ni en `paso.proto` (RNF-05).
- **Plug-ins son pasos reales** (`grpc` despachados por el ejecutor), no
  callbacks motor-side: el PM es editable sin tocar el núcleo.

## Alternativas rechazadas

- **Orquestador en código** (lógica del PM en Rust en el host/motor):
  personalizar el PM para cada fábrica exigiría recompilar; rompe "todo es
  datos" y duplica la semántica de ejecución. Rechazada.
- **Fallback en el motor** (nombre reservado `secuencia_usuario`
  resuelto con un fallback en `ejecuta_sequence_call`: si no está en
  `subsecuencias` inline, buscar en `programa.archivos` por nombre): añade
  una segunda rama de resolución por nombre, introduce ambigüedad y hace
  que el motor sepa de un concern del cargador/CLI ("process model").
  Viola el espíritu de ADR-0005. La reescritura en el cargador es más
  limpia: el motor sigue con una sola resolución por path/nombre.
- **Hooks de flujo en `ResultSink`** (pre-Main/post-Main): `ResultSink`
  es observer (no controla flujo); añadirle control rompería la separación
  y el determinismo. Rechazada.

## Recortes MVP-parcial

- **PM canónico sin `parametros` al usuario.** Sólo envuelve (identificar,
  correr, notificar); no pasa datos por-reference a la secuencia del
  usuario. Los resultados fluyen al PM por el `ResultadoSecuencia` anidado
  y `asigna`. Un PM custom con `parametros` emparejados (PM no genérico) es
  post-MVP (librería de PMs con discovery).
- **Sin plug-in dinámico de PM.** El PM se selecciona con `--process-model`
  apuntando a un YAML en disco. Sin registro ni discovery de PMs.
- **Un solo PM canónico** (`sequential.yaml`); la librería de PMs es
  post-MVP.

## Consecuencias

- ADR-0005 se **refuerza**: el motor sigue genérico y dirigido por datos;
  la noción de "process model" vive en el cargador y el YAML, no en el
  núcleo.
- ADR-0010 se **extiende**: el `sequence_call` ahora también orquesta la
  envoltura de producción, no sólo subsecuencias de usuario.
- `paso.proto` no cambia (RNF-05): el PM es motor-side como `statement` y
  `sequence_call`; los plug-ins son `grpc` normales.
- El reporte anida la secuencia del usuario bajo el paso
  `correr_secuencia_usuario` (`sub_pasos`, M4b); el reporte textual, JSON
  y CSV ya lo soportan.
- El CLI gana `--process-model` (RF-40, ver `ui-vs-headless.md`).