# Ejecutor de pasos en Python (ADR-0012)

Módulo distribuido de Anvil: un **ejecutor de lenguaje** que habla el mismo
contrato gRPC (`paso.proto`) que el ejecutor WASM embebido. El motor lo ve
como un endpoint más; no sabe que detrás hay Python. Es el primero de la
familia `executors/` (LabVIEW, MATLAB, … futuros), licencia **Apache-2.0**
(adoptable y extensible, [ADR-0012](../../docs/adr/0012-executores-de-lenguaje-como-modulos.md)).

> **Para añadir un paso no se edita `server.py`.** Se escribe una función, se
> decora con `@step` y se deja el fichero donde apunte `--steps`. El ejecutor
> lo descubre, lo sirve y **se lo describe a Anvil**
> ([ADR-0021](../../docs/adr/0021-el-ejecutor-describe-su-catalogo.md)).

## Escribir un paso

Un paso es una función normal:

```python
# mis_pasos/resistencia.py
from anvil_step import Context, Result, step


@step(outputs={"serie": str})
def medir_resistencia(ctx: Context, canal: float, escala: str = "auto") -> Result:
    """Mide resistencia en un canal, con la escala indicada."""
    ohmios = mi_instrumento.medir(canal, escala)
    return Result.measured(ohmios, outputs={"serie": "R-007"})
```

Y se corre así:

```sh
python3 server.py --steps mis_pasos
```

**La firma es el catálogo.** El nombre de cada parámetro, su tipo y si es
obligatorio salen de la propia función: no se escriben dos veces, así que no
pueden divergir. Eso es lo que permite a Anvil comprobar una secuencia **sin
ejecutarla** (`--validate --with-executors`) y decirte que escribiste `canall`
en vez de `canal` antes de poner la unidad en el banco.

Lo que Python no puede deducir lo toma el decorador:

| | |
|---|---|
| `outputs={"nombre": tipo}` | Las salidas con nombre. Un `return` no lleva nombres, y son las que `assign: result.outputs.<nombre>` lee en la secuencia. |
| `name="..."` | El nombre del paso en la secuencia, cuando no puede ser el de la función. |

Los tipos son **los cuatro del contrato**: `float`/`int` → número, `str` →
texto, `bool` → booleano y `Reference` → referencia (ver más abajo). Un
parámetro **sin anotar** se describe como *sin especificar* y Anvil lo deja sin
comprobar, en vez de suponer que es un número.

### Qué devuelve un paso

`Result.measured(valor, …)` para una medida, `Result.passed(…)` /
`Result.failed(…)` para un pass/fail, `Result.error(…)` cuando no se pudo
juzgar. También valen atajos: devolver un número es una medida, un `bool` es
pass/fail, y `None` es un `pass` sin medida.

**El umbral no es cosa del paso**: devuelve la medida y el motor la juzga contra
el `limit` de la secuencia ([ADR-0008](../../docs/adr/0008-limites-evaluados-por-el-motor.md)).

Una **excepción** dentro de un paso se convierte en `error`, nunca en `fail`: un
paso que revienta no dice nada de la unidad bajo test, y anotarla como unidad
rechazada sería un rojo falso. El ejecutor no se cae (RF-12).

### Objects that stay here: `Reference` and `ctx.objects`

A bench session, an instrument connection, a driver handle: a thing with open
sockets that **cannot cross the wire** and must not be reopened for every step.
Keep it in this process and hand the sequence a handle to it
([ADR-0022](../../docs/adr/0022-la-referencia-a-objeto-es-un-cuarto-tipo-y-nombra-una-ranura.md)):

```python
from anvil_step import Context, Reference, Result, step


@step(outputs={"bench": Reference})
def open_bench(ctx: Context, address: str) -> Result:
    """Opens the session and hands back a handle to it."""
    return Result.passed(outputs={"bench": ctx.objects.new(Bench(address))})


@step(outputs={"bench": Reference})
def set_channel(ctx: Context, bench: Reference, channel: float) -> Result:
    ctx.objects.get(bench).channel = channel
    return Result.passed(outputs={"bench": bench})   # the same handle


@step
def close_bench(ctx: Context, bench: Reference) -> Result:
    ctx.objects.close(bench).close()
    return Result.passed()
```

And in the sequence:

```yaml
locals:
  bench: { type: reference, executor: python }
setup:
  - name: open_bench
    executor: python
    inputs: { address: '127.0.0.1:4000' }
    assign: { bench: result.outputs.bench }
main:
  - name: measure_bench
    executor: python
    inputs: { bench: '${locals.bench}' }
```

**The reference names a slot, not an object.** Changing the bench's state does
**not** change its identity: `set_channel` answers the very handle it was given.
A new one is minted only when another object was really born — deriving one
configuration from another, duplicating. This is not cosmetic: the engine
evaluates a step's parameters **once** and re-sends the same ones on every
retry, so an attempt that handed back a new handle would leave the next attempt
holding one this executor already considers spent. If your language is
by-value (LabVIEW), use `ctx.objects.replace(ref, new)`: new box, same slot,
same handle.

`ctx.objects` is an `ObjectStore`, and it takes care of the two duties **Anvil
cannot check from outside**:

- **it never recycles a key**, not even after a `close`. If a closed bench's key
  came back for the next `open_bench`, an old reference would resolve cleanly to
  a live, **different** object: same executor, same lifetime, everything green,
  measuring against the wrong bench;
- **it mints a new lifetime on every start** and publishes it in the catalog,
  which is what lets Anvil find out that this process died and was born again
  while holding its references.

