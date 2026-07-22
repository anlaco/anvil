---
name: ana
description: >-
  Referencia del lenguaje Ana (archivos .ana, también llamado ana-lang) y
  protocolo para verificar cualquier duda ejecutando programas mínimos con
  bin/anac. Úsala SIEMPRE que se lea, escriba, modifique o depure código .ana
  en este proyecto, o se responda una pregunta sobre cómo funciona Ana.
  También cuando el usuario mencione "ana" o pegue código que se lee como
  español con "escribe", "si ...:", "fin". Regla central: lo que la skill no
  responda, se comprueba con un programa mínimo — nunca se contesta de
  memoria. Si Ana no puede hacer algo que este proyecto necesita, NO se
  intenta arreglar el lenguaje desde aquí: se abre un issue (ver la sección
  "Cuando Ana no llega").
---

# Ana — referencia y protocolo de verificación

Ana (también "ana-lang") es un **lenguaje natural controlado**: un punto
medio entre la máquina y el humano — fácil de entender sin saber programar,
con palabras en español, pero cada construcción tiene exactamente UNA forma
gramatical.

Este proyecto (`anvil`) está escrito en Ana. El lenguaje lo desarrolla **el
equipo de Ana**, un equipo independiente que hace crecer Ana según lo que le
piden sus clientes — de los cuales este proyecto es uno. La relación es de
cliente a proveedor: **aquí se pide, ellos deciden cómo y cuándo.** Este
repo no tiene ni necesita acceso al lenguaje en sí — solo a la herramienta
ya compilada (`bin/anac`) y a esta guía.

**Versión presente**: v0.26.5 (ver `bin/VERSION.md` para el commit exacto).
Trae **red TCP** y el lenguaje **v0.4** (con cambios que ROMPEN código de
versiones anteriores — ver la sección siguiente).

Dos límites de ESTE binario (`bin/anac` es el build self-hosted, no el
oráculo Python del equipo de Ana):
- **Solo español.** El inglés (`write`, `if`) NO funciona aquí, ni con
  `# idioma: en`. Escribe Ana en español.
- **Sin diccionarios.** Los `{clave: valor}` son del oráculo; `bin/anac` no
  los soporta (su lexer no tiene `{`/`}`). No los uses.

## Cómo ejecutar (desde la raíz de este repo)

```bash
bin/anac ejecutar programa.ana               # interpreta y corre — como Python
bin/anac compilar programa.ana               # Ana → programa.wat (texto wasm)
bin/anac ensamblar programa.wat              # .wat → programa.wasm (binario)
bin/anac empaquetar programa.ana [-o nombre] # Ana → ejecutable nativo standalone,
                                              # ya no necesita a anac para correr
                                              # (necesita bin/anac-stub al lado)
```

`empaquetar` es lo más útil para distribuir algo de este proyecto: produce
un binario que arranca en milisegundos y no depende de tener `anac`
instalado. Por ahora empaqueta para la máquina donde corre — sin
compilación cruzada todavía. Un binario empaquetado que usa red lleva dentro
un puente `wasi:sockets` de verdad.

## Protocolo empírico — la regla de oro de esta skill

Si una pregunta sobre el lenguaje no se responde con esta guía, **no
adivines**:

1. Escribe el programa mínimo que la decide (2-6 líneas).
2. Pásalo por `bin/anac ejecutar`. Si la duda es de semántica más fina,
   verifica también con `bin/anac compilar` + `bin/anac ensamblar` (o
   `bin/anac empaquetar` y correr el resultado): deben coincidir.
3. Con la respuesta en la mano, sigue trabajando con lo que el lenguaje
   permite hoy.

**Nunca inventes sintaxis nueva** para salir de un apuro, y nunca toques
nada fuera de este repo para conseguirlo. Si de verdad hace falta algo que
Ana no tiene, es el caso de la sección "Cuando Ana no llega".

## v0.4: cambios que ROMPEN código de versiones anteriores

Si hay código `.ana` en este repo escrito para una versión anterior, migra
con estos puntos (verifícalo con `bin/anac ejecutar`, no de memoria):

