---
name: ana
description: >-
  Referencia del lenguaje Ana (archivos .ana) y protocolo para verificar
  cualquier duda ejecutando programas mínimos. Úsala SIEMPRE que se lea,
  escriba, modifique o depure código .ana, se responda una pregunta sobre cómo
  funciona Ana, se trabaje en el compilador autoalojado
  (src/lexer.ana), o se toque el intérprete/compilador del repo
  anlaco-lang (administrado por la organización Anlaco). También cuando el
  usuario mencione "ana", "trocea", "fichas" o pegue código que se lee como
  español con "escribe", "si ...:", "fin". Regla central: lo que la skill no
  responda, se comprueba con un programa mínimo — nunca se contesta de
  memoria.
---

# Ana — referencia y protocolo de verificación

Ana es un **lenguaje natural controlado**: un punto medio entre la máquina
y el humano — fácil de entender sin saber programar, con palabras de tu
idioma (español o inglés), pero cada construcción tiene exactamente UNA forma
gramatical.
Archivos `.ana`. Dos motores que deben dar salida idéntica: el oráculo
Python (`oraculo/ana/`) y el compilador a WebAssembly. La v0.1 está congelada
(tag `v0.1.0`); la v0.2 crece solo con lo que exija el compilador autoalojado
(método Wirth; cascada de ramas main → lexer → parser → compilador).

Rutas relativas a la raíz del repo `anlaco-lang`.

## Fuentes canónicas (en este orden)

| Pregunta sobre... | Mira en |
|---|---|
| Sintaxis, semántica, gramática EBNF | `espec/especificacion.md` (NORMATIVA; §5.1 isla de cálculo, §11 gramática) |
| Programas de ejemplo con su salida esperada | `espec/ejemplos/*.ana` + `.out` (e `.in` si leen teclado) |
| Qué le falta al lenguaje y por qué | `src/BITACORA.md` (choques del autoalojado) |
| Palabras clave exactas por idioma | `oraculo/ana/keywords/es.toml`, `en.toml` |
| Qué queda fuera a propósito | `espec/especificacion.md` §12 + `espec/ideas-futuras.md` |
| Mensajes de error | `oraculo/ana/mensajes/` |

La especificación manda sobre esta skill: si contradicen algo, gana la espec
(y conviene corregir la skill).

## Cómo ejecutar (desde la raíz del repo)

```bash
PYTHONPATH=oraculo python -m ana ejecutar programa.ana   # ejecuta
PYTHONPATH=oraculo python -m ana fichas programa.ana     # cómo trocea el lexer
PYTHONPATH=oraculo python -m ana arbol programa.ana      # el árbol que ve el intérprete
PYTHONPATH=oraculo python -m ana formatear programa.ana  # forma canónica
PYTHONPATH=oraculo python -m ana traducir programa.ana --a en
PYTHONPATH=oraculo python -m ana compilar programa.ana   # escribe .wat y .wasm
PYTHONPATH=oraculo python -m ana                         # REPL interactivo
```

El `.wat`/`.wasm` compilado se ejecuta con wasmtime (o `~/.wasmtime/bin/wasmtime`):

```bash
wasmtime run --dir . -W function-references=y,gc=y programa.wasm
```

Tests del proyecto: `PYTHONPATH=src python -m pytest tests/` (los de wasm se
saltan solos si no hay wasmtime).

El binario nativo (`native/anac/target/release/anac`, ver más abajo) da lo
mismo sin Python de por medio, más un cuarto verbo que el oráculo no tiene:

```bash
native/anac/target/release/anac ejecutar programa.ana
native/anac/target/release/anac compilar programa.ana    # escribe .wat
native/anac/target/release/anac ensamblar programa.wat   # escribe .wasm
native/anac/target/release/anac empaquetar programa.ana [-o nombre]  # ejecutable
                                  # nativo standalone (necesita native/anac-stub/
                                  # compilado — no forma parte del autoalojado,
                                  # ver la sección de más abajo)
```

## Protocolo empírico — la regla de oro de esta skill

Si una pregunta no se responde con esta skill ni con la espec, **no adivines**:

1. Escribe el programa mínimo que la decide (2-6 líneas) en el scratchpad.
2. Pásalo por `ejecutar`. Si la duda es léxica o sintáctica, mira también
   `fichas` y `arbol` — dicen POR QUÉ algo se acepta o se rechaza.
