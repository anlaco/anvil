# grpc (semilla)

Módulo Ana para hablar gRPC (HTTP/2 + protobuf) sobre los sockets TCP que
trae Ana desde v0.4. El equipo de Ana decidió explícitamente no construir
esto en el núcleo del lenguaje (ver issue
[anlaco/anlaco-lang#3](https://github.com/anlaco/anlaco-lang/issues/3),
comentario de cierre): framing HTTP/2 y codificación protobuf son
manipulación de bytes, y eso le toca a quien lo necesite construirlo como
módulo, sobre las primitivas de red y de bytes que Ana sí da.

**Diseñado para vivir separado.** La idea es que este directorio termine
siendo su propio proyecto, no una carpeta de `anvil` para siempre. Por
eso no importa nada de `secuenciador/` ni de nada específico de anvil —
solo depende de `bin/anac` (la herramienta del lenguaje) y de sí mismo.
El día que se extraiga, es copiar la carpeta.

**El objetivo es cumplir el estándar gRPC de verdad**, no un protocolo
propio simplificado: que un cliente gRPC real (Python, Go, lo que sea,
sin tocar) pueda invocar el secuenciador de anvil. Por eso hace falta
HPACK real y no un atajo — cualquier cliente gRPC estándar manda las
cabeceras comprimidas con HPACK desde el primer mensaje.

## Qué hay hoy (spike, no producción)

- `protobuf.ana` — varints sin signo (`varint_bytes de`/`varint_valor
  de`) y zigzag para enteros con signo (`zigzag_de de`/`zigzag_a de`).
  Construido solo con `resto de`/`cociente de` — **no hace falta ningún
  operador de bits**, contra lo que se sospechaba de entrada.
- `http2.ana` — cabecera de frame HTTP/2 (9 bytes: longitud de 24 bits,
  tipo, marca, id de stream de 31 bits), codificación y decodificación.
- `ejemplos/spike_bytes.ana` — demuestra las dos piezas de punta a punta.
  Correr con:
  ```
  cd grpc && ../bin/anac ejecutar ejemplos/spike_bytes.ana
  ```
- `huffman.ana` — Huffman de HPACK (RFC 7541 Apéndice B), tabla completa
  de 257 símbolos. Verificado dos veces: (1) sin colisiones de prefijo
  entre los 257 códigos (comprobado aparte antes de escribir el módulo);
  (2) codificar `"www.example.com"` da exactamente
  `f1e3c2e5f23a6ba0ab90f4ff` — el mismo resultado, byte a byte, del
  ejemplo oficial del RFC (Apéndice C.4.1). La tabla se sacó del texto
  del RFC (`rfc-editor.org/rfc/rfc7541.txt`), no de memoria. Correr con:
  ```
  cd grpc && ../bin/anac ejecutar ejemplos/spike_huffman.ana
  ```
- `hpack.ana` — HPACK (RFC 7541) con **tabla estática completa** (las 61
  entradas fijas del estándar) **y Huffman**: codifica/decodifica campos
  de cabecera como *Indexed Header Field* (nombre+valor exacto en la
  tabla) o *Literal Header Field without Indexing* (nombre indexado +
  valor literal, o ambos literales) — nunca toca la tabla dinámica, así
  que no hace falta llevar estado entre peticiones. Las cadenas usan
  Huffman cuando comprime (como un codificador real) y sin comprimir si
  no; decodifica ambos casos según el bit H. Verificado con el juego
  exacto de cabeceras que manda cualquier petición unaria de gRPC
  (`:method: POST`, `:scheme: http`, `:path`, `content-type:
  application/grpc`, `te: trailers`), round-trip byte a byte. Correr con:
  ```
  cd grpc && ../bin/anac ejecutar ejemplos/spike_hpack.ana
  ```

## Qué falta (todavía nada de esto existe)

- **Tabla dinámica** de HPACK — sin empezar (hace falta para acercarse al
  tamaño real que manda un cliente/servidor gRPC de verdad, y para
  decodificar las representaciones "with incremental indexing" que un
  cliente real puede mandar).
- El framing completo de streams HTTP/2 (settings, window update,
  control de flujo) — solo está la cabecera de 9 bytes, no el protocolo.
- Codificación de mensajes protobuf reales a partir de un `.proto`
  (campos, tipos, mensajes anidados) — hoy solo hay los primitivos
  (varint, zigzag), no un serializador de mensajes.
- Unir esto con los sockets TCP de Ana (`la escucha del puerto`, `la
  conexión a ... en el puerto`, ver la skill `ana`) para tener un cliente
  o servidor gRPC real hablando por la red.
- Servicio de reflexión de gRPC — necesario más adelante para que un
  futuro editor gráfico de secuencias pueda descubrir los métodos de un
  módulo sin compilar un `.proto` a mano (objetivo de producto: arrastrar
  un Python o una DLL como step, ver memoria de visión del proyecto).

**Límite de lenguaje encontrado y reportado:** Ana no tiene hoy manera de
convertir una lista de bytes de vuelta a texto (`bytes de "texto"` va en
un solo sentido) — ver
[anlaco/anlaco-lang#6](https://github.com/anlaco/anlaco-lang/issues/6).
Mientras tanto, `hpack.ana` devuelve los valores decodificados como
LISTA DE BYTES, no como texto Ana (ver comentarios en el propio
`hpack.ana`).

Si al construir alguna de estas piezas Ana genuinamente no da algo que
haga falta (candidato más probable, señalado por el propio equipo de
Ana: operadores bit a bit — aunque de momento ni varint, zigzag ni HPACK
de tabla estática lo han necesitado), se reporta como issue nuevo y
concreto en `anlaco/anlaco-lang` con el caso mínimo — no se intenta
arreglar el lenguaje desde aquí. Ver `.claude/skills/ana/` para el
protocolo.
