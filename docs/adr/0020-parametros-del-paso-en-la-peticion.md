# ADR-0020: Los parámetros del paso viajan en la petición, tipados y con versión de contrato

- **Estado:** Aceptada
- **Fecha:** 2026-08-25
- **Cómo se decidió:** desde dirección, sobre el hueco que
  [contrato-grpc.md](../contrato-grpc.md) declara desde M1 («no lleva
  variables»), la nota de post-MVP de
  [variables-y-alcances.md](../diseno/variables-y-alcances.md) («el cableo de
  variables al paso por el wire es post-MVP») y la pregunta que el issue #39
  dejó abierta sobre la versión de `anvil:paso`. Las afirmaciones sobre el
  estado de hoy están verificadas ejecutando el código de este repo (§Contexto);
  las de la competencia **no** se han contrastado con fuentes primarias en esta
  sesión y van marcadas como tales.
- **Relaciona:** ADR-0003, ADR-0005, ADR-0008, ADR-0009, ADR-0010, ADR-0012,
  ADR-0013, ADR-0015, ADR-0019, RNF-04, RNF-05, RNF-08
  ([requisitos.md](../requisitos.md)), issues #34, #39
- **Alcance:** decide el contrato. **No** lo implementa, no diseña el editor
  visual, no añade lenguajes y no toca el empaquetado del release.

## Contexto

Hoy un paso de Anvil **no recibe nada** salvo su propio nombre y el número de
intento. Verificado el 2026-08-25 en este repo:

- `crates/modelo/paso.proto`: `PeticionPaso { nombre, intento }`. Dos campos.
- Declarar `parametros:` en un paso del YAML es error de carga —el schema es un
  subconjunto estricto con `deny_unknown_fields`— pero el mensaje no nombra ni
  el campo ni el paso: `Sintaxis(TypeMismatch { expected: "string", found:
  "non-string scalar" })`. Es el mismo defecto de diagnóstico del issue #20.
- Las variables (`file_globals`, `locals`, `parameters`) existen desde M4 y
  viven **enteras en el motor**. `ejemplos/variables.yaml` lo dice en su
  cabecera: *«el cableo al paso por el wire es post-MVP: las variables viven en
  el motor; el paso gRPC no las recibe»*.

La consecuencia es que **todo lo que un paso necesita saber para medir hay que
grabárselo dentro**. En el propio repo, las tres salidas del hueco:

| Dónde | Cómo se parametriza hoy | Precio |
|---|---|---|
| `pasos_demo::medir_voltaje` | `let valor = 4.2;` en el código | recompilar para cambiar de canal |
| `pasos_scpi` | variable de entorno `ANVIL_SCPI_ADDR`, con la nota de que *«el host aún no plumbea env vars al guest»* | configuración global, invisible en el informe |
| `executors/python/server.py` | flag de proceso `--simulador 192.168.1.50:4000` | un ejecutor por configuración |

Ninguna de las tres viaja en el cable, y por tanto **ninguna de las tres queda
escrita en el informe**. Dos ejecuciones de la misma secuencia con distinto
canal producen informes idénticos. Eso choca de frente con la Regla 3 de
ADR-0019 —*lo que altera el criterio queda escrito en el informe*—, sólo que
por una puerta que aquel ADR no miró: no altera el criterio el límite, lo
altera **la condición en la que se midió**.

