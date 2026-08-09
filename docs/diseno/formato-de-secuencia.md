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
nombre: basica
setup:
  - nombre: conectar_equipo
    reintentos: 3
main:
  - nombre: medir_voltaje
    reintentos: 1
    limite:
      tipo: rango
      min: 4.5
      max: 5.5
  - nombre: verificar_led
    reintentos: 1
cleanup:
  - nombre: desconectar_equipo
    reintentos: 1
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
  `asigna` (M4, RF-31, vuelca `resultado.*` a `Locals` tras el paso),
  `tipo`/`statement` (M4, RF-27: `tipo: grpc|statement`, por defecto `grpc`;
  `statement` trae las sentencias a ejecutar si el paso es local, sin gRPC), y
  desde **M4b** `tipo: sequence_call` con `secuencia` (nombre de subsecuencia
  inline o path relativo a un archivo externo) y `parametros` (mapa
  `parameter -> "locals.X"`, by-reference). Un `sequence_call` no admite
  `reintentos > 1` ni `limite` (no mide; su estado es el agregado de la
  subsecuencia).
- Variables: `locals`, `parameters`, `file_globals` a nivel de secuencia
  (M4, RF-31, ver [variables-y-alcances.md](variables-y-alcances.md)). El tipo
  de cada variable se infiere del escalar YAML (`true`→bool, `4.5`→número,
  `"A"`→texto). Desde **M4b**, `subsecuencias:` a nivel de secuencia declara
  subsecuencias **inline** (mapa `nombre -> secuencia`), invocables por
  nombre; el `nombre:` de una inline es opcional (cae al de su clave).
- Desde **M5-ext.1** (RF-36.3, ver [executores-lenguaje.md](executores-lenguaje.md)):
  `ejecutores:` a nivel de secuencia declara la **tabla de ejecutores** y un
  paso `grpc` puede declarar `ejecutor: <nombre>` (si se omite, va al
  embebido). Cada ejecutor tiene `nombre` y `tipo`:
  - `tipo: embebido` — el ejecutor WASM de serie (`127.0.0.1:9100`). Default.
    Sin campos adicionales.
  - `tipo: wasm` — componente `.wasm` propio cargado por el **host** por
    path (M5-ext.2, ADR-0014/0015; implementado). Campo `path` (relativo
    al YAML, debe existir). El host spawnea el puente `anvil-puente-wasm`,
    que carga el componente (interfaz WIT `anvil:paso`, función `run`) y lo
    expone al motor como `grpc` (override sintético).
  - `tipo: grpc` — ejecutor de lenguaje distribuido (p. ej. Python). Campos
    `host`/`puerto` (obligatorios). IPs no-loopback **sólo si se declaran**
    (relajación acotada del loopback, ADR-0011).
  
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