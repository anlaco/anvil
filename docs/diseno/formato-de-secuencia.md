# Diseño: Formato de secuencia

> **Prioridad:** MVP. El cargador YAML se implementó en M1 (RF-20); los
> límites por paso (`limite`) llegan en M3 (RF-29). El schema sigue siendo un
> subconjunto estricto que crece de forma deliberada.

La secuencia es **datos** (ADR-0002). El cargador (`crates/cargador`) lee el
YAML y lo traduce a `DefinicionSecuencia` sin tocar el motor (ADR-0005); el
binario `crates/motor/src/bin/basica_datos.rs` es la misma secuencia expresada
en Rust, para referencia.

## Estado actual

Hoy la secuencia "basica" se construye en Rust:

```rust
DefinicionSecuencia {
    nombre: "basica_datos".into(),
    pasos_setup: vec![DefinicionPaso::nuevo("conectar_equipo", 3)],
    pasos_main: vec![
        DefinicionPaso::nuevo("medir_voltaje", 1),
        DefinicionPaso::nuevo("verificar_led", 1),
    ],
    pasos_cleanup: vec![DefinicionPaso::nuevo("desconectar_equipo", 1)],
}
```

El objetivo: expresar lo mismo en YAML, cargarlo y traducirlo a
`DefinicionSecuencia` sin tocar el motor (ADR-0005).

## Schema YAML propuesto

```yaml
# Secuencia de ejemplo "basica"
name: basica
setup:
  - name: conectar_equipo
    retries: 3
main:
  - name: medir_voltaje
    retries: 1
    limit:
      type: range
      min: 4.5
      max: 5.5
  - name: verificar_led
    retries: 1
cleanup:
  - name: desconectar_equipo
    retries: 1
```

Reglas:

- Tres secciones: `setup`, `main`, `cleanup` (todas opcionales salvo
  `main`, que es la medición). `cleanup` corre siempre (semántica del
  motor, no negociable).
- Cada paso tiene `nombre` (obligatorio) y `reintentos` (entero ≥ 1;
  defecto 1).
