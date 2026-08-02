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
  `asigna` (M4, RF-31, vuelca `resultado.*` a `Locals` tras el paso) y
  `tipo`/`statement` (M4, RF-27: `tipo: grpc|statement`, por defecto `grpc`;
  `statement` trae las sentencias a ejecutar si el paso es local, sin gRPC), y
  `parametros` (cuando el paso admita firma, post-MVP).
- Variables: `locals`, `parameters`, `file_globals` a nivel de secuencia
  (M4, RF-31, ver [variables-y-alcances.md](variables-y-alcances.md)). El tipo
  de cada variable se infiere del escalar YAML (`true`→bool, `4.5`→número,
  `"A"`→texto).
- El subconjunto es **estricto** (`deny_unknown_fields`): un campo no reconocido
  falla la carga en vez de ignorarse en silencio. `precondicion`/`asigna`/
  `statement` se **parsean a AST al cargar** (fail-fast): un error de sintaxis
  se reporta como error de validación con el nombre del paso (ADR-0009).

## Cargador

- **Validación de schema** al cargar (campos obligatorios, tipos, `reintentos ≥ 1`,
  coherencia `tipo` ↔ `statement`: un `statement` sin `statement` o un `grpc`
  con `statement` son error).
- Errores de schema → la secuencia no carga (fail-fast), no se ejecuta a
  medias.
- El cargador produce `DefinicionSecuencia`; el motor la recorre (ADR-0005).

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

- Secuencias anidadas como archivos referenciados → cubierto por
  *sequence call* (post-MVP), no por include de YAML en esta fase.
- Schema formal (JSON Schema / WIT) publicable → post-MVP, cuando el
  contrato de secuencia se estabilice.