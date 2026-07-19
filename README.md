# anvil

Un proyecto escrito en **Ana** (ana-lang; archivos `.ana`). Trae su propia
herramienta (`bin/anac`) lista para usar, sin depender de tener nada más
instalado.

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

`bin/anac` y `bin/anac-stub` no están versionados aquí — son un artefacto
de build, no código fuente de este proyecto. Ver `bin/VERSION.md`.

## El lenguaje

Ana lo desarrolla un equipo independiente, según lo que le piden sus
clientes — de los que `anvil` es uno. Este proyecto no tiene ni necesita el
código fuente del lenguaje: si algo hace falta y Ana no lo tiene, se anota
en [`NECESIDADES-ANA.md`](NECESIDADES-ANA.md) y se reporta aparte — nunca se
arregla desde aquí. La guía completa del lenguaje, para escribir código
`.ana` en este repo, está en `.claude/skills/ana/`.
