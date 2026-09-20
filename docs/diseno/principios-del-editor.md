# Diseño: Principios del editor (AP)

> **Prioridad:** vigente. Gobierna todo `editor/`.

Los **AP** (*Anvil Principles*) son las reglas que sostienen el editor de
secuencias. Se citan en el código por su número —`(AP-04)`— y no se repiten en
prosa allí donde se aplican: la cita es el enlace a este documento.

Este fichero se escribió **después** que las citas. Hasta
[ADR-0043](../adr/0043-the-editor-is-laid-out-as-teststand-and-declares-what-it-does-not-do.md)
los AP vivían en una conversación y se citaban 24 veces en seis ficheros sin
estar definidos en ninguna parte del repositorio. Lo que sigue es su
reconstrucción a partir de esas citas, que son la única fuente que quedaba.

## Los huecos, y por qué se dejan

Sólo seis números aparecen citados: **02, 03, 04, 05, 07 y 12**. Faltan el 01,
el 06 y del 08 al 11.

**Los huecos se reservan, no se renumeran.** Que el más alto sea el 12 dice que
la serie original tenía al menos doce, así que renumerar los seis
supervivientes rompería el significado de cada cita del código a cambio de una
lista bonita. Un número reservado es un principio que existió y que no se ha
recuperado; si aparece, vuelve a su sitio. Un principio nuevo toma el siguiente
número libre por arriba.

| | Estado |
|---|---|
| AP-01 | **No recuperado.** Sin ninguna cita en el repositorio. |
| AP-02 | Recuperado |
| AP-03 | Recuperado |
| AP-04 | Recuperado |
| AP-05 | Recuperado |
| AP-06 | **No recuperado.** |
| AP-07 | Recuperado |
| AP-08 … AP-11 | **No recuperados.** |
| AP-12 | Recuperado |
| AP-13 | Nuevo (ADR-0043) |

---

## AP-02 — El layout es el de TestStand, recortado a lo que el motor sabe hacer

La arquitectura de información del editor es la del Sequence Editor de
TestStand: dónde está cada cosa y cómo se llama lo decidieron veinticinco años
de uso en producción, y quien llega a Anvil llega de ahí.

Lo que se copia es **dónde se busca un ajuste**, no el widget que lo pinta ni
la semántica que hay detrás. `comparison: none` da `done` donde TestStand da
*Passed* ([ADR-0040](../adr/0040-a-step-type-says-how-a-step-is-judged-not-what-it-calls.md) §8),
y eso no es un fallo de paridad: es la paridad que no se firma.

AP-13 dice qué pasa con lo que el motor todavía no sabe hacer.

> Citado en `editor/index.html:11`, `editor/src/app.mjs:3`.

## AP-03 — Sin control de flujo anidado, porque el motor no lo tiene

La lista de pasos es plana por fase. El motor tiene `precondition`,
`condition`, `statement` y la llamada a subsecuencia; no tiene `if`, ni bucles,
ni bloques anidados. El editor no ofrece ninguno.

Es AP-04 aplicado a la forma de la lista: un árbol en pantalla prometería un
árbol en el fichero.

> Citado en `editor/src/app.mjs:6`, `editor/src/document.mjs:159`.

## AP-04 — El editor no puede construir una secuencia que el cargador rechace

**El principio que sostiene el resto.** Es el más citado del repositorio, con
diferencia, y del que cuelgan las decisiones más caras del editor.

Ninguna secuencia de clicks puede producir un fichero que
`crates/cargador/src/lib.rs` refuse. No «avisa de», no «marca en rojo»:
**no puede**. Pedirle a alguien que arregle un fichero inválido que el editor
acaba de escribir es exactamente lo que AP-04 prohíbe.

De aquí salen, entre otras:

- El vocabulario del documento es el del cargador, nunca inventado aquí: cada
  clave de `document.mjs` es una que el cargador acepta, y el cargador rechaza
  las demás con `deny_unknown_fields`.
- Elegir un código de comparación **escribe sus límites**: un `GELE` sin cotas
  no carga, así que el código no se puede elegir «para rellenar luego».
- Insertar un paso escribe los campos que su tipo exige —`statement` en un
  statement, `condition` en un pass_fail, `sequence` en un sequence_call—
  porque un paso al que le faltan se rechaza al cargar.
- Cambiar el `type` de un paso **borra los campos que el tipo nuevo refuse**.
  Antes no lo hacía, y un click del menú de tipos dejaba la secuencia sin
  cargar.
- Nombrar un `module` rellena el ejecutor que lo sirve: no hay ejecutor por
  defecto donde caerse ([ADR-0041](../adr/0041-there-is-no-embedded-executor.md)).