- **La forma `con` de FUNCIONES se eliminó.** `define f con a, b:` y la
  llamada `f con x, y` dan error amable. Migración (la elige lo que la
  función DEVUELVE, ver "Formas de define"):
  - devuelve un valor → forma `de`: `define f de a, b:` / `f de x, y`.
  - no devuelve nada (acción) → forma `a`/`en`: `define registra un paso en
    una secuencia:` / `registra x en s`.
  - devuelve booleano → forma `?`: `define una caja contiene? una buscada:`
    / `[1,2,3] contiene 2`.
  - **Ojo**: `con` SIGUE viva para CONSTRUIR tarjetas (`un nuevo punto con
    la x 3`). Solo desapareció en funciones.
- **Comparar tipos distintos con `es` ahora da ERROR de ejecución**, no
  `falso` en silencio. `si 3 es "3":` revienta con "No se puede comparar…".
  Excepciones: entero vs decimal (comparan numéricamente) y cualquier cosa
  contra `nada` (siempre vale, para `si X es nada:`).
- **`nada` sustituye al centinela `-1`** que devolvían algunas frases.
- **`a`/`veces` ya valen como nombre de parámetro** en la forma `de`.
- **Rutas relativas al fuente** (no al directorio de trabajo) en varios
  sitios; y el issue #4 (llamada cualificada tras `usa` con subcarpeta) está
  ARREGLADO — ya no hace falta esquivar rutas con subcarpeta.

## Cuando Ana no llega: reportar, no arreglar

Este proyecto y el lenguaje se desarrollan **por separado**, y no comparten
ningún archivo — la comunicación entre `anvil` y el equipo de Ana es un
issue, como con cualquier proveedor. Si algo que `anvil` necesita no se
puede expresar en Ana (verificado con el protocolo empírico de arriba, no
de memoria):

1. **No se modifica el lenguaje desde aquí.** Este repo no tiene ni el
   código fuente de Ana ni motivo para tenerlo.
2. Se abre un issue en el repositorio del equipo de Ana:
   ```bash
   gh issue create --repo anlaco/anlaco-lang \
     --title "[anvil] título corto de la necesidad" \
     --body "Qué necesita anvil: ...
   Por qué Ana no lo cubre hoy (verificado con): ...
   Programa mínimo que lo demuestra:
   \`\`\`
   ...
   \`\`\`"
   ```
   El prefijo `[anvil]` en el título es lo que identifica que el pedido
   viene de este proyecto — no hay más acoplamiento que ese.
3. Se sigue trabajando con lo que el lenguaje permite — un rodeo, no un
   bloqueo. El equipo de Ana decide, en su propio tiempo y en su propio
   proceso, si esa necesidad entra al lenguaje y cómo; el seguimiento pasa
   por el propio issue (comentarios, cierre), no por nada de este repo.

Esta separación es intencional: evita que arreglar un problema puntual de
`anvil` derive en tocar un lenguaje que usan otros proyectos, y evita
también un archivo compartido que los dos lados tendrían que mantener
sincronizado a mano.

## Chuleta

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
define media de un a_, un b_:         # forma "de" (devuelve valor); llamada: media de 4, 6
    devuelve ((a_ + b_) / 2)          # (comas entre argumentos, nunca "y")
fin
define registra un x en una lista:    # forma "a"/"en" (acción, no devuelve); registra 5 en l
    añade x a lista
fin
guarda "hola" en "notas.txt"          # archivos
añade "adiós" a "notas.txt"           # a texto = archivo; a lista = elemento
el t es contenido de "notas.txt"
detente con "El número no es válido"  # para el programa; mensaje a stderr, código 1
la edad es respuesta a "¿años?"       # entrada; "15" se vuelve número solo
elemento 2 de lista, primero de, último de, cantidad de, al azar entre 1 y 6
usa "modelo"                          # módulos — trae modelo.ana (misma carpeta)
las fichas son lexer.trocea de código # llamada cualificada: SIEMPRE con punto y forma "de"
el archivo es elemento 1 de los argumentos  # los de la línea de comandos
un punto tiene:                       # tarjetas (registros) — declara un tipo
    una x
    una altitud
