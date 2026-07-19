---
name: anlaco
description: >-
  Referencia del lenguaje Anlaco (archivos .ana) y protocolo para verificar
  cualquier duda ejecutando programas mínimos. Úsala SIEMPRE que se lea,
  escriba, modifique o depure código .ana en este proyecto, o se responda una
  pregunta sobre cómo funciona Anlaco ("ana"). También cuando el usuario
  mencione "ana", "anlaco", "trocea", "fichas" o pegue código que se lee como
  español con "escribe", "si ...:", "fin". Regla central: lo que la skill no
  responda, se comprueba con un programa mínimo — nunca se contesta de
  memoria.
---

# Anlaco — referencia y protocolo de verificación

Anlaco es un **lenguaje natural controlado**: un punto medio entre la máquina
y el humano — fácil de entender sin saber programar, con palabras de tu
idioma (español o inglés), pero cada construcción tiene exactamente UNA forma
gramatical.

Este repo (`anvil`) es un PROYECTO escrito en Anlaco, no el lenguaje en sí.
La fuente canónica del lenguaje —especificación, compilador, oráculo Python
de referencia— vive en el repo hermano **`../anlaco-lang`**. Este repo trae
su propia herramienta (`bin/anac`, ver `bin/VERSION.md` para de qué commit de
`anlaco-lang` viene) para no depender de tenerlo instalado aparte.

## Fuentes canónicas (en `../anlaco-lang`, en este orden)

| Pregunta sobre... | Mira en |
|---|---|
| Sintaxis, semántica, gramática EBNF | `../anlaco-lang/espec/especificacion.md` (NORMATIVA; §5.1 isla de cálculo, §11 gramática) |
| Programas de ejemplo con su salida esperada | `../anlaco-lang/espec/ejemplos/*.ana` + `.out` |
| Qué le falta al lenguaje y por qué | `../anlaco-lang/src/BITACORA.md` |
| Qué queda fuera a propósito | `../anlaco-lang/espec/especificacion.md` §12 + `espec/ideas-futuras.md` |

La especificación manda sobre esta skill: si contradicen algo, gana la espec.

## Cómo ejecutar (desde la raíz de ESTE repo, `anvil`)

```bash
bin/anac ejecutar programa.ana               # interpreta y corre — como Python
bin/anac compilar programa.ana               # Ana → programa.wat (texto wasm)
bin/anac ensamblar programa.wat              # .wat → programa.wasm (binario)
bin/anac empaquetar programa.ana [-o nombre] # Ana → ejecutable nativo standalone,
                                              # ya no necesita a anac para correr
                                              # (necesita bin/anac-stub al lado)
```

Para dudas de verdad finas (comparar los dos motores del lenguaje, no solo
correr un programa), el oráculo Python vive en `../anlaco-lang/oraculo/` —
requiere Python 3.11+ y `PYTHONPATH=../anlaco-lang/oraculo python -m anlaco
...`, ejecutado desde `../anlaco-lang`. No hace falta para el día a día:
`bin/anac ejecutar` ES el intérprete, no una imitación.

## Protocolo empírico — la regla de oro de esta skill

Si una pregunta no se responde con esta skill ni con la espec, **no adivines**:

1. Escribe el programa mínimo que la decide (2-6 líneas).
2. Pásalo por `bin/anac ejecutar`.
3. Si la duda es de semántica (no de mensajes de error), verifica también con
   `bin/anac compilar` + `bin/anac ensamblar` + wasmtime (o directamente
   `bin/anac empaquetar` y correr el resultado). Intérprete y compilador
   deben coincidir.
4. Si el lenguaje NO puede hacer algo y duele para este proyecto, es un
   candidato a reportar en `../anlaco-lang` (`src/BITACORA.md`), no a
   inventar sintaxis nueva aquí.

**Nunca inventes sintaxis nueva** para salir de un apuro: el lenguaje solo
crece por la bitácora de `anlaco-lang` (método Wirth). Escribe el rodeo con
lo que hay.

## Chuleta (v0.2 + v0.3)

