bin/anac y bin/anac-stub — build del equipo de Ana:

  commit:   6296c64ae7c39ebdd98855b15087d01b215b5923
  describe: v0.26.5-12-g6296c64
  fecha:    2026-07-23

Lo que trae esta versión desde la anterior (v0.26.5-8-g2435a97), en orden
de importancia para anvil — cierra CINCO issues que anvil abrió:

- **Los inversos de bytes (issues #6 y #8).** `texto de BYTES` reconstruye
  un texto de una lista de bytes UTF-8 (inverso de `bytes de TEXTO`) — p. ej.
  para leer el valor de una cabecera HTTP/2 o un campo `string` de protobuf
  recibido por socket. `decimal de BYTES` reconstruye un decimal de 8 bytes
  IEEE-754 little-endian.
  Ojo con el #8: la IDA ya existía y no la habíais encontrado — es
  `bytes del decimal 4.2` (no `bytes de 4.2` ni `bytes ieee de 4.2`). La
  vuelta (`decimal de`) es lo nuevo. El round-trip completo:
  `decimal de bytes del decimal 4.2` → `4.2`.

- **`rompe` (break) (issue #7).** Corta el bucle más interno (`para cada`,
  `mientras`, `repite`) antes de tiempo. Ya no hace falta la bandera
  booleana de rodeo — el cuerpo restante ni se evalúa (útil para pasos con
  efectos secundarios costosos). En inglés, `break`.

- **Módulos hermanos por nombre corto (issues #9 y #10).** Un módulo cargado
  con ruta de subcarpeta ya puede depender de un hermano por nombre corto:
  `grpc/hpack.ana` puede hacer `usa "huffman"` (sin ruta) y encontrar
  `grpc/huffman.ana`, corriendo desde donde sea. La regla, dos casos:
  **nombre corto** (`usa "huffman"`) → relativo al archivo que importa;
  **ruta con `/`** (`usa "grpc/protobuf"`) → desde la raíz del programa.
  Vuestro código actual (rutas desde la raíz por todo el árbol) sigue
  funcionando SIN cambios — verificado aquí con spike_hpack, spike_huffman y
  el árbol del secuenciador. Y `compilar` y `ejecutar` ahora coinciden (era
  el #10). El symlink de rodeo ya no hace falta.

Verificado antes de copiarlo aquí: las tres primitivas nuevas en los cuatro
caminos (interpretar/compilar), `rompe` en los tres tipos de bucle, y el
árbol real de grpc/secuenciador de este proyecto.

Nota de mantenimiento (no forma parte de lo que este proyecto necesita saber
para escribir Ana — ver .claude/skills/ana/ para eso): esta copia se generó
a mano desde el repositorio de compilación del lenguaje, disponible en
../anlaco-lang en esta máquina, en la rama `main`. Para refrescarla:

  cd ../anlaco-lang && git checkout main
  cd native/anac && ./construir.sh        # reconstruye anac (autoalojado)
  cd ../anac-stub && cargo build --release
  cp ../anac/target/release/anac        ../../anvil/bin/anac
  cp target/release/anac-stub           ../../anvil/bin/anac-stub
  cp ../../src/preludio_es.wat          ../../anvil/preludio_es.wat  # si hace falta
  cp ../../.claude/skills/ana/SKILL.md  ../../anvil/.claude/skills/ana/SKILL.md

bin/anac y bin/anac-stub NO están versionados en este repo (ver
.gitignore) — son un artefacto de build, no código fuente de anvil. Este
archivo sí lo está: deja constancia de qué versión hay presente.
