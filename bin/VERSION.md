bin/anac y bin/anac-stub — build del equipo de Ana:

  commit:   c4516e4a0c61ff7cb3bdc6a226035228f1010783
  describe: v0.26.2-12-gc4516e4
  fecha:    2026-07-19

Verificado antes de copiarlo aquí: punto fijo de auto-creación, 16/16
ejemplos dorados, y el banco de empaquetar.

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