3. Si la duda es de semántica (no de mensajes de error), verifica también en
   el compilador: `compilar` + wasmtime. Los dos motores deben coincidir.
4. Con la respuesta en la mano: si es un hecho estable del lenguaje, añádelo
   a la sección de gotchas de esta skill; si es algo que el lenguaje NO puede
   hacer y duele, es candidato a choque en `src/BITACORA.md`.

**Nunca inventes sintaxis nueva** para salir de un apuro: el lenguaje solo
crece por la bitácora (método Wirth). Escribe el rodeo con lo que hay (mira
la sección «Verificado que SÍ da de sí la v0.1» de la bitácora) y anota el dolor.

## Chuleta (v0.2 + v0.3 + v0.4)

```
# comentario                          # idioma: es  (primera línea, opcional)
el precio es 100                      # asignación; "son" para plurales
la lista es ["pan", 3.14, verdadero]  # índices DESDE 1
el d es {"pan": 3, "leche": 2}        # v0.4: diccionario, clave->valor
escribe elemento "pan" de d           # 3 — mismo "elemento...de" que las listas
escribe elemento "huevos" de d        # nada — clave ausente, NO es error
pon ["huevos", 6] en d                # añade o sobreescribe una clave
escribe "Total: {precio + 21} €"      # interpolación: {} evalúa; {{ }} = llave literal
el iva es (precio * 0.21)             # TODO cálculo va entre paréntesis (isla)
si precio es mayor que 50:            # comparaciones siempre con palabras
    escribe "caro"
si no, si precio es 50:               # v0.2: cadena con coma obligatoria
    escribe "justo"
si no:
    escribe "barato"
fin                                   # UN solo fin cierra toda la cadena
para cada letra en "Añil":            # v0.2: un texto es la lista de sus letras
    escribe letra
fin
para cada n del 1 al 10:
mientras n es menor que 5:
repite 3 veces:
    rompe                             # v0.4: corta el bucle más interno (break)
define doble de n:                    # forma "de": función de VALOR — devuelve algo
    devuelve (n * 2)                  # llamada: doble de 3
fin
define registra un texto en una ruta: # forma "a"/"en": función de ACCIÓN — no devuelve nada
    escribe "{texto} -> {ruta}"       # llamada: registra "hola" en "f.txt"
fin
define una caja contiene? una buscada:   # forma infija "?": función BOOLEANA (predicado)
    devuelve verdadero                   # llamada: [1, 2, 3] contiene 2
fin
guarda "hola" en "notas.txt"          # archivos
añade "adiós" a "notas.txt"           # a texto = archivo; a lista = elemento
el t es contenido de "notas.txt"
detente con "El número no es válido"  # v0.2: para el programa; mensaje a stderr, código 1
la edad es respuesta a "¿años?"       # entrada; "15" se vuelve número solo
elemento 2 de lista, primero de, último de, cantidad de, al azar entre 1 y 6
usa "lexer"                           # v0.2: módulos — trae lexer.ana (misma carpeta)
las fichas son lexer.trocea de código  # SIEMPRE cualificado con punto; carga 1 vez
el archivo es elemento 1 de los argumentos  # v0.2: los de la línea de comandos
un punto tiene:                      # v0.2: tarjetas (registros) — declara un tipo
    una x
    una altitud
fin
el p es un nuevo punto con la x 3, la altitud 4   # construye una tarjeta
escribe p.x                          # acceso a campo; el punto NO se encadena
el m es resto de 17 entre 5          # v0.3: división entera de suelo (2)
el q es cociente de 17 entre 5       # (3)
los b son bytes de "hola"            # v0.3: bytes UTF-8, lista de 0-255
guarda los bytes b en "salida.bin"   # v0.3: escritura binaria, sin decodificar
el t es texto de b                   # v0.4: bytes UTF-8 -> texto (inverso de "bytes de")
los ocho son bytes del decimal 4.2   # v0.3: 8 bytes IEEE-754 LE de un decimal
el d es decimal de ocho              # v0.4: 8 bytes IEEE-754 -> decimal (inverso)
```

Los argumentos (v0.2): `los argumentos` es la lista de textos que siguen al
programa (`ana ejecutar prog.ana 7 hola` → `["7", "hola"]`; sin el nombre
del programa; siempre textos; una lista nueva cada vez). Ver §5.7.

