# Diseño: Modelo de pasos

> **Prioridad:** MVP-parcial. El adapter gRPC **ya existe**; los built-in y
> el registro/descubrimiento de pasos son MVP/MVP-parcial; los custom step
> types son post-MVP.

Cómo se define, registra, descubre y versiona un paso. Trazable a
`ejemplos/departamento/demo/src/lib.rs` (el banco de demo) y [ADR-0003](../adr/0003-pasos-por-grpc-por-nombre.md).

## El adapter es gRPC

En TestStand, un *adapter* es el puente a un lenguaje (LabVIEW, C/C++, .NET,
Python). En Anvil **el adapter es gRPC**: cualquier lenguaje que hable el
contrato protobuf es un adapter, sin código de pegamento en el motor y sin
runtime de vendor (ADR-0003). Un paso lo sirve un ejecutor en su propio
proceso —el puente WASM, Python o C#—; `anvil` no lleva ninguno
([ADR-0041](../adr/0041-there-is-no-embedded-executor.md)).

**Materializado en M5-ext (ADR-0013/0014):** Anvil distribuye **executores de
lenguaje** como módulos (`executors/`, primero Python) y el routing
**nombre→endpoint** (M5-ext.1, hecho: `ejecutores:`/`ejecutor:` en el YAML +
override `--executor`). El **cargador de `.wasm` por path** (modelo `.vi`:
compilar y referenciar, sin recompilar) está **hecho (M5-ext.2)** y lo hace
el **host** (un guest WASM no puede instanciar wasmtime dentro de sí mismo;
ADR-0013/0014). Ver
[diseno/executores-lenguaje.md](executores-lenguaje.md).

## Despacho por nombre

El motor pide un paso por `nombre`; el ejecutor lo ata a una función. El
despacho es el **único** punto donde el nombre del cable se ata a código. En
el puente WASM, `resolve` (`executors/wasm/src/main.rs`) elige el módulo por
el prefijo `<módulo>/` y el registro del SDK
(`executors/rust/anvil-step/src/registry.rs`) elige la función; un paso que el
componente no sirve devuelve:

```rust
Outcome::error(format!(
    "this component does not serve a step called '{}' (it serves: {})",
    ctx.step_name, known
))
```

Un nombre desconocido es `error`, **no pánico**: una secuencia mal escrita
no tumba el ejecutor (RF-12).

## Los tipos de paso

Un paso declara su `type`, y **es obligatorio**: dice *cómo se juzga el paso*,
no qué llama ([ADR-0040](../adr/0040-a-step-type-says-how-a-step-is-judged-not-what-it-calls.md)).
Lo que llama es `module`, y son cosas distintas: dos pasos pueden llamar al
mismo `module` y juzgarse de forma distinta, y un `pass_fail` puede no llamar a
nadie.

| Tipo | Cómo se juzga | Necesita |
|---|---|---|
| **`action`** | No juzga. El `pass` del módulo se informa como `done`; su `fail` o su `error` se mantienen. | `module` |
| **`pass_fail`** | El estado del módulo **es** el veredicto; si hay `condition`, decide la expresión (y puede leer `result.*`). | `module`, `condition`, o los dos |
| **`numeric_limit`** | El motor compara un número contra el `limit` (ADR-0008). El número es `value`, o la medida del módulo. | `limit`, y `module` o `value` |
| **`statement`** | No juzga: si las sentencias terminan bien, `done`. | `statement` |
| **`sequence_call`** | El agregado de la subsecuencia. | `sequence` |

Los tipos son del **motor**, no del ejecutor: el ejecutor sólo devuelve
`pass`/`fail`/`error`/`skipped` y, si mide, un número; quién decide qué
significa eso es el motor, con lo que dice la secuencia. Sigue siendo genérico
(ADR-0005): ninguno de los cinco sabe qué se está midiendo.

### Por qué `done` y no `pass`

Un `action` que abre un relé no ha juzgado la unidad, así que informarlo como
`pass` era decir que algo salió bien cuando lo único que pasó es que se hizo.
`done` es **neutral**: no corta el Main, no hace verde una secuencia por sí
solo y no cuenta como medida (ADR-0040 §6). Lo mismo para un `statement` y para
un `numeric_limit` con `comparison: none`, que registra el valor sin
compararlo.

### Cómo se encarnan

- **`action` y `pass_fail`** no necesitan lógica en el ejecutor: son pasos
  normales que devuelven `pass`/`fail`/`error`. En el banco de demo,
  `demo/check_led` es un `pass_fail` y `demo/open_relay` un `action`.
- **`numeric_limit`** se apoya en los **límites como datos** (RF-29,
  [limites-y-estados.md](limites-y-estados.md)): el módulo mide y devuelve
  `valor_medido`; el motor evalúa el `Limite` del YAML y produce el estado
  (ADR-0008). No hace falta un paso «limit test» dedicado ni tocar el
  contrato. Un `numeric_limit` **sin número que juzgar** es `error`, no un
  límite que no se aplica en silencio (ADR-0040 §5).
