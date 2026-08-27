# ADR-0021: El ejecutor describe su catálogo, y quien no pueda tiene que notarse

- **Estado:** Aceptada
- **Fecha:** 2026-08-27
- **Cómo se decidió:** en este repo, sobre el encargo de cerrar los issues
  [#54](https://github.com/anlaco/anvil/issues/54) y
  [#45](https://github.com/anlaco/anvil/issues/45), y sobre el hueco que
  [ADR-0020 §3](0020-parametros-del-paso-en-la-peticion.md) dejó escrito a
  propósito. Todo lo que se afirma del estado de hoy está **verificado
  ejecutando** el código de este repo (§Contexto); lo de la competencia viene de
  las fuentes que cita el `#45` y no se ha vuelto a contrastar aquí.
- **Relaciona:** ADR-0003, ADR-0005, ADR-0012, ADR-0015, ADR-0019, ADR-0020,
  issues #39, #45, #54, [contrato-grpc.md](../contrato-grpc.md),
  [diseno/ui-vs-headless.md](../diseno/ui-vs-headless.md)
- **Alcance:** decide **e implementa** el mecanismo de catálogo y el modo de
  descubrimiento del ejecutor Python. No diseña el editor visual, no toca el
  WIT y no añade lenguajes.

## Contexto

ADR-0020 dio a los pasos parámetros y salidas **con nombre**, y en el mismo
documento apuntó la factura:

> `resultado.salidas.tension` **no es validable al cargar** —el cargador no sabe
> qué devuelve un paso— así que un nombre equivocado es un `error` de ejecución,
> no de carga. Es una excepción a la regla de detección de ADR-0019, y es
> inevitable **mientras no exista introspección de firma**.

Esa excepción es la que se cierra aquí. Y no es teórica: verificado el
2026-08-27 en este repo, una secuencia con `canall` en vez de `canal` y con
`assign: result.outputs.temperaturaa` cargaba sin una sola queja y sólo se
rompía —o peor, medía otra cosa— con la unidad ya en el banco.

Hay un segundo hueco, y es el del `#54`. La [decisión 0011 de
dirección](../../../00-DIRECCION/decisiones/0011-los-ejecutores-de-lenguaje-son-producto-descargable.md)
fija que un ejecutor de lenguaje es **producto descargable**: quien quiera pasos
en Python lo descarga, pone sus pasos donde diga la documentación, y no edita ni
una línea de nuestro código. `executors/python/server.py` no lo cumplía:
despachaba con un `if/elif` de tres casos escritos a mano, así que aportar un
paso propio era editar ese fichero — «recompilar el adapter», que es justo el
dolor de TestStand del que presume no tener ADR-0003.

Los dos huecos son el mismo problema visto por sus dos caras: **nada en el
sistema sabía qué pasos existen**. Ni Anvil para validarlos, ni el ejecutor para
servirlos sin que alguien los escribiera a mano en un `match`. Por eso se
cierran en la misma decisión.

Y un tercer hallazgo, que salió al reproducir: el ejecutor Python **estaba roto
desde el commit de traducción al inglés** (`579f468`). Leía `request.nombre` y
`request.intento`, campos que el contrato ya no tiene. Que nadie se enterara
durante días es el argumento más fuerte a favor de lo que se decide aquí: un
ejecutor que no se puede interrogar tampoco se puede comprobar.

## Decisión

### 1 — Se le pregunta al ejecutor; no se inspecciona su artefacto

Un RPC nuevo en el mismo servicio, `Describe(CatalogRequest) → Catalog`.

**TestStand lee el conector del VI** y se queda sin gasolina en cuanto el
artefacto no lleva metadatos: con una DLL de C sin type information, *«you will
have to manually specify the prototype»*. **OpenTAP** lo resuelve por
construcción, reflejando las propiedades públicas de la clase C# — y ése es
exactamente el mecanismo que lo ata a .NET.

Preguntar no tiene esa frontera: funciona igual con WASM, con Python, con una
caja en otra sala y con lo que venga. Es la única forma compatible con la tesis
de ADR-0003 —cualquier lenguaje que hable gRPC es un adapter— y la única que no
hay que reescribir con cada tecnología nueva.

**Los dos mecanismos estándar que ya teníamos no servían**, y conviene dejarlo
escrito para que nadie los vuelva a proponer: gRPC Server Reflection y el WIT
embebido en el `.wasm` dirían lo mismo —*«hay un método `Invoke` que recibe un
nombre, un intento y una lista»*—, correcto e inútil. Es la factura del despacho
por nombre (ADR-0003), que es la decisión que nos hace lenguaje-agnósticos.

### 2 — Una sola llamada devuelve el catálogo entero, no una por paso

`Catalog` trae `repeated StepSpec`, y cada `StepSpec` lleva nombre, entradas,
salidas y una línea de documentación. El motor ya necesita saber qué nombres
atiende cada endpoint; una llamada al arrancar resuelve routing y validación de
golpe.

### 3 — Una vez por endpoint al arrancar, nunca antes de cada paso

No por coste —sería asumible incluso paso a paso— sino por dos razones que sí
pesan:

- paso a paso te enteras del error **en el paso 47**, con la unidad medio
  probada;
- un catálogo que cambia a mitad de corrida hace el informe irreconstruible.

Un hallazgo **detiene la corrida antes del primer paso**, con exit 1. Es la
regla de detección de ADR-0019 aplicada donde ahora sí se puede: lo comprobable
sin medir se decide sin medir.

### 4 — Poder no contestar, pero que se note

Un ejecutor de terceros puede no implementar `Describe`. Entonces sus pasos
salen como **sin comprobar**, con el motivo y el recuento:

```
aviso: 2 step(s) unchecked on 'python': it does not answer Describe (…)
```

Ni error —cerraría la puerta a terceros— ni silencio, que es el verde falso de
ADR-0019. Son tres respuestas distintas y ninguna se puede mapear sobre otra.

**Cómo se dice «no describo» sin que se confunda con «no sirvo nada»:** un
`Catalog` con `steps` vacío es ambiguo, así que el mensaje lleva un booleano
`describes`. proto3 hace que el default sea `false`, de modo que **todo silencio
—un cuerpo vacío, un par antiguo, un `UNIMPLEMENTED` que llega como un stream
roto— cae en «no me compruebes»**, que es la única lectura segura. Un ejecutor
que de verdad no sirve ningún paso lo dice poniéndolo a `true`. Es el mismo
truco del eco de contrato de ADR-0020 §4b: que la ausencia de dato no se pueda
leer como un dato.

**El puente WASM es hoy el caso real de esto.** `anvil:step@0.3.0` exporta un
único `run(name, attempt, inputs)`: el componente despacha por nombre dentro de
sí mismo, así que desde fuera no hay lista de nombres que publicar. El puente
contesta `describes = false` y esos pasos salen sin comprobar. Hacerlos
describibles exige una función de introspección en el WIT —versión nueva de la
interfaz y recompilar todo componente, ADR-0020 §4d— y ésa es la decisión que
espera el `#39`, no ésta.

### 5 — Sin inventar un sistema de tipos

Los tres de ADR-0020 (número, texto, booleano), más obligatorio/opcional y su
valor por defecto. El `enum ValueType` tiene un `UNSPECIFIED = 0`, y con la
misma lógica del punto anterior: **un tipo que el ejecutor no declara queda sin
comprobar, nunca se adivina que es número**.

El valor por defecto se declara **para el lector y para el futuro editor, no
para el motor**: el motor no lo envía, lo aplica el paso. Un default aplicado en
dos sitios es un default que diverge.

**Qué se comprueba y qué no, exactamente.** Sólo lo que va a cruzar el cable:
los pasos `grpc` que no estén `disable: true`. Un `statement`, un `pass_fail` y
un `sequence_call` los orquesta el motor y ningún ejecutor los describe
(ADR-0009, ADR-0010, ADR-0018); un paso deshabilitado se registra como
`saltado` **sin preguntar a nadie** (RF-34), así que no puede medir otra cosa, y
negarse a correr una secuencia entera por un paso que su autor apagó a
propósito sería la comprobación pasándose de frenada. La regla es una: se
comprueba lo que se va a invocar.

**Y una expresión no se comprueba de tipo.** `${locals.n}` puede valer un número
o un texto hasta que la corrida lo diga; rechazarla sería inventarse un
hallazgo. Se comprueba su **nombre**, que es lo que cuesta el dedazo. Lo demás
—que sea del tipo que el paso pide— sigue siendo de ejecución, y está dicho.

### 6 — `--validate` no rompe su promesa: se sale de ella a mano

`--validate` promete *«carga y valida sin ejecutar ni conectar (CI sin
hardware)»*, y preguntar la firma **exige conectar**. No se pueden tener las dos
cosas, así que:

| | ¿Conecta? | ¿Comprueba firmas? |
|---|---|---|
| `--validate` | No | No |
| `--validate --with-executors` | Sí | Sí |
| Al arrancar la corrida | Sí (ya lo hacía) | Sí, antes del primer paso |

El flag es un opt-in explícito de quien tiene los ejecutores levantados, y fuera
de `--validate` es un error de uso: las firmas se comprueban siempre al correr,
así que aceptarlo en silencio sugeriría que enciende algo.

Esto obligó a tocar `va_a_ejecutar_pasos` en el host: con `--with-executors` sí
hay que levantar el ejecutor embebido. Es la única excepción al issue #22, y es
explícita.

### 7 — En el ejecutor Python, la firma **es** el catálogo

Se decoran funciones y se dejan caer en una carpeta:

```python
from anvil_step import step, Result

@step(outputs={"serie": str})
def medir_resistencia(ctx, canal: float, escala: str = "auto") -> Result:
    """Mide resistencia en un canal."""
    return Result.measured(99.5, outputs={"serie": "R-007"})
```

Nombres, tipos y qué es obligatorio salen de `inspect.signature` y de las
anotaciones. **Nada se escribe dos veces, así que nada puede divergir** — es el
truco de OpenTAP (la reflexión del lenguaje), pero sin quedarse atado a un
lenguaje, porque lo que viaja es el catálogo y no la clase.

Lo que Python no puede inferir lo toma el decorador: `outputs` (un `return` no
lleva nombres) y `name` (cuando el nombre del paso en la secuencia no es un
identificador válido).

**Dónde se ponen los pasos:** una ruta configurable, que es el patrón de
«proyecto» que sugería el issue. Por orden: `--steps PATH` (repetible), la
variable `ANVIL_PYTHON_STEPS`, o `./steps`. Una ruta que no existe es un fallo
de arranque: un ejecutor que sirve el catálogo vacío por un dedazo en una ruta
es el verde falso más barato que hay.

**El ejecutor hace cumplir su propio catálogo.** Un parámetro que el paso no
declara, uno obligatorio que falta o uno del tipo equivocado son `error`, nunca
un default silencioso. Un catálogo que nadie hace cumplir es una promesa que
nadie cumple, y el paso acabaría midiendo otra cosa y diciendo `pass`.

En Rust no hay reflexión, así que los dos ejecutores propios (`pasos_demo`,
`pasos_scpi`) publican su firma desde **la misma tabla que despacha**, y hay un
test que lo sujeta.

### 8 — Añadir `Describe` **no** sube el contrato

Sigue en 3. La regla de ADR-0020 §4c es *«sube `contrato` todo cambio en el que
el silencio de un par antiguo pueda alterar un veredicto»*. Un ejecutor que no
describe su catálogo **mide exactamente lo mismo**: no altera ningún veredicto,
sólo reduce lo que se puede comprobar antes. Subirlo habría convertido a todo
ejecutor existente en incompatible a cambio de nada.

## Alternativas descartadas

- **Un sidecar de metadatos junto al artefacto** (un `.json` al lado del
  `.wasm`/`.py`). Se descarta por la misma razón que se descarta inspeccionar el
  artefacto: es un segundo sitio donde vive la verdad, y el día que el fichero y
  el código no coincidan, Anvil aprueba una secuencia equivocada con toda
  confianza. Preguntando, el que contesta es el que va a ejecutar.
- **gRPC Server Reflection.** Ver §1: describe métodos, no pasos.
- **Un RPC por paso (`Describe(nombre)`).** Ver §2 y §3.
- **Que no contestar sea un error.** Cierra la puerta a terceros y contradice
  ADR-0012, que es lo que hace de los ejecutores un producto adoptable.
- **Un registro declarado en un fichero de configuración del ejecutor Python**
  (una lista de módulos en un YAML). Es el `if/elif` con otra sintaxis: sigue
  habiendo un sitio que editar para añadir un paso.

## Recortes

- **No se toca el WIT ni el puente más allá de decir «no describo».** El
  `anvil check <paso>.wasm` del #39 sigue pendiente y sigue siendo la misma
  información: cuando el WIT sepa describirse, sale casi gratis.
- **No se valida el tipo de una expresión** (§5), ni las salidas que lee algo
  que no sea `assign`.
- **No hay deadline en la llamada a `Describe`.** `wasi-grpc` v0.1 no lo ofrece,
  así que un ejecutor de terceros que acepte la conexión y no conteste nunca
  colgaría el arranque. Los nuestros contestan siempre —incluso a una ruta
  desconocida, con un cuerpo vacío, que es un arreglo de esta misma tanda— y
  cualquier servidor gRPC de biblioteca devuelve `UNIMPLEMENTED`. Queda escrito:
  es de `wasi-grpc`, no de aquí.
- **No se decide el editor visual**, aunque esto es la mitad de lo que le hacía
  falta (`ui-vs-headless.md`).
- **Los nombres de paso y de parámetro no se traducen al inglés.** Son datos de
  las secuencias de los usuarios, no identificadores del código: renombrarlos
  rompería toda secuencia escrita, y eso es una decisión aparte con su propio
  issue.

## Consecuencias

- **`paso.proto` cambia por segunda vez**, y de forma **aditiva**: un RPC nuevo
  y mensajes nuevos. Ningún tag existente se mueve. Un ejecutor que no lo
  conozca sigue funcionando exactamente igual.
- **`--validate` gana un modo**, y con él la primera comprobación de Anvil que
  necesita hablar con algo. La promesa del modo por defecto se mantiene intacta,
  y eso hay que seguir defendiéndolo: es su razón de existir en CI.
- **Una corrida puede ahora no llegar a empezar.** Es nuevo, y es deliberado:
  antes se empezaba a probar la unidad y se descubría el fallo a mitad.
- **El ejecutor Python pasa a ser producto.** Tiene SDK (`anvil_step`), tests
  propios que no necesitan gRPC, y `--list` para ver el catálogo sin levantar un
  banco. Y deja de tener un fichero que el usuario tenga que editar.
- **Ya ha encontrado un fallo latente en el propio repo**, antes de salir de
  esta sesión: `ejemplos/variables.yaml` llamaba a `verificar_frecuencia`, que
  no lo sirve ningún ejecutor. Nunca dio guerra porque Main corta en el primer
  fallo y no se llegaba a invocar — hasta que alguien tocara el límite del paso
  anterior. Es exactamente la clase de bomba de relojería que motiva el #45, y
  la encontró el mecanismo, no una revisión.
- **Un fixture de test tuvo que cambiar de forma**, y conviene saberlo: el del
  issue #27 usaba un paso inexistente para provocar un `error`, y eso ya no
  llega a ejecutarse. Ahora usa un paso real que falla midiendo (SCPI sin
  instrumento), que además es el caso realista.
- **Un test de regresión por regla, y visto fallar.** En particular: el silencio
  leído como catálogo vacío, el tipo de una expresión adivinado, y el catálogo
  de Rust divergiendo del despacho. Los tres se han visto en rojo reintroduciendo
  el fallo a mano — y dos tests que *no* se vieron fallar resultaron estar
  probando otra cosa, y se reescribieron.
