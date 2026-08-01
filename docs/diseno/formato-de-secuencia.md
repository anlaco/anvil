# Diseño: Formato de secuencia

> **Prioridad:** MVP. **Propuesta** (hoy la secuencia se construye en
> código; este doc define el schema YAML de entrada).

La secuencia es **datos** (ADR-0002). Este doc propone el schema YAML que
reemplaza la construcción en código de
`crates/motor/src/bin/basica_datos.rs`.

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
- Campos opcionales por paso: `limite` (ver
  [limites-y-estados.md](limites-y-estados.md)), `disable`, `pause_on_fail`
  (ver [motor-de-ejecucion.md](motor-de-ejecucion.md)), `precondicion`
  (ver [motor-de-expresiones.md](motor-de-expresiones.md)), y
  `parametros` (cuando el paso admita firma, post-MVP).
- Variables: `locals`, `parameters`, `file_globals` a nivel de secuencia
  (ver [variables-y-alcances.md](variables-y-alcances.md)).

## Cargador

- **Validación de schema** al cargar (campos obligatorios, tipos, `reintentos ≥ 1`).
- Errores de schema → la secuencia no carga (fail-fast), no se ejecuta a
  medias.
- El cargador produce `DefinicionSecuencia`; el motor no cambia.

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