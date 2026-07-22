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

**Ya conseguido (2026-07-22): un cliente `grpcio` de Python real, sin
modificar, completó una llamada unaria de punta a punta contra un
servidor escrito a mano en Ana** — ver `prueba_interop/`. No fue trivial:
el cliente real usa una representación de HPACK que no habíamos
contemplado, y se encontró precisamente por probar contra tráfico real
en vez de solo contra nuestros propios tests (ver más abajo).

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
- `ejemplos/servidor_handshake.ana` + `ejemplos/cliente_handshake.ana` —
  primer spike que toca un socket TCP real: dos procesos Ana
  independientes hacen el saludo inicial de HTTP/2 (el cliente manda el
  preface de 24 bytes `PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n`, cada lado manda
  su frame SETTINGS con `http2.cabecera_frame`, el servidor contesta con
  el ACK). Verificado de punta a punta por loopback: el servidor confirma
  `preface coincide: verdadero` y ambos lados leen bien el tipo y la
  marca del frame que les llega. Correr con (servidor primero, en
  segundo plano):
  ```
  cd grpc && ../bin/anac ejecutar ejemplos/servidor_handshake.ana &
  cd grpc && ../bin/anac ejecutar ejemplos/cliente_handshake.ana
  ```
- `ejemplos/spike_interop_real.ana` — decodifica, con nuestro propio
  `hpack.ana`, los 225 bytes de cabecera EXACTOS que mandó un cliente
  `grpc-python` de verdad (capturados y documentados en
  `prueba_interop/`). No es un ejemplo inventado: es tráfico real.
- `prueba_interop/` — la prueba completa: un cliente `grpcio` de Python
  sin modificar habla con `servidor_saludador.ana` (un servidor gRPC
  mínimo escrito en Ana con las piezas de arriba) y completa una llamada
  unaria real, con respuesta correcta. Ver `prueba_interop/README.md`
  para reproducirlo.

## Qué se aprendió probando contra un cliente real

Nuestro `hpack.codifica_campo` produce cabeceras con "Literal Header
Field without Indexing" (prefijo de 4 bits). Un cliente gRPC real
(`grpc-python`, sobre `grpc-c`/nghttp2) manda casi todo con **"Literal
Header Field with Incremental Indexing"** (prefijo de 6 bits, patrón
`0x40`) — una representación que `decodifica_campo` no entendía. Sin
probar contra tráfico real esto no se habría visto: nuestros propios
tests, al decodificar solo lo que nosotros mismos codificábamos, nunca
iban a generar ese patrón. Arreglado en `hpack.ana` — ver el comentario
junto a `decodifica_campo`.

## Qué falta (todavía nada de esto existe)

- **Tabla dinámica** de HPACK — sin empezar. No bloqueó la prueba de
  interoperabilidad (decodificar no depende de mantener la tabla), pero
  hace falta para acercarse al tamaño real que manda un cliente/servidor
  gRPC de verdad en peticiones sucesivas de la misma conexión.
- El resto del framing de streams HTTP/2 más allá de una llamada unaria
  de un solo stream — control de flujo de verdad (hoy se ignoran los
  `WINDOW_UPDATE` que manda el cliente), múltiples streams concurrentes,
  mensajes más grandes que un frame.
- Serializar mensajes protobuf a partir de tipos con nombre —
  `servidor_saludador.ana` decodifica/codifica el único campo string que
  necesita a mano; no hay todavía un serializador general para mensajes
  con varios campos y tipos.
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
