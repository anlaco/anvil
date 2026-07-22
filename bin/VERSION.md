bin/anac y bin/anac-stub — build del equipo de Ana:

  commit:   2435a9749da1639919443b4e5f9fc4ac9450b9b7
  describe: v0.26.5-8-g2435a97
  fecha:    2026-07-22

Salto grande desde la copia anterior (v0.26.2-15-g0bef9b1, que era
pre-v0.4 y sin red). Lo que trae esta versión, en orden de importancia
para anvil:

- **Red TCP real (lo que anvil pidió).** Sockets de servidor
  (`la escucha del puerto`, `la aceptación de`, `bytes recibidos de`,
  `envía … a`, `cierra`) y de cliente (`la conexión a … en el puerto`).
  Verificado aquí mismo con `bin/anac`: `la escucha del puerto 9099`
  devuelve un asa de socket real. Bajo el capó, `anac` migró a WASI
  Preview 2 / Component Model (`wasi:sockets`), y `empaquetar` produce
  binarios con un puente `wasi:sockets` de verdad.

- **v0.4 del lenguaje (cambios que ROMPEN código pre-v0.4).** La forma
  `con` de funciones se ELIMINÓ: `f con a, b` ahora da error amable —
  usa las formas `de` / `a`-`en` / `?` (ver la skill). Comparar tipos
  distintos con `es` ahora da error de ejecución (antes: `falso` en
  silencio). Rutas de módulos/archivos relativas al fuente, no al cwd.
  `nada` sustituye al centinela `-1`. `a`/`veces` ya valen como nombre
  de parámetro en la forma `de`.

- **Fix del issue #4** (ya venía en la copia anterior, se mantiene):
  `usa "sub/carpeta/módulo"` seguido de llamada cualificada ya funciona
  (caché por nombre corto).

Nota importante para anvil: los **diccionarios** (`{clave: valor}`) son
del ORÁCULO Python solamente; el `bin/anac` self-hosted NO los soporta
(su lexer no tiene `{`/`}`). No los uses aquí. Ver la skill.

Verificado antes de copiarlo aquí: `escribe`, listas, `resto`/`cociente`,
y una escucha TCP real, todo con este `bin/anac`.

Nota de mantenimiento (no forma parte de lo que este proyecto necesita saber
para escribir Ana — ver .claude/skills/ana/ para eso): esta copia se generó
a mano desde el repositorio de compilación del lenguaje, disponible en
../anlaco-lang en esta máquina, en la rama `main`. Para refrescarla:

  cd ../anlaco-lang && git checkout main
  cd native/anac && cargo build --release
  cd ../anac-stub && cargo build --release
  cp ../anac/target/release/anac        ../../anvil/bin/anac
  cp target/release/anac-stub           ../../anvil/bin/anac-stub

bin/anac y bin/anac-stub NO están versionados en este repo (ver
.gitignore) — son un artefacto de build, no código fuente de anvil. Este
archivo sí lo está: deja constancia de qué versión hay presente.
