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

## Qué falta (todavía nada de esto existe)

- HPACK (compresión de cabeceras de HTTP/2) — sin empezar.
- El framing completo de streams HTTP/2 (settings, window update,
  control de flujo) — solo está la cabecera de 9 bytes, no el protocolo.
- Codificación de mensajes protobuf reales a partir de un `.proto`
  (campos, tipos, mensajes anidados) — hoy solo hay los primitivos
  (varint, zigzag), no un serializador de mensajes.
- Unir esto con los sockets TCP de Ana (`la escucha del puerto`, `la
  conexión a ... en el puerto`, ver la skill `ana`) para tener un cliente
  o servidor gRPC real hablando por la red.

Si al construir alguna de estas piezas Ana genuinamente no da algo que
haga falta (candidato más probable, señalado por el propio equipo de
Ana: operadores bit a bit), se reporta como issue nuevo y concreto en
`anlaco/anlaco-lang` con el caso mínimo — no se intenta arreglar el
lenguaje desde aquí. Ver `.claude/skills/ana/` para el protocolo.
