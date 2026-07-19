bin/anac y bin/anac-stub — build del equipo de Ana:

  commit:   c545b22a072925aa9b696bdb62a7d3f6d0daceb9
  describe: v0.26.2-14-gc545b22
  fecha:    2026-07-19

Arregla el issue #2 (compilar/ensamblar/empaquetar fallaban con "No
encuentro el archivo preludio_es.wat"): ese archivo ahora va embebido en
el binario y se materializa solo si falta en el directorio de trabajo,
así que ya no hace falta copiarlo junto a anac/anac-stub.

Verificado antes de copiarlo aquí: punto fijo de auto-creación, el banco
de empaquetar, y a mano el caso exacto del issue #2 (ejecutar/compilar/
empaquetar el programa mínimo del reporte, aquí mismo en bin/, sin
preludio_es.wat presente).

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