- Campos opcionales por paso: `limite` (desde M3, ver
  [limites-y-estados.md](limites-y-estados.md) — `tipo: rango|comparacion` con
  `min`/`max` o `op`/`esperado`), `disable` y `pause_on_fail`
  (M4, RF-34, ver [motor-de-ejecucion.md](motor-de-ejecucion.md)),
  `precondicion` (M4, RF-33, ver [motor-de-expresiones.md](motor-de-expresiones.md)),
  `asigna` (M4, RF-31, vuelca `resultado.*` a `Locals` tras el paso — **no**
  si el paso dio `error`, y desde los tres campos de `resultado`
  —`estado`, `mensaje`, `valor_medido`; ADR-0019 Regla 2— más
  `resultado.salidas.<nombre>` desde ADR-0020),
  `tipo`/`statement` (M4, RF-27: `tipo: grpc|statement`, por defecto `grpc`;
  `statement` trae las sentencias a ejecutar si el paso es local, sin gRPC), y
  desde **ADR-0020** `parametros` en un paso `grpc` (ver más abajo), y
  desde **M4b** `tipo: sequence_call` con `secuencia` (nombre de subsecuencia
  inline o path relativo a un archivo externo) y `parametros` (mapa
  `parameter -> "locals.X"`, by-reference — **ojo: el mismo nombre, otra
  cosa**; ver más abajo). Un `sequence_call` no admite
  `reintentos > 1` ni `limite` (no mide; su estado es el agregado de la
  subsecuencia), y por el mismo motivo su `asigna` **no puede leer
  `resultado.valor_medido`**: ese campo vale siempre `nothing` y borraba el
  destino en silencio (issue anlaco/Anvil-Test#20). De su `resultado` hay
  `estado` y `mensaje`; para devolver un valor medido, `parameters`.
  Desde **ADR-0018**, `tipo: pass_fail` con `condicion` (una
  expresión booleana que evalúa el motor: `true` → `paso`, `false` → `fallo`)
  — el veredicto **compuesto** sobre variables ya pobladas. Un `pass_fail` no
  admite `reintentos > 1`, `asigna`, `limite` ni `ejecutor`. Un `statement`
  tampoco admite `asigna`, por el mismo motivo: no produce `resultado.*` que
  volcar (ADR-0019, regla de detección).
- Variables: `locals`, `parameters`, `file_globals` a nivel de secuencia
  (M4, RF-31, ver [variables-y-alcances.md](variables-y-alcances.md)). El tipo
  de cada variable se infiere del escalar YAML (`true`→bool, `4.5`→número,
  `"A"`→texto). Los tres son **estrictos, y se comprueban al cargar** (no en
  runtime): leer un nombre no declarado es error de carga en cualquier
  expresión (issue anlaco/Anvil-Test#19), y también lo es escribir donde no se
  puede (issue #17) — `file_globals` es de sólo lectura siempre, y
  `parameters` sólo se escribe **desde una subsecuencia**, para devolver el
  valor al llamador. Los tipos, en cambio, no se comprueban al cargar: no son
  decidibles sin evaluar (ADR-0019, Regla 2). Desde **M4b**, `subsecuencias:` a nivel de secuencia declara
  subsecuencias **inline** (mapa `nombre -> secuencia`), invocables por
  nombre; el `nombre:` de una inline es opcional (cae al de su clave).
- Desde **M5-ext.1** (RF-36.3, ver [executores-lenguaje.md](executores-lenguaje.md)):
  `ejecutores:` a nivel de secuencia declara la **tabla de ejecutores** y un
  paso `grpc` puede declarar `ejecutor: <nombre>` (si se omite, va al
  embebido). Cada ejecutor tiene `nombre` y `tipo`:
  - `tipo: embebido` — el ejecutor WASM de serie, en loopback. Default. Sin
    campos adicionales. El puerto lo elige el host (efímero por proceso, o el
    de `--port`); 9100 es el default del guest ejecutor suelto.
  - `tipo: wasm` — componente `.wasm` propio cargado por el **host** por
    path (M5-ext.2, ADR-0014/0015; implementado). Campo `path` (relativo
    al YAML, debe existir). El host spawnea el puente `anvil-puente-wasm`,
    que carga el componente (interfaz WIT `anvil:paso`, función `run`) y lo
    expone al motor como `grpc` (override sintético).
  - `tipo: grpc` — ejecutor de lenguaje distribuido (p. ej. Python). Campos
    `host`/`puerto` (obligatorios). IPs no-loopback **sólo si se declaran**
    (relajación acotada del loopback, ADR-0011).
  
  La tabla se declara **una sola vez, en la secuencia raíz** — la que se pasa
  a `anvil`; con `--process-model`, también en el process model. Una
  subsecuencia (externa o inline) no declara los suyos: los referencia por
  nombre con `ejecutor:`, y declarar ahí un `ejecutores:` es error de carga.
  Antes se descartaba en silencio, incluso cuando contradecía a la de la raíz
  (issue anlaco/Anvil-Test#21).

  El nombre `__anvil_embebido__` está reservado (lo usa el motor); el
  cargador lo rechaza. `ejecutor:` en un paso `statement`/`sequence_call`
  es error (sólo aplica a `grpc`). Override por CLI:
  `--ejecutor nombre=host:puerto` (re-apunta o convierte un ejecutor sin
  tocar el YAML, patrón `--limits`).

### Subsecuencias: inline o por path (M4b)

Una subsecuencia se declara de dos formas:

- **Inline** bajo `subsecuencias:`, invocada por **nombre**. Privada del
  archivo. Útil cuando sólo la usa esa secuencia.
- **En archivo aparte**, invocada por **path relativo**
  (`secuencia: ./medir_fuentes.yaml`). Reutilizable desde varias secuencias.

Convención para distinguir nombre vs path en `secuencia`: si contiene `/` o
termina en `.yaml`/`.yml` → path (relativo al directorio del archivo que lo
contiene); si no → nombre (inline). El cargador resuelve los paths, valida
lvalues y firma, y detecta ciclos al cargar (fail-fast); el motor no abre
ficheros. Ver ADR-0010.
- El subconjunto es **estricto** (`deny_unknown_fields`): un campo no reconocido
  falla la carga en vez de ignorarse en silencio. `precondicion`/`asigna`/
  `statement` se **parsean a AST al cargar** (fail-fast): un error de sintaxis
  se reporta como error de validación con el nombre del paso (ADR-0009).


## `parametros` en un paso `grpc` (ADR-0020)

Un paso puede recibir valores desde la secuencia, en vez de llevarlos grabados
dentro:

```yaml
- name: medir_voltaje
  retries: 1
  inputs:
    canal: 2                      # número
    etiqueta: "banco-3"           # texto
    promediar: true               # booleano
    muestras: '${locals.n_muestras}'   # expresión, la evalúa el motor
  limit: { type: range, min: 4.5, max: 5.5 }
  assign:
    temp: result.outputs.temperatura
```

- **El tipo es el del escalar YAML**, y es el que viaja por el cable: `canal:
  2` es un número y `canal: "2"` es texto. No se convierten entre sí.
- **Las expresiones van entre `${...}`** y las evalúa el **motor** antes de
  llamar (ADR-0009): el paso no ve `locals`, se le pasan valores.
- Una expresión que falla deja el paso en **`error`** y el ejecutor no llega a
  invocarse. **Nunca hay valor por defecto**: medir con un parámetro inventado
  da un número que parece bueno y no lo es.
- Un `parametros:` que no sea un mapa de escalares es **error de carga**, no
  de ejecución: es decidible sin banco.

### El mismo nombre significa dos cosas, y por eso hay una red

`parametros:` ya existía en un `tipo: sequence_call`, donde es **by-reference**
(ADR-0010): `{ canal: locals.canal }` es una *referencia* a esa variable, y la
subsecuencia puede escribirla de vuelta. En un paso `grpc` es **by-value**: se
envía una copia del valor y no vuelve nada por ahí (lo que vuelve son las
`salidas`).

Los dos no pueden coincidir en el mismo paso, así que no hay ambigüedad real —
pero copiar un bloque de un sitio al otro sí cambiaría el significado en
silencio. Por eso **esto no carga**:

```yaml
- name: medir_voltaje
  inputs: { canal: locals.canal }   # error de carga
```

> el parámetro 'canal' del paso 'medir_voltaje' vale 'locals.canal', que
> viajaría como el texto literal "locals.canal" y no como el valor de esa
> variable. Si querías la variable, escríbela como '${locals.canal}'.

En un `statement` o un `pass_fail`, `parametros` no significa nada y se
rechaza.

## Salidas: `resultado.salidas.<nombre>`

Un paso puede devolver valores con nombre además de la medida. No participan
en el veredicto —el motor sigue evaluando el `limite` contra `valor_medido`
(ADR-0008)— y se leen desde `asigna`:

```yaml
assign:
  temp: result.outputs.temperatura
```

**No es validable al cargar**: el cargador no sabe qué devuelve un paso hasta
que corre, así que un nombre equivocado es `error` de **ejecución**, no de
carga. Es la única excepción a la regla de detección de ADR-0019 en el
formato, y lo que le devolvería este terreno a `--validate` es la
introspección de firma ([issue #45](https://github.com/anlaco/anvil/issues/45)).

Sin `inout`: entra por `parametros`, sale por `salidas`, y no hay tercer
camino. Un `sequence_call` sí pasa valores by-reference, y puede porque lo
orquesta el motor contra su propio entorno; un paso gRPC no.

## Cargador

- **Validación de schema** al cargar (campos obligatorios, tipos, `reintentos ≥ 1`,
  coherencia `tipo` ↔ `statement`: un `statement` sin `statement` o un `grpc`
  con `statement` son error).
- El destino de `asigna` y los lvalues de `statement` deben estar declarados
  en `locals`/`parameters` de su secuencia, y `asigna` no puede nombrar un
  `parameter` (DEF-3 del informe de beta: sin esto, un destino mal escrito o
  el nombre de un `parameter` creaba una `Local` nueva en silencio en vez de
  fallar). Ver [variables-y-alcances.md](variables-y-alcances.md).
- Errores de schema → la secuencia no carga (fail-fast), no se ejecuta a
  medias.
- El cargador produce `DefinicionSecuencia`; el motor la recorre (ADR-0005).

## Process model (M5, ADR-0016)

Un **process model** (PM) es una secuencia YAML envoltorio: una
`DefinicionSecuencia` más, cuyo `main` lleva un `sequence_call` a la
secuencia del usuario. El PM canónico es `process_models/sequential.yaml`
(`identificar_uut` en `setup`, `sequence_call` al usuario en `main`,
`notificar_resultado` en `cleanup`).

Convención: el PM autora el call con `secuencia: secuencia_usuario` (un
**nombre reservado**, no un path). El cargador, en
`cargar_programa_con_pm(ruta_pm, ruta_usuario)`, lo reescribe al path
canónico de la secuencia del usuario y la registra en `programa.archivos`.
**No extiende el schema**: un PM es un YAML con `setup`/`main`/`cleanup`/
`subsecuencias` como cualquier secuencia. El motor no se toca (ADR-0005);
el CLI lo selecciona con `--process-model <ruta>`.

El PM canónico declara sin `parametros`, así la secuencia del usuario no
debe declarar `parameters` en su raíz (firma vacía == vacía). Un PM custom
puede emparejar `parametros` ↔ `parameters` (post-MVP: librería de PMs).

## Por qué YAML y no JSON/XML

- **Diffable y legible** por no-programadores (ingenieros de test que
  authoran). XML (`.TapPlan` de OpenTAP, `.seq` de TestStand) es opaco al
  diff. JSON es legible pero sin comentarios.
- **Comentarios** (`#`): documentar por qué un límite es ese.
- **Versionable** en Git limpio.

## Sidecar de límites (post-MVP)

El property loader (ver [limites-y-estencias.md](limites-y-estados.md))
permite un **sidecar** de límites (p. ej. `basica.limits.yaml`) separado del
flujo, para cambiar umbrales por lote/variante sin tocar la secuencia.

## Out-of-scope

- Secuencias anidadas → cubiertas por **sequence call** (M4b): inline
  (`subsecuencias:`) o por path a archivo externo. **No** por include de
  YAML: el cargador resuelve los paths al cargar (ver arriba y ADR-0010).
- Schema formal (JSON Schema / WIT) publicable → post-MVP, cuando el
  contrato de secuencia se estabilice.