- La paleta no ofrece un tipo de paso que el cargador no conozca.

Su consecuencia hacia el motor está escrita en `editor/README.md`: *cualquier
cosa que exigiera cambiar el motor para acomodar al editor es un error de
diseño*, y se sostiene porque el editor nunca le pide al motor algo que el
motor no haga ya.

> Citado en `editor/src/app.mjs:8,326`, `editor/index.html:12`,
> `editor/src/document.mjs:16,67,159,220,261,272,375,464,569`,
> `editor/README.md:64`, `editor/test/insert.test.mjs:2`,
> `editor/test/authoring.test.mjs:278`.

## AP-05 — Las secuencias se revisan como diffs

El texto es la fuente de verdad y la vista de pasos es una proyección editable
de él, no un modelo paralelo que se serializa encima. Editar un campo muta su
nodo y se re-emite el texto, así que **comentarios, orden de claves y formato
sobreviven**.

No es una cortesía. Las secuencias de Anvil se escriben a mano, viven en git y
se revisan en diffs; `ejemplos/basica.yseq` abre con nueve líneas que explican
de dónde sale su umbral y qué ADR lo decidió. Un editor que reformatea eso al
guardar es un editor que se abandona a los dos usos. **«Una línea cambiada» es
el requisito, no el adorno.**

> Citado en `editor/src/document.mjs:6,390`,
> `editor/test/document.test.mjs:7`, `editor/test/authoring.test.mjs:244`.

## AP-07 — El motor corre dentro del editor

El editor hospeda **el mismo** `anvil-guest.wasm` que embebe el binario nativo
([ADR-0011](../adr/0011-distribucion-un-binario-hospeda-wasmtime.md)),
transpilado a JavaScript. No hay un fork del motor para la UI
([ADR-0031](../adr/0031-one-engine-two-front-ends.md)): es el mismo componente
con otro proveedor de sus imports WASI.

Consecuencia práctica: **abrir, cargar, validar e informar no necesitan
wasmtime, ni bridge, ni banco.** Lo que el editor dice de un fichero es lo que
dirá la línea de comandos, porque lo dice el mismo código.

Correr de verdad sí necesita el bridge, que es donde el editor alcanza la red.

> Citado en `editor/README.md:6`, `editor/test/validate.test.mjs:6`.

## AP-12 — Una vista que no está al día lo dice

El editor nunca aparenta estar mostrando algo que no está mostrando.

El caso que lo motiva: mientras el texto se está escribiendo y no parsea, la
vista de pasos enseña **el último árbol que sí parseó, marcado como viejo**.
Congelar en silencio sería mentir y quedarse en blanco sería inservible; la
tercera opción es la única honesta.

La marca tiene que verse **sin leerse** —nadie lee la barra de estado mientras
teclea—, así que los paneles proyectados se atenúan
(`editor/src/style.css:1079`). Y mientras el texto no parsea, el error de YAML
*es* la respuesta: el motor no tiene nada mejor que decir y un segundo parser
sólo daría una versión peor del mismo error.

Es el mismo principio que la Regla 2 de
[ADR-0019](../adr/0019-que-hace-anvil-cuando-no-puede-juzgar.md) —el silencio
nunca se lee como «bien»— aplicado a la interfaz en vez de al veredicto.

> Citado en `editor/src/document.mjs:13`, `editor/src/app.mjs:1941`,
> `editor/src/style.css:1079`.

## AP-13 — Un hueco de paridad se declara donde va a aparecer

Lo que TestStand tiene y Anvil no **no se omite de la interfaz**: sale en su
sitio, en gris, diciendo qué hace TestStand ahí y en cuál de tres situaciones
está Anvil — `todo`, `elsewhere` o `never`.

Así el inventario de lo que falta vive donde no puede quedarse obsoleto, en vez
de en un documento que nadie reabre.

Y la dirección es siempre la misma: **el motor desbloquea la casilla, nunca al
revés.** Una función se implementa y se verifica headless —CLI y test— y sólo
entonces el editor deja de pintarla en gris. Es AP-04 visto desde el otro lado:
si el editor pudiera adelantarse, podría ofrecer lo que el cargador rechaza.

El mecanismo, el alcance y la definición de los tres veredictos están en
[ADR-0043](../adr/0043-the-editor-is-laid-out-as-teststand-and-declares-what-it-does-not-do.md);
el inventario, en `editor/src/paridad.mjs` y en la página que se genera de él,
[`docs/paridad-teststand.md`](../paridad-teststand.md).
