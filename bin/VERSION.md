bin/anac y bin/anac-stub están compilados desde:

  repo:    anlaco-lang
  commit:  c4516e4a0c61ff7cb3bdc6a226035228f1010783
  describe: v0.26.2-12-gc4516e4
  fecha:   2026-07-19

No hay todavía una GitHub Release formal de anlaco-lang con estos binarios
adjuntos (los tags existen, pero el workflow de release solo publica al
empujar un tag — hoy se compiló a mano, sin tag nuevo). Estos binarios son
un build directo del `main` de ese día, ya verificado: punto fijo de
auto-creación, 16/16 ejemplos dorados, y el banco de empaquetar.

Para refrescarlos cuando anlaco-lang avance:

  cd ../anlaco-lang/native/anac && cargo build --release
  cd ../anac-stub && cargo build --release
  cp target/release/anac       ../../anvil/bin/anac
  cp target/release/anac-stub  ../../anvil/bin/anac-stub

bin/anac y bin/anac-stub NO están versionados en este repo (ver
.gitignore) — son artefactos de otro repo, no código fuente de anvil. Este
archivo sí lo está: es lo que deja constancia de qué versión hay presente.