Y hay un segundo hueco simétrico, ya fichado: un paso sólo puede devolver una
medida (`valor_medido`, y `asigna` sólo lee tres campos fijos,
`modelo::CAMPOS_RESULTADO`). Devolver dos números de una subsecuencia cuesta
andamiaje (issue #34); devolver dos números de un **paso** no se puede.

El hueco está declarado desde el principio como pendiente, no es un
descubrimiento. Lo que decide este ADR es **con qué forma se cierra**, antes de
que se cierre tres veces distintas: una para el ejecutor embebido, otra para el
puente WASM y otra para quien escriba el suyo.

### Por qué se decide ahora y no al implementarlo

Porque el contrato ya tiene usuarios fuera del repo. El 14/08 un componente
`anvil:paso` producido por Telekino —otra tecnología, sin `cargo-component`,
sin tocar Anvil— encajó a la primera (issue #39). Ese mismo issue deja la
pregunta escrita: *«conviene decidir ya si habrá compatibilidad hacia atrás en
el host o si la regla será recompilar, y escribirlo»*. Sigue sin respuesta, y
la respuesta la necesita cualquiera que produzca un `.wasm` hoy.

## Decisión

### 1 — Los parámetros van **dentro de `PeticionPaso`**, no en un RPC aparte

Una invocación de paso es **un mensaje, una respuesta**. Los parámetros son
argumentos de esa llamada y viajan con ella.

```proto
message Valor {
  string nombre = 1;
  oneof valor {
    double numero    = 2;
    string texto     = 3;
    bool   booleano  = 4;
  }
}

message PeticionPaso {
  string nombre = 1;
  int32  intento = 2;
  repeated Valor parametros = 3;   // nuevo
  int32  contrato = 4;             // nuevo: versión que habla el motor
}
```

La alternativa —un `Configura`/`SetParams` previo a `Invoca`— se descarta en
§Alternativas, pero la razón principal conviene tenerla aquí: **partir la
llamada en dos convierte al ejecutor en un objeto con estado**, y el ejecutor
de Anvil no lo tiene por diseño. El puente WASM instancia el componente una vez
y atiende N llamadas sobre un Store (ADR-0015); el ejecutor Python es un
`ThreadPoolExecutor`. Con parámetros fuera de la llamada, dos invocaciones
concurrentes del mismo paso —que es el post-MVP de paralelismo (RF-39)— se
pisan, y un reintento (`intento: 2`) no puede saber si la configuración que hay
puesta es la suya. El coste que se evita a cambio es cero: el argumento de
RNF-04 que ya sostiene ADR-0003 —el salto local es despreciable frente al
tiempo de un instrumento— vale igual para unos cientos de bytes más.

### 2 — Entrada: tipada, con nombre, y con los tres tipos que ya existen

- **Con nombre, no posicional.** El YAML se escribe y se revisa a mano y se
  lee en un diff; reordenar dos parámetros no puede cambiar lo que mide el
  banco. Sin introspección de firma —que no existe— el nombre es el único
  anclaje estable.
- **Tipada en el cable, con `oneof`.** Los tipos son exactamente los tres de
  `modelo::ValorDefinicion` / `expr::Value`: número (`f64`), texto y booleano.
  Ni más (nada de listas ni mapas: un parámetro que necesita estructura es un
  paso mal cortado) ni menos.
- **Un `oneof` sin rama puesta es error**, no un cero. Es la Regla 2 de
  ADR-0019 aplicada al cable de entrada.

**Sobre la asimetría con las medidas, que es deliberada.** El contrato manda
`valor_medido`, `limite_min` y `limite_max` como `string`, y
[contrato-grpc.md](../contrato-grpc.md) da tres razones. La primera —«en proto3
un `string` vacío no se transmite», así que un `0.0` sería ambiguo con "no hay
medida"— es la única que pesa, y **aquí no aplica**: la ausencia de un
parámetro se expresa no estando en la lista, no con un valor centinela. Un
`repeated` resuelve el tri-estado que un campo escalar no resolvía. Por eso la
entrada va tipada aunque la medida siga en texto, y por eso **este ADR no toca
los campos 4-6**: cambiarlos es otra decisión, con otros afectados (los sinks,
el CSV, los issues #41 y #33) y sin relación con esta.

En el YAML, la superficie es una tabla de pares, con expresiones donde hoy ya
las hay:

```yaml
- nombre: medir_voltaje
  reintentos: 1
  parametros:
    canal: 2
    etiqueta: "banco-3"
    promediar: true
    muestras: '${locals.n_muestras}'
  limite: { tipo: rango, min: 4.5, max: 5.5 }
```

El tipo es el del literal, o el del resultado de la expresión. Las evalúa el
motor antes de la llamada (ADR-0009: las expresiones las evalúa el motor, no el
paso); **una expresión que falla convierte el paso en `error`**, por el camino
que `aplica_asigna` ya abrió, y nunca en un valor por defecto. Un `parametros:`
que no sea un mapa de escalares es **error de carga**, no de ejecución (regla
de detección de ADR-0019): es decidible sin banco.

Esto es, por fin, el cableo de variables al paso que
[variables-y-alcances.md](../diseno/variables-y-alcances.md) tiene apuntado
como post-MVP — y en la única forma que no rompe el aislamiento: **el motor
evalúa y envía valores; el paso no ve el entorno**. Un paso no lee `locals`; se
le pasa un número.

### 3 — Salida: la misma forma, por el mismo cable

```proto
message ResultadoPasoProto {
  // 1..6 sin cambios: nombre, estado, mensaje, valor_medido, limite_min, limite_max
  repeated Valor salidas = 7;   // nuevo
  int32  contrato = 8;          // nuevo: eco de lo que el ejecutor entendió
}
```

Un paso puede devolver N valores con nombre y tipo, y `asigna` los lee como
`resultado.salidas.<nombre>`. `valor_medido` **no se toca ni se duplica**:
sigue siendo la medida contra la que el motor evalúa el `limite` (ADR-0008).
`salidas` es lo demás —una temperatura de contexto, un número de serie leído,
un coeficiente— y no participa en el veredicto.

**Sin `inout`, sin by-reference.** Un `sequence_call` sí pasa `parameters` de
entrada y salida por referencia (ADR-0010), y puede, porque lo orquesta el
motor contra su propio entorno. Un paso gRPC no: entra por `parametros`, sale
por `salidas`, y no hay tercer camino. Copiar el `inout` de TestStand aquí
significaría darle al paso una referencia al entorno del motor, que es
exactamente lo que ADR-0003 y ADR-0005 evitan.

**El precio, dicho claro:** hoy los campos de `resultado` son tres y conocidos,
y por eso #27 pudo convertirse en error de carga. `resultado.salidas.tension`
**no es validable al cargar** —el cargador no sabe qué devuelve un paso— así
que un nombre equivocado es un `error` de ejecución, no de carga. Es una
excepción a la regla de detección de ADR-0019, y es inevitable **mientras no
exista introspección de firma**. Lo que convierte a la introspección de
`Describe` (hoy apuntada en [contrato-grpc.md](../contrato-grpc.md) como
extensión para el editor visual) en otra cosa: deja de ser un lujo de la UI y
pasa a ser lo que le devuelve a `--validate` el terreno que este ADR le quita.
Su diseño no es de aquí, pero su motivo sí: queda escrito.

### 4 — Cómo se versiona el contrato a partir de ahora

Hoy el contrato **no declara versión en el wire**, y `contrato-grpc.md` lo
tiene como *pendiente*. La política, desde aquí:

**a) Un único mecanismo: un entero monótono, `contrato`, en la petición y en la
respuesta.** El motor manda el que habla; el ejecutor devuelve **el que ha
entendido**. No hay rutas versionadas (`/EjecutorPasos.v2/Invoca`) ni RPC de
saludo: dos mecanismos para lo mismo divergen, y una ruta nueva obliga a
regenerar stubs y a tocar el puente para no ganar nada que el número no dé.

- Contrato **1** = el de hoy (sin parámetros). Un ejecutor 1 ignora los tags 3
  y 4 —proto3 lo permite— y devuelve `contrato = 0` por defecto.
- Contrato **2** = éste.

**b) El eco es lo que impide el silencio.** Un campo aditivo es «compatible»
sólo en el sentido de que el mensaje decodifica. Un ejecutor viejo que ignora
`parametros` **mide otra cosa y dice `paso`**: es el verde falso de ADR-0019,
por una puerta nueva. Por eso:

> Si el paso declaró `parametros` (o su `asigna` lee `salidas`) y el eco del
> ejecutor es menor que 2, el paso es **`error`**, nombrando el endpoint y las
> dos versiones. Nunca `fallo`, y nunca se ejecuta con los parámetros perdidos.

Y su recíproco, que es lo que mantiene vivo lo que ya funciona:

> Si el paso **no** declara parámetros ni lee `salidas`, un ejecutor de
> contrato 1 sigue siendo válido y no cambia nada.

**c) Cuándo sube el número.** La regla no es «aditivo vs. rupturista», es ésta:

> Sube `contrato` todo cambio en el que **el silencio de un par antiguo pueda
> alterar un veredicto**. Lo que un par puede ignorar sin que la afirmación
> sobre la unidad cambie (un campo informativo, una traza), no lo sube.

Retirar o renombrar un tag exige ADR, entrada *breaking* en el CHANGELOG y
`reserved` sobre el tag: **un tag no se reutiliza jamás**. Dejar de servir un
número de contrato antiguo es también un cambio *breaking* con ADR.

**d) El WIT se versiona por recompilación — ésta es la respuesta al #39.** La
versión de `anvil:paso` va en el nombre de la interfaz, viaja pegada al
artefacto, y wasmtime falla al instanciar si no casa. **No habrá capa de
compatibilidad en el puente**: la regla es recompilar. El puente es el único
que traduce, y por tanto el único que sabe qué número de contrato corresponde a
qué versión del WIT; **es él quien responde el eco** por los pasos WASM, que no
lo ven nunca. Un componente sigue sin saber de gRPC, de protobuf ni de
versiones de contrato (ADR-0015).

El WIT pasa a `anvil:paso@0.2.0`:

```wit
interface paso {
  variant valor { numero(f64), texto(string), booleano(bool) }
  record nombrado { nombre: string, valor: valor }
  record resultado {
    estado: string,
    mensaje: string,
    valor-medido: option<f64>,
    salidas: list<nombrado>,
  }
  run: func(nombre: string, intento: s32, parametros: list<nombrado>) -> resultado;
}
```