fin
el p es un nuevo punto con la x 3, la altitud 4   # construye ("con" SÍ vale aquí)
escribe p.x                           # acceso a campo; el punto NO se encadena
el m es resto de 17 entre 5           # división entera de suelo (2)
el q es cociente de 17 entre 5        # (3)
los b son bytes de "hola"             # bytes UTF-8, lista de 0-255
guarda los bytes b en "salida.bin"    # escritura binaria, sin decodificar
```

Los argumentos: `los argumentos` es la lista de textos que siguen al
programa (`bin/anac ejecutar prog.ana 7 hola` → `["7", "hola"]`; sin el
nombre del programa; siempre textos; una lista nueva cada vez).

Escapes en textos (lista CERRADA): `\"` `\n` `\\`. Nada más tras la barra.

## Formas de `define` — la elige el tipo de devolución, no el gusto

| forma | devuelve | definición | llamada |
|---|---|---|---|
| sin parámetros | lo que sea | `define saluda:` | `saluda` |
| `de` | un valor | `define doble de n:` | `doble de 3` |
| `a` / `en` | nada (acción) | `define registra un paso en una secuencia:` | `registra x en s` |
| infija `?` | booleano | `define una caja contiene? una buscada:` | `[1,2,3] contiene 2` |

Una función, una forma. El artículo indefinido presenta cada parámetro:
`define área de un ancho, una altura`. `de` acepta tantos parámetros como
haga falta (1-3 por legibilidad). La forma `con` ya NO existe (v0.4).

## Red (TCP) — lo que anvil pidió

Servidor:
```
el servidor es la escucha del puerto 8099   # asa, o "nada" si no pudo
si servidor es nada:
    detente con "no se pudo escuchar"
fin
la conexion es la aceptación de servidor    # BLOQUEA hasta que llega un cliente; asa o nada
los datos son bytes recibidos de conexion   # lista de bytes (0-255)
envía datos a conexion                       # acción
cierra conexion
cierra servidor
```
Cliente:
```
la conexion es la conexión a "127.0.0.1" en el puerto 8099   # asa, o "nada" si falló
```
Las asas son enteros opacos. `envía` toma una lista de bytes (usa `bytes de
"texto"` para convertir). Verificado que `la escucha del puerto` devuelve un
socket real con este `bin/anac`. Ejemplo completo de eco: pídelo al equipo
de Ana o mira un servidor mínimo con el patrón de arriba.

## Gotchas — lo que sorprende al que viene de otros lenguajes

- **La isla de cálculo**: `el x es precio * 2` es ERROR; se escribe
  `el x es (precio * 2)`. Dentro de `{...}` de interpolación no hacen falta
  paréntesis. `más/menos/por/entre` son alias de `+ - * /` (canónico: símbolos).
- **Los índices empiezan en 1.**
- **La sangría NO significa nada**: la estructura la dan `:` y `fin`.
- **Los dos registros**: todo programa puede escribirse LLANO
  (`variable es 3`, `define área de ancho, alto:`) o ADORNADO con artículos
  (`la variable es 3`, `de un ancho`). Ambos son el MISMO árbol. Ninguna
  frase EXIGE azúcar para funcionar.
- **Una sentencia por línea**, salvo dentro de un paréntesis o corchete sin
  cerrar: una lista literal o un cálculo SÍ pueden partirse en varias líneas
  (v0.4). Una llamada en forma `de` (sin paréntesis propios) no se parte.
- **`es` es dos cosas**: al inicio de sentencia, asignación; en condición,
  igualdad. No hay asignación dentro de expresiones.
- **Sin truthiness**: las condiciones exigen booleano (una variable booleana
  sola sí vale: `si carnet:`). No hay `no` suelto: la negación es `no es`.
- **Llamadas** (v0.4): la sintaxis depende de la forma — `f de x`, `f a x` /
  `f x en y`, `x f y` (infija `?`). Una llamada como argumento de otra o como
  elemento de lista literal va entre paréntesis: `fib de (fib de 4)`,
  `[(fib de 3), 2]`. Dentro de un cálculo no hace falta: `(fib de (n - 1) +
  fib de (n - 2))`.