Escapes en textos (lista CERRADA): `\"` `\n` `\\`. Nada más tras la barra.

## Formas de `define` — la elige el tipo de devolución, no el gusto (§4.5)

Ana tiene TRES formas de `define` (más la ausencia total de parámetros), el
mismo eje que las primitivas (`cantidad de X`, `guarda X en Y`, `A es mayor
que B`):

| forma | devuelve | definición | llamada |
|---|---|---|---|
| sin parámetros | lo que sea | `define saluda:` | `saluda` |
| `de` | un valor | `define doble de n:` | `doble de 3` |
| `a` / `en` | nada (acción) | `define registra un texto en una ruta:` | `registra "hola" en "f.txt"` |
| infija `?` | booleano | `define una caja contiene? una buscada:` | `[1,2,3] contiene 2` |

**Una función, una forma; se elige `de`/`a`-`en`/infija `?` según lo que la
función devuelva.** La forma `con` (legado, posicional con comas) **se
eliminó en v0.4** — escribirla da un error amable que explica cómo migrar
(ver `src/BITACORA.md` "Quitar la forma con del todo"). Hasta ese momento la
usaba el propio compilador autoalojado (`lexer.trocea con código`, etc.);
hoy todo el árbol de `src/*.ana` (~1038 sitios, 148 funciones) usa `de`,
convertido mecánicamente y verificado igual de árbol AST antes/después.
Verificado que la anidación de llamadas entre paréntesis en forma `de`
(`fib de (fib de 4)`, `[(fib de 3), 2]`) funciona igual que antes en `con`.
El artículo indefinido presenta cada parámetro en cualquier forma:
`define área de un ancho, una altura`, `define una caja contiene? una
buscada`. **Sin techo formal**: `de` acepta tantos parámetros como haga
falta (el compilador autoalojado tiene funciones con hasta 6); la guía de
estilo recomienda 1-3 por legibilidad, pero no es un límite de gramática.

## Gotchas — lo que sorprende al que viene de otros lenguajes

- **La isla de cálculo**: `el x es precio * 2` es ERROR; se escribe
  `el x es (precio * 2)`. Dentro de `{...}` de interpolación no hacen falta
  paréntesis. `más/menos/por/entre` son alias de `+ - * /` (canónico: símbolos).
- **Los índices empiezan en 1.**
- **La sangría NO significa nada**: la estructura la dan `:` y `fin`. Canónica:
  4 espacios; `formatear` la repara.
- **Los dos registros (regla de oro 5)**: todo programa puede escribirse LLANO
  (`variable es 3`, `define área de ancho, alto:`) o ADORNADO con artículos
  (`la variable es 3`, `de un ancho, una altura`). Ambos son el MISMO árbol;
  `formatear` respeta el registro elegido. Ninguna frase EXIGE azúcar para
  funcionar. La IA escribe con azúcar; los programadores pueden ir secos.
- **Una sentencia por línea**, salvo dentro de un paréntesis o corchete sin
  cerrar: una lista literal o un cálculo SÍ pueden partirse en varias líneas
  (choque 4 de la bitácora, resuelto en v0.3). La llamada en forma `de` (sin
  paréntesis propios, solo comas) sigue sin poder partirse en varias líneas.
- **`es` es dos cosas**: al inicio de sentencia, asignación; en condición,
  igualdad. No hay asignación dentro de expresiones.
- **Sin truthiness**: las condiciones exigen booleano (una variable booleana
  sola sí vale: `si carnet:`). No hay `no` suelto: la negación es `no es`.
- **Palabras reservadas traicioneras, en general**: las keywords y artículos
  no valen como nombre de variable ni de parámetro. **Excepción, desde v0.4**:
  `a` y `veces` SÍ pueden nombrar un parámetro en la forma `de` —
  `define media de a, b:` ya funciona (una pasada del lexer detecta que
  "de"/coma→coma introduce un nombre, no una preposición, y reinterpreta
  esa palabra como nombre en toda la función). Es un rodeo angosto a
  propósito: solo cubre `a`/`veces` (las dos únicas que chocan de verdad en
  la práctica) y solo en la forma `de` — la forma acción `a`/`en` NO está
  cubierta porque ahí "a"/"en" también pueden ser la preposición que
  introduce la propia forma, y desambiguar eso es ambiguo de verdad, no solo
  difícil. El resto de keywords (`en`, `con`, `y`, `o`...) siguen bloqueadas
  siempre, en cualquier forma — demasiado cargadas gramaticalmente dentro
  del cuerpo de una función como para reinterpretarlas sin riesgo (`con`
  además sigue viva para construir tarjetas: `un nuevo TIPO con CAMPO
  valor`). `el/la/las/un/una` se ignoran (decorativos) pero se conservan al
  formatear.