A reference from another lifetime, from a closed slot or from one that never
existed raises `KeyError`, and the step returns it as `error`. This executor
knows that with certainty; Anvil only by comparison.

### `ctx`, y por qué no es un parámetro

Un paso recibe `ctx` **sólo si lo declara**, y `ctx` nunca aparece en el
catálogo: es el ejecutor hablándole al paso, no un valor que salga de la
secuencia.

| | |
|---|---|
| `ctx.attempt` | El número de intento, desde 1 (RF-09). No lo pone la secuencia. |
| `ctx.options` | Lo que se pasó con `--option clave=valor`: configuración de despliegue (la dirección de un instrumento, el id de un banco). |
| `ctx.step_name` | El nombre con el que se le llamó, útil si una función sirve varios. |
| `ctx.objects` | This executor's slots (see above). It is here and not in the signature because the store belongs to the **process**, not to the measurement. |

La distinción importa: lo que cambia **qué se mide** va en la secuencia, donde
queda escrito en el informe ([ADR-0019](../../docs/adr/0019-que-hace-anvil-cuando-no-puede-juzgar.md),
Regla 3). Lo que cambia **contra qué caja se habla** va en `--option`.

### Dónde se ponen los pasos

Por orden de precedencia:

1. `--steps PATH` — repetible; una carpeta o un `.py` suelto.
2. La variable de entorno `ANVIL_PYTHON_STEPS` (lista separada por `:`).
3. `./steps` en el directorio de trabajo, si existe.

De una carpeta se cargan sus `.py` de primer nivel y sus paquetes; los que
empiezan por `_` se saltan, así un módulo de ayuda no se carga como si fuera de
pasos. La carpeta entra en `sys.path`, de modo que tus pasos pueden importarse
entre ellos.

Una ruta que **no existe** es un fallo de arranque, no un aviso: un ejecutor que
sirve el catálogo vacío por un dedazo en una ruta es un verde falso gratis.

### Ver el catálogo sin levantar nada

```sh
$ python3 server.py --steps mis_pasos --list
medir_resistencia(canal: number, escala: text = 'auto')
    Mide resistencia en un canal, con la escala indicada.
    outputs: serie: text
```

Es la misma información que Anvil obtiene por `Describe`.

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

```sh
# terminal 1 — simulador TCP (stand-in del equipo del simulador)
python3 simulador_tcp.py

# terminal 2 — ejecutor de pasos en Python, con los pasos de ejemplo
python3 server.py                 # ./steps, en 127.0.0.1:9101

# (variante LID: apuntar el simulador a la caja legacy)
python3 server.py --option simulator=192.168.1.50:4000
```

Los pasos que vienen de serie viven en [`steps/instrument.py`](steps/instrument.py)
y **no son especiales**: se descubren como los tuyos y podrías borrarlos sin
tocar `server.py`.

| Nombre | Qué hace |
|---|---|
| `conectar_equipo` | Fallo transitorio en el intento 1, pasa desde el 2 (RF-09: el `intento` llega al paso por `ctx`). |
| `medir_simulador` | Mide contra el simulador por TCP; devuelve la medida y el canal usado (el límite lo evalúa el motor, ADR-0008). |
| `verificar_led` | Pass/fail sin medida. |
| `open_bench` / `configure_bench` / `measure_bench` / `close_bench` | El patrón de objeto de ADR-0022: uno abre y acuña, varios usan, uno cierra. Ver [`ejemplos/referencia.yaml`](../../ejemplos/referencia.yaml). |

Un nombre desconocido devuelve `status: error` con la lista de los que sí sirve,
nunca una excepción (RF-12).

## Usarlo desde Anvil

El motor despacha por **nombre→endpoint** (`executors:` en el YAML, o el flag
`--executor nombre=host:puerto`). Ejemplo con el ejecutor embebido y éste en la
misma secuencia:

```yaml
name: demo_ejecutores
executors:
  - { name: embebido, type: embedded }
  - { name: python, type: grpc, host: 127.0.0.1, port: 9101 }
main:
  - name: verificar_led          # servido por el ejecutor WASM embebido
  - name: medir_simulador
    executor: python
    limit: { type: range, min: 4.0, max: 5.0 }
  - name: conectar_equipo
    executor: python
```

Y para comprobar que la secuencia casa con lo que este ejecutor ofrece, **sin
ejecutar ni un paso**:

```sh
./anvil secuencia.yaml --validate --with-executors
```

## Tests

El SDK se prueba con la biblioteca estándar y **sin gRPC**: lo que se prueba es
la superficie con la que se escribe un paso, no el cable.

```sh
cd executors/python && python3 -m unittest discover -p 'test_*.py'
# o, desde la raíz del repo: make test-executors
```

## Notas

- Los stubs (`paso_pb2*.py`) son **generados**: no se editan a mano
  (`.gitignore`). Regenerarlos con el comando de arriba.
- Este módulo usa `grpcio` (gRPC nativo de Python), no `wasi-grpc` — esa
  pila es solo para WASM ([ADR-0006](../../docs/adr/0006-wasi-grpc-propio.md)).
- El contrato con el simulador es deliberadamente trivial (línea de texto).
  Cuando el equipo del simulador cierre su protocolo real, se sustituye
  `_ask_simulator()` en `steps/instrument.py` sin tocar el resto del ejecutor.
- El código del ejecutor está en inglés (identificadores, comentarios,
  mensajes); los **nombres de paso y de parámetro no se traducen**, porque son
  datos de las secuencias ya escritas.