- **Funciones solo en el nivel superior**. Las globales se LEEN desde una
  función; escribirlas es error. Recursión: mínimo 1000 niveles garantizados.
- **`añade X a Y`** depende del tipo de Y: lista → elemento; texto → archivo.
  Los textos son inmutables (concatenar = interpolación: `"{a}{b}"`, ojo O(n²)).
- **Comparar tipos distintos con `es` da ERROR** (v0.4), no `falso` — salvo
  entero vs decimal (numérico) y cualquier cosa contra `nada`. `(7 / 2)` es
  `3.5`; `21.0` se muestra `21`.
- **Módulos**: `usa "X"` ejecuta X.ana al importar (¡sin escribe de demo en
  bibliotecas!) y todo se usa cualificado con la forma de la función:
  `X.func de args`, `X.tabla`. NO hay import plano. Dentro del módulo, sus
  funciones se llaman sin cualificar. El issue #4 (llamada cualificada tras
  `usa` con subcarpeta) está ARREGLADO: los módulos se cachean y buscan por
  su nombre corto. **Las tarjetas de un módulo importado se construyen SIN
  cualificar** (`un nuevo resultado_step con ...`, no `un nuevo
  M.resultado_step con ...`), a diferencia de funciones y tablas.
- **No hay funciones de primera clase.** Un nombre de función usado como
  valor (p. ej. `la f es saluda`) no la referencia: la AUTOINVOCA con cero
  argumentos. No hay forma de pasar una función como valor — el despacho
  dinámico se hace por nombre de texto con una cadena `si/si no`.
- **Tarjetas (registros con campos)**: `un punto tiene:` … `fin` declara un
  tipo (nivel superior); `un nuevo punto con la x 3` construye (campos no
  rellenados = `nada`, cualquier orden; `con` SÍ vale aquí); `p.x` lee un
  campo. El punto **NO se encadena**: para anidar, guarda en una palabra
  (`la izq es raiz.izquierda` … `izq.clase`).
  **Los campos escalares NO se pueden reasignar tras construir** — no existe
  `objeto.campo es valor` como mutación: se parsea como reasignar la TARJETA
  ENTERA al booleano de comparar `objeto.campo es valor` (footgun silencioso;
  `objeto` se vuelve `verdadero`/`falso`). Excepción: un **campo lista** SÍ es
  mutable en el sitio vía `añade elemento a objeto.campo` (la lista es un
  objeto compartido; el cambio se ve aun pasando la tarjeta a una función).
  Para "cambiar" un campo escalar, construye una tarjeta nueva.
- **No hay** (a propósito, o no en este binario): diccionarios `{}` (oráculo
  solo), inglés (oráculo solo), negación suelta, textos multilínea,
  excepciones. `detente con` sí para el programa (stderr, código 1); a
  diferencia de `devuelve` (solo sale de la función), `detente` para todo.
- **Aritmética de bytes**: `resto de`/`cociente de` son división entera de
  SUELO (`resto de -1 entre 128` es `127`). `bytes de "Añil"` es una LISTA de
  enteros 0-255 por BYTE UTF-8 (`[65, 195, 177, 105, 108]`, 5 elementos), a
  diferencia de `cantidad de`/`elemento N de`, que cuentan LETRAS (`cantidad
  de "Añil"` es `4`). El compilador exige que el divisor quepa en 32 bits.
- **Multilingüe (solo en el oráculo, no aquí)**: `bin/anac` es solo español.

## Sobre `bin/anac`

Cuatro verbos: `ejecutar`/`compilar`/`ensamblar` son el mismo compilador de
Ana en tres modos. `empaquetar` es distinto — es la única pieza que no vive
en el lenguaje: compila, ensambla, precompila a código máquina y pega el
resultado a `bin/anac-stub` (un anfitrión genérico) para dar un ejecutable
standalone. Detalle relevante: un programa empaquetado hereda
`ejecutar`/`compilar`/`ensamblar` si los tenía, pero nunca hereda
`empaquetar` — esa capacidad es de la herramienta, no del programa.