- **Llamadas**: la sintaxis depende de la forma elegida en el `define` (ver
  arriba) — `f de x`, `f a x`, `x f y` (infija). La forma `con` (`f con x,
  y`) se eliminó en v0.4 — da un error amable. Una llamada como argumento de
  otra o como elemento de lista literal va entre paréntesis: `fib de (fib de
  4)`, `[(fib de 3), 2]`. Dentro de un cálculo no hace falta: `(fib de
  (n - 1) + fib de (n - 2))`. Antes existía un gap documentado donde el
  compilador WASM del oráculo Python no compilaba una llamada de valor
  suelta como sentencia sin asignar (rodeo: `el _ es f de x`); verificado
  que `anac compilar` (el compilador autoalojado) ya NO lo bloquea — si te
  encuentras el error, compara qué motor estás usando.
- **Funciones solo en el nivel superior**. Las globales se LEEN desde una
  función; escribirlas es error. Recursión: mínimo 1000 niveles garantizados.
- **`añade X a Y`** depende del tipo de Y: lista → elemento; texto → archivo.
  Los textos son inmutables (concatenar = interpolación: `"{a}{b}"`, ojo O(n²)).
- **Comparar tipos distintos con `es` da ERROR de ejecución (v0.4)**, no
  `falso` en silencio — salvo entero vs decimal (comparan numéricamente) y
  cualquier cosa contra `nada` (siempre vale, para `si X es nada:`). `(7 / 2)`
  es `3.5`; `21.0` se muestra `21`.
