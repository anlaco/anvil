# ADR-0022: La referencia a objeto es un cuarto tipo, y nombra una ranura

- **Estado:** Aceptada
- **Fecha:** 2026-08-28
- **Cómo se decidió:** desde dirección, en una sesión de diseño sobre el issue
  [#55](https://github.com/anlaco/anvil/issues/55), que se abrió explícitamente
  para discutir y no para implementar. La discusión completa, con los porqués y
  lo que se descartó, está en el [comentario del
  2026-08-28](https://github.com/anlaco/anvil/issues/55#issuecomment-5451000448).
  Todo lo que se afirma del estado de hoy está **verificado leyendo y ejecutando**
  el código de este repo y se cita con fichero y línea; lo de TestStand viene de
  la documentación de NI que se cita.
- **Relaciona:** ADR-0001, ADR-0003, ADR-0005, ADR-0013, ADR-0015, ADR-0016,
  ADR-0019, ADR-0020, ADR-0021, RF-32, RF-39
  ([requisitos.md](../requisitos.md)), issue #55,
  [contrato-grpc.md](../contrato-grpc.md)
- **Alcance:** decide el tipo, su semántica y quién acuña cada parte. **No lo
  implementa.** No decide la forma exacta en JSON y CSV, no resuelve los
  huérfanos por aborto brusco, no toca `StationGlobals` y **no añade la
  composición** —contenedores y arrays—, que es un hueco distinto.

## Contexto

Una secuencia de test necesita que varios pasos operen sobre el mismo estado del
banco. En TestStand eso es un patrón de primera clase —el «rack»: un objeto con
las conexiones y la configuración de todos los instrumentos, que se pasa de un
paso a otro para que el estado sea el mismo— y en Anvil no estaba recogido en
ningún requisito, ni nota de diseño, ni ADR.

Ese objeto **no puede cruzar el cable**: lleva dentro sockets abiertos y locks
de drivers del fabricante. En TestStand funciona porque el secuenciador y los
VIs comparten proceso y lo que viaja es un puntero; Anvil invoca cada paso por
gRPC (ADR-0003), que es justamente lo que permite que un paso esté escrito en
otro lenguaje o corra en otra máquina. A TestStand le pasa lo mismo en cuanto
sale del proceso — el día que el rack tenga que vivir en el Windows 7 con los
drivers mientras el motor corre en otro sitio, allí también deja de funcionar.
Anvil paga ese precio desde el principio en vez de que aparezca a mitad de
proyecto.

Lo que cruza, por tanto, es una **referencia**: el objeto se queda en el
ejecutor y viaja un identificador suyo. Anvil y el ejecutor hablan un idioma, y
el ejecutor traduce al de su lenguaje. Eso es lo que evita tener que declarar
clases en un lenguaje neutro, generar stubs por lenguaje y arbitrar tipos entre
ellos — una IDL, que se descarta explícitamente.

**El mecanismo ya funciona hoy y está verificado** (issue #55, contra `v0.3.0`,
con `--validate --with-executors` en verde): un método es un nombre del catálogo
y el objeto es un parámetro más — el `self` de Python, el terminal de clase del
VI. El paso que abre el rack devuelve su identificador en `outputs`, la
secuencia lo recoge con `assign`, y los pasos siguientes lo reciben en `inputs`.
Como un identificador es texto opaco, los tres tipos de ADR-0020 bastan.

Lo que falta no es el mecanismo. Es el **tipo**. Hoy la referencia es una cadena
y por tanto:

- se puede concatenar en una expresión, comparar con un límite o escribir a mano
  como literal en el YAML;
- se puede pasar a un paso servido por **otro** ejecutor, donde no significa
  nada — y una secuencia no tiene un ejecutor, tiene varios: `ejecutores:` en la
  raíz y `ejecutor:` por paso (ADR-0013);
- si el ejecutor se reinicia, las referencias viejas no son basura reconocible,
  son cadenas que casualmente ya no casan.

Y el objetivo es el sector industrial: la secuencia acabará siendo una
especificación que alguien lee, revisa y firma antes de que toque una unidad.
Eso pide que lo que hace se lea sin ejecutar nada, y que el modo de fallo sea
negarse.

## Decisión

### 1. La referencia es un cuarto tipo del contrato

Número, texto, booleano y **referencia**. Son exactamente los cuatro tipos
básicos de TestStand
([NI](https://www.ni.com/docs/en-US/bundle/teststand/page/tsfundamentals/infotopics/bldgblocks_standard_custom_data_types.html)).

El tipo existe para que el motor pueda **negarse**, no para que se lea mejor: la
legibilidad es un extra. El motor rechaza las operaciones sobre referencias
—aritmética, comparación, uso como valor esperado o como límite— y rechaza un
literal de referencia escrito a mano. Leer una referencia de una variable o
pasarla a un paso **no** es una operación: `assign` e `inputs` son expresiones y
tienen que seguir funcionando.

Sigue siendo **opaca** para el motor: Anvil no sabe qué hay dentro, no la puede
juzgar y no puede saber si el objeto sigue vivo.

### 2. Plana, sin clase

El tipo dice que es una referencia, no de qué clase. El `Object Reference` de
TestStand también es plano: la seguridad de clase la da el sitio de la llamada
—qué paso recibe qué parámetro—, no el tipo de la variable.

Un tipo por clase exigiría que Python, Java y LabVIEW se pusieran de acuerdo en
cómo se escribe un nombre de clase, y eso es la IDL que el §Contexto descarta.
El precio, aceptado: pasar el handle del multímetro donde iba el de la fuente no
lo caza el tipo. Lo caza el ejecutor, en ejecución.

### 3. El tipo va en el cable, en la variable y en el catálogo

Los tres, y el de la variable no es redundante.

`locals:` gana la posibilidad de declarar una variable de tipo referencia —hoy
`ValorDefinicion` (`crates/modelo/src/lib.rs:581`) sólo tiene los tres escalares
y `validar_lvalues` (`crates/cargador/src/lib.rs:942`) exige valor inicial, así
que **hoy la variable que va a llevar un rack no se puede ni declarar**.

Y es lo único que hace comprobable el ejecutor cruzado sin análisis de flujo de
datos. `inputs: { rack: '${locals.rack}' }` es una expresión, y `comprueba_tipo`
(`crates/motor/src/catalogo.rs:406`) devuelve `None` para toda expresión, con
test que lo clava en `catalogo.rs:638` — *«el tipo de una expresión no se
adivina»*. Seguir la referencia hasta su productor exigiría atravesar `assign`,
los `parameters` de las subsecuencias y el process model, que es exactamente el
análisis que ADR-0021 declinó hacer. Con el tipo declarado en la variable, la
comprobación es local y se ve leyendo el fichero.

### 4. El sello lo ponen los dos, cada uno lo que sabe

- El **nombre del ejecutor** lo estampa Anvil al recibir la referencia. El
  proceso Python no sabe cómo lo ha llamado la secuencia: los nombres los pone
  `ejecutores:`, del lado de Anvil, que además es quien enruta (ADR-0013).
- La **carga opaca** la acuña el ejecutor. Anvil no la interpreta jamás.

Ninguno de los dos afirma nada sobre lo que sabe el otro.

### 5. Ranura, no objeto

**La referencia nombra un casillero del ejecutor, no un objeto concreto.** Mutar
el estado no cambia la identidad: un paso que configura el banco devuelve la
misma referencia que recibió. Un paso acuña una referencia nueva sólo cuando de
verdad ha nacido otro objeto —derivar una configuración de otra, duplicar—,
nunca al mutar el que ya tenía.

Esto importa porque en LabVIEW una clase es un dato **por valor**: el VI recibe
una caja con el banco y devuelve otra caja distinta. El ejecutor LabVIEW guarda
la caja nueva en el mismo casillero y contesta la misma referencia; su
naturaleza por valor se queda dentro del ejecutor, que es donde no molesta.

Cuatro razones, de más a menos peso:

1. **Los reintentos.** `ejecuta_con_reintentos` (`crates/motor/src/lib.rs:235`)
   evalúa los parámetros una vez y reenvía los mismos en cada intento, y la
   `asigna` sólo corre sobre el resultado final. Con semántica de objeto, si el
   intento 1 muta y devuelve referencia nueva, el intento 2 sale con la vieja —
   la que el ejecutor ya considera pisada. O se rompen los reintentos o se
   rehacen, y los reintentos son sagrados en un secuenciador.
2. **Elimina un problema en vez de gestionarlo.** Con semántica de objeto, un
   bucle de doscientas unidades donde cada una toca el banco deja doscientas
   referencias huérfanas vivas en el mapa del ejecutor, en una corrida que va
   perfectamente bien y no da síntoma hasta que da uno gordo. Con ranura no hay
   nada que quede huérfano.
3. **Un idioma.** Con semántica de objeto, la naturaleza por valor de LabVIEW se
   filtra hasta la especificación firmada y obliga a quien audita a entender por
   qué el banco cambia de nombre a mitad de página.
4. **El `assign` olvidado deja de ser peligroso.** Con semántica de objeto, si se
   olvida el `assign` no falla nada: los pasos siguientes usan una referencia que
   existe, está viva y responde — el banco de **antes** de configurarlo. Verde y
   mal. Es el «se te olvida un cable y el objeto se pierde» de LabVIEW importado
   a Anvil. Con ranura no hay cable que olvidar.

Lo que la ranura quita: tener dos versiones del banco a la vez, que en LabVIEW
es bifurcar el cable. Si hiciera falta, será un paso explícito de duplicar, que
acuña referencia nueva porque de verdad ha nacido un segundo banco.

### 6. La validez de sesión no es tipado, y va por otro sitio

El tipo protege de un error del **autor de la secuencia**, y eso es estático. Que
el proceso de enfrente se haya muerto y vuelto a nacer no lo resuelve ningún
sistema de tipos: se resuelve preguntando si sigue siendo el mismo.

El ejecutor acuña un identificador de **vida** al arrancar y lo publica en el
mensaje `Catalog` del `Describe`, que ya existe (ADR-0021). Anvil lo compara; un
ejecutor que no lo trae hace que Anvil diga que no lo puede comprobar, que es la
lectura segura de ADR-0019.

La comprobación va **antes** de invocar el paso, no al leer el resultado: el
reinicio se detecta en la llamada siguiente, que puede ser justo la que lleva la
referencia muerta.

Y el ejecutor rechaza por su cuenta toda referencia cuya vida no sea la suya:
él lo sabe con certeza, Anvil sólo por comparación.

### 7. Dos deberes del ejecutor que el contrato no puede verificar

Se escriben donde los lea quien escriba un ejecutor. Un ejecutor que los
incumpla es un ejecutor roto.

1. **No reciclar claves dentro de una misma vida.** Si el ejecutor cierra un
   banco y el siguiente `abrir_banco` reutiliza la clave, una referencia vieja
   resuelve limpiamente a un objeto vivo y distinto: mismo ejecutor, misma vida,
   todo verde, midiendo contra el banco equivocado. Anvil no puede detectarlo
   desde fuera de ninguna manera.
2. **Acuñar una vida nueva y distinta en cada arranque.**

### 8. WASM se adapta

El ejecutor embebido no puede sostener una referencia hoy: su WIT es
`run(name, attempt, params) -> resultado`, una función sin recursos y sin estado
entre llamadas (ADR-0020 §4d), así que el componente del usuario no tiene dónde
guardar el mapa.

Eso **no exime al WASM del patrón**. WASM es la tecnología que esta casa ha
adoptado (ADR-0001) y trabaja para Anvil, no al revés: si hay que darle estado,
se le da. Pero **viene después**: la primera vuelta implementa el tipo para los
ejecutores gRPC de proceso, y el WIT se toca en un ADR propio cuando le toque.
Hasta entonces, un componente WASM que reciba una referencia es un `error`
explícito, nunca un silencio.

## Alternativas descartadas

- **Dejarlo en `text`, como está.** Funciona —está verificado— y no cuesta nada.
  Se descarta porque ninguna de las cuatro negativas del §1 se puede sostener
  sobre una cadena, y porque el destinatario final es una especificación firmada.
- **Tipo por clase.** Cazaría el handle equivocado, pero exige declarar clases
  fuera de los cuatro lenguajes, acordar cómo se escriben y generar stubs. Es
  CORBA, y TestStand tampoco lo hizo.
- **Que el objeto viaje serializado.** No puede: lleva sockets y locks.
- **Poner el tipo sólo en el catálogo y dejar el cable en `text`.** Da casi todo
  el valor estático sin tocar el `oneof` ni subir el contrato, y era la opción
  barata. Se descarta porque deja la comprobación en manos de que el ejecutor
  esté bien escrito, y en industrial el ejecutor lo escribe un tercero.
- **Un scope de sesión en el YAML**, con apertura y cierre garantizados por el
  motor. Resolvería el ciclo de vida de raíz —es lo que hace el process model de
  TestStand— pero obliga al motor a aprender un concepto de dominio, y eso choca
  de frente con ADR-0005. Merece mirarse otra vez cuando se cierren los
  huérfanos, no ahora.

## Consecuencias

- **`paso.proto` cambia por tercera vez, y esta vez NO de forma aditiva.** El
  `oneof` de `Value` gana una rama y `ValueType` un valor. El eco de contrato
  está gateado por el entero, no por funcionalidad: `veredicto_del_eco`
  (`crates/motor/src/lib.rs:427`) da `error` si el paso usa `inputs:` y el eco es
  menor que `CONTRACT`. Subir a 4 convierte en `error` **todo** paso con
  `inputs:` contra cualquier ejecutor que hable 3, use referencias o no. Es un
  flag day, se asume —pre-v1, no hay compromiso de retrocompatibilidad— y va en
  el CHANGELOG con esas palabras, no como nota al pie.
- **El informe no sale gratis.** `valor_a_json`
  (`crates/result_sink/src/json.rs:127`) y `nombrados_a_csv`
  (`crates/result_sink/src/csv.rs:133`) son `match` exhaustivos: una cuarta
  variante no compila hasta decidir su forma. Y en CSV el par va como
  `nombre=valor` unido por `;`, mientras `csv_campo` (`csv.rs:150`) sólo escapa
  coma, comilla, CR y LF: **una carga opaca que contenga `;` o `=` corrompe la
  celda en silencio.** La forma exacta y el escapado se deciden al implementar;
  que no se corrompa nada es requisito.
- **La trazabilidad se mantiene y no cuesta nada.** La fila del paso en el
  informe lleva `inputs` y `outputs` a la vez, así que queda escrito con qué
  banco se hizo cada medida. Es la Regla 3 de ADR-0019 sobreviviendo a un patrón
  que a primera vista parecía saltársela.
- **`--validate` sin `--with-executors` no comprueba tipos de referencia contra
  el catálogo**, porque sin ese modo no hay catálogo
  (`crates/motor/src/bin/anvil.rs:275`). Lo que sí cabe en el modo por defecto es
  lo declarado: el tipo de la variable en `locals:` y a qué ejecutor se despacha
  cada paso, que `resolver_endpoint` (`crates/motor/src/lib.rs:149`) resuelve sin
  red.
- **Riesgo aceptado de la ranura:** invita al ejecutor a esconder cambios de
  estado — un paso que reconfigura el banco entero y no cambia la identidad no
  deja rastro en la secuencia. La defensa no es el tipo, es que ese paso escriba
  en el informe lo que hizo (Regla 3 de ADR-0019), y eso no depende de esta
  decisión.

## Lo que queda abierto, y a propósito

- **El nombre del ejecutor no identifica un proceso.** `resolver_endpoint`
  (`crates/motor/src/lib.rs:149`) colapsa todo `type: embedded` al centinela
  `__anvil_embebido__` (`crates/cargador/src/lib.rs:293`), y
  `aplicar_override_ejecutores` (`cargador/src/lib.rs:391`) puede re-apuntar dos
  nombres al mismo `host:puerto`. Si la referencia lleva el nombre del YAML, el
  chequeo rechaza casos legítimos; si lleva el endpoint, no es lo que dice el §4.
- **La corrida que se para en seco no ejecuta el `cleanup`:** si `unaria`
  devuelve `Err`, `ejecuta_secuencia_interna` (`crates/motor/src/lib.rs:511`)
  sale antes del bucle de cierre. Justo cuando salta la comprobación de vida, el
  paso que cerraría el rack no corre. Si esa comprobación aborta o produce un
  `ResultadoStep` en `error` que deja seguir al cleanup, se decide al
  implementar: sólo lo segundo le da a Anvil ocasión de cerrar algo.
- **Huérfanos por aborto brusco**, y la referencia que nadie sostiene ya. La
  ranura los reduce a caso raro, no los elimina.
- **El process model no tiene canal para pasar un rack.** ADR-0016 exige firma
  vacía en la raíz del usuario, y un mismo nombre de ejecutor en el PM y en el
  fichero del usuario es error de carga (`crates/cargador/src/lib.rs:1363`). Hoy
  un rack abierto en el `setup` del PM no puede llegar a la secuencia del
  usuario, que es justo donde el issue #55 quería colgar el ciclo de vida.
- **Paralelismo (RF-39):** si la vida es la del proceso o la de la corrida
  cambia quién la acuña, y esto no lo decide.
- **`StationGlobals` (RF-32):** dónde vive una referencia que sobrevive a varias
  unidades.
- **La composición** —contenedores y arrays—, que es la *otra* ausencia frente a
  TestStand y la que hace falta para devolver un barrido de cien puntos. Es un
  hueco distinto y este ADR no lo toca.