```
# comentario                          # idioma: es  (primera línea, opcional)
el precio es 100                      # asignación; "son" para plurales
la lista es ["pan", 3.14, verdadero]  # índices DESDE 1
escribe "Total: {precio + 21} €"      # interpolación: {} evalúa; {{ }} = llave literal
el iva es (precio * 0.21)             # TODO cálculo va entre paréntesis (isla)
si precio es mayor que 50:            # comparaciones siempre con palabras
    escribe "caro"
si no, si precio es 50:               # cadena con coma obligatoria
    escribe "justo"
si no:
    escribe "barato"
fin                                   # UN solo fin cierra toda la cadena
para cada letra en "Añil":            # un texto es la lista de sus letras
    escribe letra
fin
para cada n del 1 al 10:
mientras n es menor que 5:
repite 3 veces:
define media con un a_, un b_:        # llamada: media con 4, 6  (comas, nunca "y")
                                      # estilo v0.2: el indefinido presenta el parámetro
    devuelve ((a_ + b_) / 2)
fin
guarda "hola" en "notas.txt"          # archivos
añade "adiós" a "notas.txt"           # a texto = archivo; a lista = elemento
el t es contenido de "notas.txt"
detente con "El número no es válido"  # para el programa; mensaje a stderr, código 1
la edad es respuesta a "¿años?"       # entrada; "15" se vuelve número solo
elemento 2 de lista, primero de, último de, cantidad de, al azar entre 1 y 6
usa "lexer"                           # módulos — trae lexer.ana (misma carpeta)
las fichas son lexer.trocea con código  # SIEMPRE cualificado con punto; carga 1 vez
el archivo es elemento 1 de los argumentos  # los de la línea de comandos
un punto tiene:                      # tarjetas (registros) — declara un tipo
    una x
    una altitud
fin
el p es un nuevo punto con la x 3, la altitud 4   # construye una tarjeta
escribe p.x                          # acceso a campo; el punto NO se encadena
el m es resto de 17 entre 5          # v0.3: división entera de suelo (2)
el q es cociente de 17 entre 5       # (3)
los b son bytes de "hola"            # v0.3: bytes UTF-8, lista de 0-255
guarda los bytes b en "salida.bin"   # v0.3: escritura binaria, sin decodificar
```

Los argumentos: `los argumentos` es la lista de textos que siguen al
programa (`bin/anac ejecutar prog.ana 7 hola` → `["7", "hola"]`; sin el
nombre del programa; siempre textos; una lista nueva cada vez).

Escapes en textos (lista CERRADA): `\"` `\n` `\\`. Nada más tras la barra.

## Gotchas — lo que sorprende al que viene de otros lenguajes

- **La isla de cálculo**: `el x es precio * 2` es ERROR; se escribe
  `el x es (precio * 2)`. Dentro de `{...}` de interpolación no hacen falta
  paréntesis. `más/menos/por/entre` son alias de `+ - * /` (canónico: símbolos).
- **Los índices empiezan en 1.**
- **La sangría NO significa nada**: la estructura la dan `:` y `fin`.
- **Los dos registros**: todo programa puede escribirse LLANO
  (`variable es 3`, `define área con ancho, alto:`) o ADORNADO con artículos
  (`la variable es 3`, `con un ancho`). Ambos son el MISMO árbol. Ninguna
  frase EXIGE azúcar para funcionar. La IA escribe con azúcar; los
  programadores pueden ir secos.
- **Una sentencia por línea** y **una lista no puede partirse en varias
  líneas**. Tablas grandes = una línea larga.
- **`es` es dos cosas**: al inicio de sentencia, asignación; en condición,
  igualdad. No hay asignación dentro de expresiones.
- **Sin truthiness**: las condiciones exigen booleano (una variable booleana
  sola sí vale: `si carnet:`). No hay `no` suelto: la negación es `no es`.
- **Palabras reservadas traicioneras**: `a` y `veces` (y todas las keywords y
  artículos) no valen como nombres de variable.
- **Llamadas**: `f con x, y`. Una llamada como argumento de otra o como
  elemento de lista literal va entre paréntesis: `f con (g con 1)`, `[(f con 1), 2]`.
  Dentro de un cálculo no hace falta: `(fib con (n - 1) + fib con (n - 2))`.
  El compilador WASM aún NO compila una llamada suelta como sentencia
  (`f con x` sin asignar → error amable; el intérprete sí la acepta).
  Rodeo portable: `el _ es f con x`.