**e) Las cuatro copias del contrato se mueven en el mismo commit**: el
`.proto`, el espejo `prost` hecho a mano de `crates/modelo/src/proto.rs`, el
WIT del puente y —regenerados— los stubs del ejecutor Python. Están así por
ADR-0006 (wasi-grpc v0.1 no trae codegen) y ya lo dice `contrato-grpc.md`; con
un número de versión de por medio, desincronizarlas deja de ser un bug de
compilación para pasar a ser un eco que miente.

### 5 — Qué pasa con lo que ya existe

**Nada se rompe por el lado gRPC.** Es la propiedad que compra la regla (b):

- `ejemplos/*.yaml`: **ninguno** declara parámetros. Ninguno cambia. Que sigan
  pasando sin tocarlos es la prueba de que el contrato 1 sigue servido.
- `pasos_demo`, `pasos_scpi`: no cambian. Su firma interna `fn(i32)` pasará a
  llevar los parámetros cuando se implemente, pero eso es despacho interno del
  ejecutor, no contrato.
- `executors/python/server.py`: no cambia. Sus stubs están gitignoreados y se
  regeneran; hasta que alguien quiera parámetros, es un ejecutor de contrato 1
  perfectamente válido.

**Se rompe el `.wasm` ya compilado, y es a conciencia.** `run` cambia de firma,
así que todo componente `anvil:paso@0.1.0` —`ejemplos/hola-paso` y el de
Telekino, que son los que hay— deja de instanciar y hay que recompilarlo. La
alternativa era exportar `run` y `run-con-parametros` y quedarse las dos para
siempre; se descarta porque deja el camino viejo como el fácil, y el camino
viejo es el que pierde los parámetros en silencio. El fallo, en cambio, es
ruidoso e inmediato: lo da wasmtime al instanciar, antes de medir nada.

