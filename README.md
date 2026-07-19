# anvil

Un proyecto escrito en [Anlaco](../anlaco-lang) (archivos `.ana`). Trae su
propio `anac` — el compilador/intérprete nativo de Ana — listo para usar,
sin depender de tener Python ni el repo del lenguaje instalados.

## Herramientas (`bin/`)

```
bin/anac ejecutar programa.ana              # interpreta y corre (como Python)
bin/anac compilar programa.ana              # Ana → programa.wat (texto wasm)
bin/anac ensamblar programa.wat             # .wat → programa.wasm (binario)
bin/anac empaquetar programa.ana [-o nombre] # Ana → ejecutable nativo standalone
```

`empaquetar` es lo más útil para un proyecto de verdad: produce un binario
que ya no necesita a `anac` para correr (arranca en unos milisegundos,
requiere `bin/anac-stub` al lado — ya está).

`bin/anac` y `bin/anac-stub` no están versionados aquí (son artefactos de
otro repo, no código fuente de este proyecto) — ver `bin/VERSION.md` para
saber de qué commit de `anlaco-lang` vienen y cómo refrescarlos.

## Referencia del lenguaje

Este repo no duplica la especificación ni el compilador — vive en el
[repo hermano `anlaco-lang`](../anlaco-lang), que es la fuente canónica.
La skill de Claude Code de este repo (`.claude/skills/anlaco/`) apunta ahí.
