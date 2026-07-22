# prueba_interop

Prueba de interoperabilidad real: un cliente gRPC de verdad (`grpcio`,
la librería oficial de Python — sin modificar, sin ningún truco) hablando
contra `servidor_saludador.ana`, un servidor escrito a mano en Ana usando
`protobuf.ana` + `http2.ana` + `hpack.ana` de `grpc/`.

**Resultado verificado el 2026-07-22**: el cliente Python manda
`SaludoRequest(nombre="anvil")` y recibe de vuelta
`SaludoResponse(mensaje="Hola, anvil!")` — un viaje de ida y vuelta
completo, HPACK con Huffman incluido, sobre un socket TCP real.

## Qué hay aquí

- `saludador.proto` — el contrato del servicio de prueba (un método
  unario mínimo).
- `saludador_pb2.py` / `saludador_pb2_grpc.py` — generados por
  `protoc`, NO están versionados (son un artefacto de build, igual que
  `bin/anac` en la raíz del repo). Se regeneran con el comando de abajo.
- `cliente.py` — el cliente real de `grpcio`, sin nada especial.
- `servidor_saludador.ana` — el servidor, en Ana.
- `captura_cruda.py` — un servidor "tonto" en Python puro (sin gRPC) que
  solo hace el saludo mínimo de HTTP/2 y vuelca a `captura.bin` todo lo
  que un cliente real le mande. Así se capturaron los bytes reales que
  ahora viven en `../ejemplos/spike_interop_real.ana`.

## Cómo reproducir

```bash
python3 -m pip install --user grpcio grpcio-tools
cd grpc/prueba_interop
python3 -m grpc_tools.protoc -I. --python_out=. --grpc_python_out=. saludador.proto

# en una terminal:
cd grpc && ../bin/anac ejecutar prueba_interop/servidor_saludador.ana

# en otra:
cd grpc/prueba_interop && python3 cliente.py
```

Debería imprimir `Respuesta: Hola, anvil!`.

## Qué reveló esta prueba

Un cliente gRPC real no manda las cabeceras como las manda nuestro
propio `hpack.codifica_campo` (que usa "Literal Header Field without
Indexing"). `grpc-python` usa **"Literal Header Field with Incremental
Indexing"** (prefijo de 6 bits, patrón `0x40`) para casi todo — una
representación que `decodifica_campo` no entendía todavía. Se encontró
al capturar tráfico real y se arregló en `hpack.ana` (ver comentario en
el propio archivo) — sin eso, el servidor no habría podido leer la
petición de ningún cliente gRPC real, solo la suya propia.