- **Funciones solo en el nivel superior**. Las globales se LEEN desde una
  función; escribirlas es error. Recursión: mínimo 1000 niveles garantizados.
- **`añade X a Y`** depende del tipo de Y: lista → elemento; texto → archivo.
  Los textos son inmutables (concatenar = interpolación: `"{a}{b}"`, ojo O(n²)).
- **Comparar tipos distintos con `es` da `falso`**, no error (salvo entero vs
  decimal, que comparan numéricamente). `(7 / 2)` es `3.5`; `21.0` se muestra `21`.
- **Módulos**: `usa "X"` ejecuta X.ana al importar (¡sin escribe de demo en
  bibliotecas!) y todo se usa cualificado: `X.func con args`, `X.tabla`. NO
  hay import plano. Dentro del módulo, sus funciones se llaman sin
  cualificar. Gotcha de compilación: los accesos cualificados (`M.x`) solo
  son fiables después de que un `usa "M"` se haya EJECUTADO antes en el
  programa.
- **Tarjetas (registros con campos)**: `un punto tiene:` … `fin` declara un
  tipo (nivel superior, hermana de `define`); `un nuevo punto con la x 3`
  construye (los campos no rellenados valen `nada`, cualquier orden); `p.x`
  lee un campo. El punto **NO se encadena**: para anidar, guarda en una
  palabra (`la izq es raiz.izquierda` … `izq.clase`).
- **No hay** (a propósito): diccionarios, negación suelta, textos
  multilínea, excepciones. (`detente con` para el programa: mensaje a
  stderr, código 1. A diferencia de `devuelve` —que solo sale de la
  función— `detente` para todo el programa, se ejecute donde se ejecute.)
- **«Ana de máquina» (v0.3)**: `resto de`/`cociente de` son división entera
  de SUELO (como Python: `resto de -1 entre 128` es `127`, no `-1`). Una tira
  de bytes NO es un tipo nuevo: `bytes de "Añil"` es una LISTA de enteros
  0-255 por BYTE UTF-8 (`[65, 195, 177, 105, 108]`, 5 elementos), a diferencia
  de `cantidad de`/`elemento N de`, que cuentan LETRAS (`cantidad de "Añil"`
  es `4`). `guarda los bytes B en RUTA` escribe binario sin pasar por
  `mostrar`. El compilador WASM exige que el divisor de `resto`/`cociente`
  quepa en 32 bits; el intérprete no tiene ese límite.
- **Multilingüe**: el idioma se detecta solo o se fija con `# idioma: es`.
  Frases multi-palabra: gana la más larga (`es mayor o igual que` antes que `es`).

## Sobre `bin/anac` — qué es y qué no es

`bin/anac` es un binario nativo (Rust + wasmtime enlazado estático) que
embebe el compilador/intérprete de Ana **escrito en Ana** — Ana se compila a
sí misma; el detalle vive en `../anlaco-lang` (`src/*.ana`, autoalojado,
punto fijo verificado). Cuatro verbos:

- `ejecutar`/`compilar`/`ensamblar` reenvían el argv directo a ese
  compilador embebido — son el mismo código Ana en los tres casos, con
  distinto modo.
- `empaquetar` es distinto: es lógica **solo de este host en Rust**, no
  existe dentro del compilador autoalojado. Compila+ensambla con el
  compilador embebido, precompila el `.wasm` resultante a código máquina, y
  lo pega a `bin/anac-stub` (un binario genérico, sin compilador dentro) para
  producir el ejecutable final. Por eso un programa empaquetado con
  `bin/anac empaquetar` sabe ejecutarse a sí mismo, pero si ESE programa
  fuera a su vez el propio `anac.ana`, el resultado sabría `ejecutar`/
  `compilar`/`ensamblar` pero NO `empaquetar` — esa pieza no se hereda,
  porque no es Ana, es Rust. (Confirmado y con banco de pruebas en
  `../anlaco-lang/native/anac/verifica_empaquetar.py`.)

Por ahora `empaquetar` produce binarios para la máquina donde corre — sin
compilación cruzada todavía.