- **Módulos (v0.2)**: `usa "X"` ejecuta X.ana al importar (¡sin escribe de
  demo en bibliotecas!) y todo se usa cualificado: `X.func de args`,
  `X.tabla`. NO hay import plano. Dentro del módulo, sus funciones se llaman
  sin cualificar. El compilador WASM YA compila `usa` (inlining: cada
  módulo aporta sus funciones/globales al mismo .wasm y su cuerpo se
  convierte en una función `$init` con guarda; `usa` compila a su llamada).
  Gotcha de compilación: los accesos cualificados (`M.x`) se resuelven en
  tiempo de compilación contra el módulo ya compilado, así que solo son
  fiables después de que un `usa "M"` se haya EJECUTADO antes en el
  programa (no hay carga perezosa en el sitio del acceso, a diferencia del
  intérprete). En el uso normal (siempre con `usa` primero) es invisible.
  **Cómo se resuelve la ruta de `usa` (arreglado #9/#10, 2026-07-23)** — dos
  reglas simples: un **NOMBRE CORTO** (`usa "huffman"`, sin `/`) busca el
  archivo **junto al módulo que hace el import** (relativo al archivo); una
  **RUTA con `/`** (`usa "grpc/protobuf"`) se resuelve **desde la raíz del
  programa** (el directorio de trabajo), venga de donde venga el `usa`. Así:
  un módulo en `grpc/hpack.ana` puede hacer `usa "huffman"` para su hermano
  `grpc/huffman.ana` (nombre corto, relativo), Y `usa "grpc/protobuf"` sigue
  apuntando a `grpc/protobuf.ana` desde cualquier profundidad (ruta, raíz) —
  sin acumular carpetas. Antes del arreglo, el `usa` interno se resolvía
  contra el cwd del proceso (rompía los imports por nombre corto entre
  hermanos). La caché sigue indexada por nombre corto (`X.func` solo conoce
  el último tramo). Los cuatro caminos (oráculo/nativo × interpretar/compilar)
  coinciden.
- **Tarjetas (v0.2, registros con campos)**: `un punto tiene:` … `fin`
  declara un tipo (nivel superior, hermana de `define`); `un nuevo punto
  con la x 3` construye (los campos no rellenados valen `nada`, cualquier
  orden); `p.x` lee un campo. El punto **NO se encadena**: para anidar,
  guarda en una palabra (`la izq es raiz.izquierda` … `izq.clase`); un
  segundo punto (`raiz.izquierda.clase`) es error amable. La forma
  `X.miembro` sirve para módulos Y para campos: se distingue por la
  izquierda (módulo cargado con `usa` → módulo; si no → campo de la
  variable `X`). Ambos motores las compilan byte a byte. Ver §4.9.
- **Diccionarios (v0.4)**: `{"pan": 3, "leche": 2}` (literal), `elemento
  CLAVE de D` (lee — reutiliza el mismo "elemento...de" de las listas),
  `pon [CLAVE, VALOR] en D` (escribe/sobreescribe, acción sin cópula, como
  `guarda`/`añade`), `cantidad de D`, `para cada CLAVE en D:` (recorre
  claves). **Clave ausente da `nada`, no error** (a diferencia de un índice
  de lista fuera de rango) — sirve también para comprobar existencia: `si
  (elemento X de D) es nada:`. Claves válidas: número o texto, NUNCA
  booleano (por dentro son un dict de Python; `hash(True) == hash(1)`
  rompería la regla de tipos incomparables de §6). Entero y decimal son la
  misma clave si valen igual (`3` y `3.0`). **Solo el intérprete todavía**:
  el compilador a WASM da un error amable y remite a `ana ejecutar` (mismo
  mecanismo que la forma acción sin compilar) — nadie lo ha pedido
  (método Wirth). Ver §4.10.
- **No hay** (a propósito, espec §12): negación suelta, textos multilínea
  (de verdad, dentro de comillas), excepciones. (`detente con` SÍ existe desde v0.2: para el
  programa; el mensaje va a stderr y termina con código 1 — ver §4.8. A
  diferencia de `devuelve` —que solo sale de la función— `detente` para todo
  el programa, se ejecute donde se ejecute.)
- **«Ana de máquina» (v0.3)**: `resto de`/`cociente de` son división entera
  de SUELO (como Python: `resto de -1 entre 128` es `127`, no `-1`). Una tira
  de bytes NO es un tipo nuevo: `bytes de "Añil"` es una LISTA de enteros
  0-255 por BYTE UTF-8 (`[65, 195, 177, 105, 108]`, 5 elementos), a diferencia
  de `cantidad de`/`elemento N de`, que cuentan LETRAS (`cantidad de "Añil"`
  es `4`). `guarda los bytes B en RUTA` escribe binario sin pasar por
  `mostrar` (a diferencia de `guarda`). El compilador WASM exige que el
  divisor de `resto`/`cociente` quepa en 32 bits; el intérprete no tiene ese
  límite. Ver §5.4/§4.6/§6.
- **Inversos de bytes (v0.4)**: `texto de BYTES` reconstruye un texto de una
  lista de bytes UTF-8 (inverso de `bytes de TEXTO`); `decimal de BYTES`
  reconstruye un decimal de exactamente 8 bytes IEEE-754 little-endian
  (inverso de `bytes del decimal N`, que YA existía desde v0.3 — ¡esa es la
  frase para decimal→bytes, no `bytes de 4.2`!). En inglés `text of` /
  `decimal of`. Cierran el round-trip para hablar protocolos binarios. El
  intérprete valida: `texto de` falla amable si los bytes no son UTF-8 válido;
  `decimal de` exige 8 bytes. **Asimetría consciente**: el compilado NO valida
  UTF-8 en `texto de` (solo el intérprete) — coinciden ante bytes válidos (el
  caso real), divergen solo ante UTF-8 inválido. `texto`/`decimal` sueltos
  siguen libres como nombre de variable (solo la frase de dos palabras casa).
- **`rompe` (v0.4, break)**: corta el bucle más interno (`para cada`,
  `mientras`, `repite`) y sigue tras él. A diferencia de `devuelve` (sale de
  la función) y `detente` (para el programa), `rompe` solo abandona el bucle;
  en bucles anidados solo el interno. Fuera de un bucle es error amable. En
  inglés `break`. Palabra suelta, sin argumento.
- **Multilingüe**: el idioma se detecta solo o se fija con `# idioma: es`.
  Frases multi-palabra: gana la más larga (`es mayor o igual que` antes que `es`).

## El compilador autoalojado — ⭐ ANA YA ESTÁ AUTOALOJADA (2026-07-13)

En `src/` vive la cadena entera de Ana escrita en Ana —
`lexer.ana`, `parser.ana`, `evaluador.ana`, `compilador.ana`,
`ensamblador.ana` (v0.3, el port de `wat2wasm.py`) y `anac.ana` (el
driver CLI: `ejecutar`/`compilar`/`ensamblar`) — los programas Ana más
grandes que existen y el cliente que dirigió la v0.2 y la v0.3. Las fichas
del lexer son `[tipo, texto, línea]`; el árbol del parser son TARJETAS por
clase (`un escribe_n tiene: clase, expresion fin`, no listas por posición
— migrado en R3, 2026-07-14). **PUNTO FIJO conseguido**: `anac.ana`
compilado a `.wasm` se compila a sí mismo y el resultado es byte a byte
idéntico generación tras generación (banco `verifica_generaciones.py`,
estándar GCC). El `.wat` que emite `anac.ana` es el módulo COMPLETO
(preludio renderizado + programa); wasmtime lo corre directo, y
`anac ensamblar` lo convierte a `.wasm` binario sin Python.

Bancos (todos verdes): `verifica_generaciones.py` (el punto fijo),
`verifica_bootstrap.py` (el compilador compilado compila los ejemplos),
`verifica_parser.py`, `verifica_compilador.py`, `verifica_evaluador.py` (con
módulos), `verifica_detente.py` (detente en las tres rutas de la cadena,
comparando stdout, stderr y código), `verifica_errores.py` (errores amables
del lexer y el parser en las tres rutas), `verifica_ejecutor.py` (el
INTÉRPRETE autoalojado en wasm, sin Python), `verifica_tarjetas.py` (registros
en los tres motores), `verifica_anac.py` (el driver CLI unificado + su propio
punto fijo), `verifica_ensamblador.py` (v0.3: el ensamblador, con la prueba de
fuego de ensamblar `native/anac/anac.wat` entero) — cada uno byte a byte contra
el anfitrión, que sigue de ORÁCULO. Cada muro nuevo se apunta en
`src/BITACORA.md` con su programa mínimo; la solución se decide con el
usuario y se construye en `main`. Al tocar un `.ana`, verifica en los dos
motores Y que el punto fijo se mantiene.

**`anac empaquetar` (2026-07-19) NO es autoalojado**: a diferencia de
`ejecutar`/`compilar`/`ensamblar` (los tres reenvían el argv al mismo
`anac.wasm` embebido — código Ana puro), `empaquetar` es lógica exclusiva
del host en Rust (`native/anac/src/main.rs`): compila+ensambla con el
compilador embebido, precompila el `.wasm` resultante a código máquina, y lo
pega a `native/anac-stub/` (binario hermano sin compilador dentro, crate
aparte que comparte `native/anac-motor/` para la config del motor wasmtime)
para producir un ejecutable standalone. Consecuencia comprobada: empaquetar
`anac.ana` (el propio driver autoalojado) da un ejecutable que sabe
`ejecutar`/`compilar`/`ensamblar` de sí mismo — reproduce su `anac.wat` byte
a byte — pero NO sabe `empaquetar`, porque esa pieza no vive en Ana. Banco:
`native/anac/verifica_empaquetar.py`. Sin compilación cruzada todavía:
empaqueta para la máquina donde corre.

Los ERRORES de Ana van por stderr con código de salida 1 (como
GCC/Clang/rustc), igual que `detente`: stdout queda para el resultado. Lo
unificó la fase C (2026-07-14), tras decidir el usuario que un compilador
serio hace así.

Python ya está jubilado del CAMINO DE BUILD (v0.3, 2026-07-19): con
`ensamblador.ana` + `anac ensamblar`, `web/construir.py` fabrica `anac.wasm`
sin tocar Python en ningún punto. Sigue vivo solo como ORÁCULO de los
bancos `verifica_*.py` (comparación byte a byte), nunca en el camino de
ejecución. Lo que aún QUEDA (aparcado, sin cliente que lo exija): el
COMPILADOR (tanda 3 de errores amables, ~6 búsquedas de función no
definida — ya no bloqueada por structs, que llegaron con las tarjetas;
sigue aplazada porque no duele), más errores del parser (EOF a media
frase), y el PRELUDIO reescrito en Ana (bignum/mostrar/WASI — memoria
lineal y punteros de verdad, con huevo-gallina: es el propio runtime de
los valores de Ana). Gotcha de la
cadena: `contenido de` resuelve rutas contra el directorio de
trabajo, no contra la carpeta del fuente.