- **`statement`** se implementó en M4-núcleo; **`sequence_call`**, en M4b (ver
  abajo).

### El veredicto compuesto (`type: pass_fail` sin `module`, ADR-0018)

Las dos vías anteriores fallan sobre **un** paso y **una** medida: el paso lo
decide, o el motor evalúa el `limit` de su medida. El criterio de aceptación
que **combina varias medidas** —el que un ingeniero escribe al final de la
secuencia— es un `pass_fail` que no llama a nadie:

```yaml
- name: verificar_dut
  type: pass_fail
  condition: 'locals.v > 4.9 && locals.v < 5.1 && locals.temp < 50.0'
```

Lo evalúa el **motor**, no el paso (mismo patrón que `limit` y `precondition`):
`true` → `pass`, `false` → `fail`, no-Bool → `error`. Bool estricto, sin
truthiness. Es el análogo del step type `Pass/Fail Test` de TestStand, cuyo
data source es una expresión booleana.

`statement` se queda **sólo con asignación**, a propósito: cada construcción
hace una cosa, y así olvidar un `=` sigue siendo un error de sintaxis en vez de
un cambio silencioso de significado. Un `pass_fail` o un `numeric_limit` **sin
`module`** no admite `retries > 1` (evalúa una expresión pura: el veredicto no
cambia entre intentos), ni `assign`, ni `executor`; **con `module`** admite los
dos primeros, porque entonces sí llama a alguien (ADR-0042 §3). Un `pass_fail`
nunca admite `limit`. Todo lo demás es error al cargar.

### Cómo se encarna sequence call en M4b

- **Motor-side, sin gRPC**: el motor orquesta la subsecuencia contra su
  propio `EntornoMotor`; `paso.proto` no cambia (ADR-0010). El resultado se
  anida en `ResultadoStep.sub_pasos` con el estado agregado de la subsec.
- **Inline o por path**: la subsecuencia se declara bajo `subsecuencias:`
  del mismo archivo (invocada por **nombre**) o en un **archivo aparte**
  (invocada por **path relativo**). Inline = privada del archivo; por path =
  pública y reutilizable.
- **Parameters de entrada/salida by-reference** (como TestStand): el call
  mapea cada `Parameter` a un `locals.X` del padre — copia `locals.X` →
  `parameters.P` al iniciar y `parameters.P` (final) → `locals.X` al volver.
  La subsecuencia escribe en sus `parameters` (relajación acotada de "sólo se
  muta Locals"; el paso gRPC sigue aislado).
- El **cargador** resuelve paths, valida lvalues y firma, y detecta ciclos
  al cargar (fail-fast); el motor no abre ficheros (ADR-0005).

  Ver [variables-y-alcances.md](variables-y-alcances.md),
  [formato-de-secuencia.md](formato-de-secuencia.md) y ADR-0010.

## Registro y descubrimiento de pasos (hecho)

Un ejecutor **describe su catálogo**: qué módulos sirve, con qué entradas y
qué salidas (ADR-0021, y en Rust por firma, ADR-0024). El registro del SDK es
lo que despacha y lo que se publica, así que no hay dos listas que puedan
divergir.

- El motor lo consulta nada más conectar, antes del primer paso, y sin banco
  con `--validate --with-executors`; compara contra la secuencia los `inputs` y
  los `result.outputs.<nombre>` (`motor::comprueba_programa`). Un ejecutor que
  no describe su catálogo se queda sin comprobar, y se avisa: negarse a correr
  cerraría la puerta a terceros.
- Es la base de la **introspección de firma** que necesita el editor
  visual (ver [ui-vs-headless.md](ui-vs-headless.md) y
  [contrato-grpc.md](../contrato-grpc.md)): el registro pasa de "nombre" a
  "nombre + parámetros + retorno", para que arrastrar el archivo del code
  module auto-pueble la tabla de parámetros como en TestStand.

## Versionado de pasos (MVP-parcial, aplazado a post-M3)

Un paso puede evolucionar (firmas, semántica). Propuesta:

- Un paso declara su **versión** en el registro (p. ej. `medir_voltaje@1`).
- La secuencia referencia un paso **por nombre**, opcionalmente con versión
  mínima; si el ejecutor ofrece una menor → `error`.
- El **contrato** (`paso.proto`) es lo estable; la versión del paso es
  metadata del registro, no del wire del `Invoca`.

## Custom step types (post-MVP)

Step types definidos por el usuario que **encapsulan** comportamiento
repetitivo (un *custom* "medir y comparar contra límite de este lote"). En
TestStand llevan substeps (Edit/Pre/Step/Post/OnNewStep). En Anvil, post-MVP;
no se replica el sistema de substeps de TestStand 1:1 (complejo y frágil,
[investigación](../investigacion/TestStand-y-competencia.md) §2). Un custom
type será, probablemente, una **secuencia parametrizada** reutilizable
(sequence call con parámetros), no un substep framework.

## Out-of-scope

- Substeps Pre/Run/Post heredados de TestStand.
- Editor de custom step types con paneles (C# pane de TestStand).