Dos deberes que esa rotura arrastra, y que no son de este ADR pero sí de quien
lo implemente: el diagnóstico de ese fallo es hoy *«failed to convert function
to given type»* sin más pista (issue #24), y `anvil check` (issue #39) es lo
que convierte «recompila» en una instrucción accionable. **La regla de
recompilar sólo es defendible con esas dos piezas**; sin ellas se está mandando
al usuario a leer un volcado.

**Y una migración que se gana gratis:** `ANVIL_SCPI_ADDR` y `--simulador` son
candidatos naturales a parámetro. Cuidado con la pendiente: **un parámetro es
lo que varía por paso dentro de la secuencia** (canal, número de muestras,
tolerancia, etiqueta). La dirección de un instrumento es configuración de
despliegue y su sitio es `ejecutores:` en el YAML, no la tabla de parámetros de
cada paso. Convertir esto en un canal de configuración general es la forma de
que dentro de un año no se sepa qué corrió.

### 6 — Los parámetros enviados van al informe

Es la Regla 3 de ADR-0019 sin novedad de criterio: el JSON y el CSV registran
los parámetros con los que se invocó cada paso y las `salidas` que devolvió. Un
informe que no distingue una medida en el canal 2 de la misma medida en el
canal 3 no es auditable, y auditable es lo único que hace útil a un informe
meses después.

El **reporte textual no cambia** (RNF-08): la procedencia y el detalle viven en
el JSON y el CSV, igual que se decidió para la procedencia del límite.

## Alternativas descartadas

**Un RPC aparte (`Configura` antes de `Invoca`).** Es la opción que el enunciado
obliga a considerar y la que más cosas rompe: convierte al ejecutor en un objeto
con estado entre llamadas; obliga al puente WASM —hoy sin estado por invocación—
a llevar una tabla de sesiones; abre una carrera en cuanto haya paralelismo
(RF-39); deja ambiguo qué configuración ve un reintento; y parte en dos la
unidad que el informe tiene que auditar como una. A cambio ahorra unos bytes por
llamada que RNF-04 ya declaró irrelevantes. Su único argumento real —parámetros
grandes que no conviene remandar en cada reintento— no describe a nadie hoy: un
paso de test recibe escalares, y si un día recibe un blob, eso es un recurso con
su propio ciclo de vida, no un parámetro.

**Parámetros posicionales.** Más baratos en el cable y peores en todo lo demás:
sin introspección, el orden es un acuerdo tácito entre el YAML y el código del
paso, y un diff que reordena dos líneas cambia lo que mide el banco sin que se
vea.

**Todo en texto, como las medidas.** Coherente con lo que hay, y equivocado: la
razón que justificó el texto en las medidas (el tri-estado de proto3) no existe
en un `repeated`, y obligaría a cada ejecutor a re-parsear —el Python decidiendo
si `"2"` es número o cadena— que es justo el pegamento que un contrato tipado
existe para no escribir.

**Un `map<string,string>` o un blob JSON.** Opaco para el informe, opaco para
una futura `Describe`, y una invitación a anidar estructura que el paso tendría
que interpretar. Un contrato que transporta JSON no es un contrato: es un tubo.

**Que el paso lea el entorno del motor (`locals`, `file_globals`).** Sería
«cablear variables al paso» en el sentido literal, y rompe la tesis entera: el
paso dejaría de ser opaco y aislado (ADR-0003, ADR-0005) y el motor dejaría de
ser genérico. El motor evalúa y manda valores; nadie más ve el entorno.

**Dejarlo pendiente hasta que haya editor visual.** Es lo que se ha hecho hasta
hoy, y tiene un coste que ya se está pagando: tres formas distintas de
parametrizar un paso conviviendo en el repo, ninguna en el informe, y un
contrato con usuarios externos al que cada semana cuesta más cambiarle la firma.

## Cómo lo resuelve el sector

> **No verificado con fuentes primarias en esta sesión.** ADR-0019 dejó
> escrito lo que pasa cuando esta sección se escribe de memoria: se afirmó algo
> falso sobre TestStand y hubo que refutarlo. Así que aquí va sólo lo que este
> repo ya tiene documentado con fuente, y lo demás queda como pendiente de
> contrastar antes de usarlo en material público.

Lo que sí está fichado con fuente en
[investigacion/TestStand-y-competencia.md](../investigacion/TestStand-y-competencia.md)
§1.4: TestStand tiene *expression engine* que cubre «precondiciones,
postcondiciones, límites, asignaciones, **parámetros a code modules**», y una
jerarquía de variables con scopes que permite «cablear datos entre pasos y
declarar límites sin código pegamento, de forma auditable». Es decir: **que los
parámetros del paso se declaren como datos y se cableen con expresiones es la
forma del líder del mercado**, y este ADR va en esa dirección, no en contra.

Lo que se decide **distinto** a TestStand, y por qué: allí los parámetros son
`inout` por referencia contra el code module; aquí sólo entrada por
`parametros` y salida por `salidas`, porque el paso está detrás de un proceso y
un contrato, no dentro del proceso del secuenciador. El `inout` sobrevive donde
sí tiene sentido —`sequence_call`, que es motor-side (ADR-0010)—.

Pendiente de contrastar antes de afirmarlo fuera: cómo expone OpenTAP las
propiedades de un step y sus *external parameters*, y qué garantías de
compatibilidad da cada uno al cambiar la firma de un step existente. Esa
segunda pregunta es la que más nos importa y es la que menos sabemos.

## Recortes

- **No se implementa aquí.** Ni `.proto`, ni `proto.rs`, ni WIT, ni cargador,
  ni informe. Este ADR es la decisión; el trabajo se encarga aparte.
- **`Describe` / introspección de firma no se diseña.** Se fija su motivo
  (§3) y se deja fuera. Sin ella, `--validate` no puede comprobar nombres de
  parámetros ni de salidas, y eso es una pérdida aceptada, no un olvido.
- **Los campos 4-6 (`valor_medido`, `limite_min`, `limite_max`) no se tocan.**
  Siguen en texto. Los issues #41 y #33 hablan de ellos y se deciden aparte.
- **No se decide qué hace un paso con un parámetro que no conoce**, más allá
  de la obligación del contrato: `error`, nunca un valor por defecto silencioso.
  Anvil puede imponerlo en sus ejecutores y en el puente; en uno de terceros es
  una obligación escrita, y la única defensa real vuelve a ser `Describe`.
- **No hay listas ni mapas como tipo de parámetro.** Si aparece un caso que los
  pida de verdad, se decide entonces y sube el contrato.
- **No se decide el paralelismo** (RF-39). Se ha usado como argumento —un
  ejecutor con estado se rompe con él— pero su diseño sigue siendo post-MVP.

## Consecuencias

- **`paso.proto` cambia por primera vez desde que se escribió.** RNF-05 exige
  ADR/RFC para eso; este ADR lo cumple. El proceso RFC sigue diferido
  (roadmap, «Procesos diferidos»), y este cambio no lo activa.
- **Es *breaking* para los `.wasm` compilados y para nadie más.** Va al
  CHANGELOG como tal, con la instrucción de recompilar y con `anvil check`
  (#39) como la herramienta que lo diagnostica. Hacerlo ahora cuesta dos
  componentes; hacerlo con usuarios cuesta un periodo de deprecación.
- **El cargador gana validaciones que son de `--validate`**, no de ejecución:
  `parametros` que no es mapa de escalares, valor que no es uno de los tres
  tipos, expresión que lee un nombre no declarado (la línea que ya abrió el
  arreglo de `--validate`, commit `e333463`).
- **`asigna` gana un lado derecho que el cargador no puede comprobar**
  (`resultado.salidas.X`), y con él la primera excepción declarada a la regla
  de detección de ADR-0019.
- **El informe gana columnas y el JSON estructura.** Quien consuma el JSON verá
  campos nuevos; son aditivos y opcionales.
- **Un test de regresión por regla, y visto fallar.** En particular el eco: un
  ejecutor de contrato 1 recibiendo un paso con `parametros` tiene que salir
  `error`, y ese test hay que verlo en rojo devolviendo el eco correcto a mano.
  Un test de eco que sólo comprueba el camino feliz no protege de nada, que es
  exactamente el defecto contra el que este repo ya se ha tropezado.
- **La tesis de ADR-0003 se completa.** «Cualquier lenguaje que hable gRPC es
  un adapter» era cierto y estaba cojo: un adapter al que sólo se le puede
  decir su nombre no sustituye a un code module con parámetros. Con esto, la
  comparación con TestStand deja de tener un asterisco.
