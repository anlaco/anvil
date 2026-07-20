bin/anac y bin/anac-stub — build del equipo de Ana:

  commit:   0bef9b1d94a9ef378cf2f5096f927cfdbf7e1301
  describe: v0.26.2-15-g0bef9b1
  fecha:    2026-07-20

Arregla el issue #4 (`usa "X"` seguido de `usa "Y"` fallaba a veces por
el nombre del módulo): la causa real era que una llamada cualificada
(`modulo.función con args`) solo conoce el nombre CORTO del módulo — el
que se usa en el punto de llamada, nunca la ruta del `usa` que lo trajo.
Cuando el `usa` tenía subcarpeta (`usa "secuenciador/motor"`), la caché
de módulos quedaba indexada por la ruta completa y la llamada cualificada
(que busca solo por "motor") no la encontraba, así que intentaba recargar
"motor.ana" contra el directorio de trabajo actual y fallaba con "No
encuentro el archivo". Arreglado en los tres motores (intérprete y
compilador autoalojados, y el oráculo Python) para que todos cacheen y
busquen los módulos por su nombre corto. Ya NO hace falta el workaround
de evitar rutas con subcarpeta en `usa` ni renombrar módulos para
esquivar la colisión.

Verificado antes de copiarlo aquí: punto fijo de auto-creación, el banco
de empaquetar, la suite completa del repo de Ana (525 tests), y a mano
el caso exacto del issue #4 (aquí mismo en bin/, con `usa` a una
subcarpeta seguido de una llamada cualificada).

Nota de mantenimiento (no forma parte de lo que este proyecto necesita saber
para escribir Ana — ver .claude/skills/ana/ para eso): esta copia se generó
a mano desde el repositorio de compilación del lenguaje, disponible en
../anlaco-lang en esta máquina. Para refrescarla:

  cd ../anlaco-lang/native/anac && cargo build --release
  cd ../anac-stub && cargo build --release
  cp target/release/anac       ../../anvil/bin/anac
  cp target/release/anac-stub  ../../anvil/bin/anac-stub

bin/anac y bin/anac-stub NO están versionados en este repo (ver
.gitignore) — son un artefacto de build, no código fuente de anvil. Este
archivo sí lo está: deja constancia de qué versión hay presente.
