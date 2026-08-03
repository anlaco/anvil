# Ejecutor de pasos en Python (ADR-0012)

Módulo distribuido de Anvil: un **ejecutor de lenguaje** que habla el mismo
contrato gRPC (`paso.proto`) que el ejecutor WASM embebido. El motor lo ve
como un endpoint más; no sabe que detrás hay Python. Es el primero de la
familia `executores/` (LabVIEW, MATLAB, … futuros), licencia **Apache-2.0**
(adoptable y extensible, [ADR-0012](../../docs/adr/0012-executores-de-lenguaje-como-modulos.md)).

## Qué demuestra

El escenario que motivó ADR-0012: un paso que toca hardware no tiene por qué
vivir en el SO de Anvil. Aquí los pasos de instrumento hablan **por TCP con
un simulador de instrumento**. En producción ese destino puede ser una caja
con los drivers del fabricante en un Windows 7 (**LID**), una VM, o el
simulador real que está desarrollando otro equipo — para Anvil todo es "un
endpoint gRPC en una IP".

## Requisitos

- Python 3.10+
- `grpcio` (para correr el ejecutor):

```sh
pip install grpcio
```

- `grpcio-tools` (solo para regenerar los stubs si cambia `paso.proto`):

```sh
pip install grpcio-tools
```

## Generar los stubs

Los ficheros `paso_pb2.py` y `paso_pb2_grpc.py` están **gitignored** (son
generados). Tras clonar el repo —o si cambia `paso.proto`— regenerarlos:

```sh
python3 -m grpc_tools.protoc \
  -I ../../crates/modelo \
  --python_out=. --grpc_python_out=. \
  ../../crates/modelo/paso.proto
```

## Correrlo

### Opción A — directo (desarrollo, sin aislar)

```sh
# terminal 1 — simulador TCP (stand-in del equipo del simulador)
python3 simulador_tcp.py

# terminal 2 — ejecutor de pasos en Python
python3 server.py                 # 127.0.0.1:9101

# (variante LID: apuntar el simulador a la caja legacy)
python3 server.py --simulador 192.168.1.50:4000
```

### Opción B — como LID aislado con Docker (demo del patrón)

El patrón LID completo: el ejecutor corre en un contenedor aislado que solo
puede salir al simulador por una red interna. Anvil (en el host) habla por
gRPC con el contenedor; el contenedor habla por TCP con el simulador. Ni el
LID ni el simulador tienen acceso a Internet ni a la LAN — son las "puertas
declaradas".

```sh
# genera los stubs (si no existen ya)
python3 -m grpc_tools.protoc -I ../../crates/modelo \
  --python_out=. --grpc_python_out=. ../../crates/modelo/paso.proto

# levanta el LID + simulador en una red interna aislada
docker compose up

# en otra terminal: Anvil corre la secuencia que mezcla embebido + LID
./anvil ejemplos/demo_lid.yaml

# parar
docker compose down
```

Topología de la demo:

```
anvil (host Linux)
  ├─ ejecutor embebido (WASM, built-in)  → verificar_led
  └─ gRPC 127.0.0.1:9101 → LID (contenedor Docker, red_lid internal)
                             ├─ medir_simulador / conectar_equipo (gRPC)
                             └─ TCP red_lid → simulador (aislado)
```

- `red_lid` es `internal: true`: el LID y el simulador no salen a Internet
  ni a la LAN. Solo se ven entre sí.
- El LID expone `9101` al host (para que Anvil conecte); el simulador no
  expone nada (solo accesible desde `red_lid`).
- Para el escenario Win10 real (Sandboxie-Plus en vez de Docker): mismo
  patrón, mismo YAML, mismo `ejecutores:` — solo cambia la tecnología del
  aislamiento detrás; ver
  [investigacion/aislamiento-lid.md](../../docs/investigacion/aislamiento-lid.md).

Ver [ejemplos/demo_lid.yaml](../../ejemplos/demo_lid.yaml) para la secuencia.

Pasos que atiende:

| Nombre | Qué hace |
|---|---|
| `conectar_equipo` | Fallo transitorio en el intento 1, pasa desde el 2 (RF-09: el `intento` llega al paso). |
| `medir_simulador` | Mide contra el simulador por TCP; devuelve `valor_medido` (el límite lo evalúa el motor, ADR-0008). |
| `verificar_led` | Pass/fail sin medida. |

Un nombre desconocido devuelve `estado: error`, nunca una excepción (RF-12).

## Usarlo desde Anvil

El motor despacha por **nombre→endpoint** (`ejecutores:` en el YAML, o el
flag `--ejecutor nombre=host:puerto`). Ejemplo con el ejecutor embebido y
este en la misma secuencia:

```yaml
nombre: demo_ejecutores
ejecutores:
  - { nombre: embebido, tipo: embebido }
  - { nombre: python, tipo: grpc, host: 127.0.0.1, puerto: 9101 }
main:
  - nombre: verificar_led          # servido por el ejecutor WASM embebido
  - nombre: medir_simulador, ejecutor: python
    limite: { tipo: rango, min: 4.0, max: 5.0 }
  - nombre: conectar_equipo, ejecutor: python
```

## Notas

- Los stubs (`paso_pb2*.py`) son **generados**: no se editan a mano
  (`.gitignore`). Regenerarlos con el comando de arriba.
- Este módulo usa `grpcio` (gRPC nativo de Python), no `wasi-grpc` — esa
  pila es solo para WASM ([ADR-0006](../../docs/adr/0006-wasi-grpc-propio.md)).
- El contrato con el simulador es deliberadamente trivial (línea de texto).
  Cuando el equipo del simulador cierre su protocolo real, se sustituye
  `lee_simulador()` en `server.py` sin tocar el resto del ejecutor.
