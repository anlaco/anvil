# Diseño: Modelo de pasos

> **Prioridad:** MVP-parcial. El adapter gRPC **ya existe**; los built-in y
> el registro/descubrimiento de pasos son MVP/MVP-parcial; los custom step
> types son post-MVP.

Cómo se define, registra, descubre y versiona un paso. Trazable a
`crates/pasos_demo/src/lib.rs` y [ADR-0003](../adr/0003-pasos-por-grpc-por-nombre.md).

## El adapter es gRPC

En TestStand, un *adapter* es el puente a un lenguaje (LabVIEW, C/C++, .NET,
Python). En Anvil **el adapter es gRPC**: cualquier lenguaje que hable el
contrato protobuf es un adapter, sin código de pegamento en el motor y sin
runtime de vendor (ADR-0003). Hoy el prototipo hospeda `pasos_demo` en el
mismo `.wasm` del ejecutor; el objetivo es que un paso pueda ser un
**servidor gRPC en cualquier lenguaje**.

**Materializado en M5-ext (ADR-0013/0014):** Anvil distribuye **executores de
lenguaje** como módulos (`executores/`, primero Python) y el routing
**nombre→endpoint** (M5-ext.1, hecho: `ejecutores:`/`ejecutor:` en el YAML +
override `--executor`). El **cargador de `.wasm` por path** (modelo `.vi`:
compilar y referenciar, sin recompilar) está **hecho (M5-ext.2)** y lo hace
el **host** (un guest WASM no puede instanciar wasmtime dentro de sí mismo;
ADR-0013/0014). Ver
[diseno/executores-lenguaje.md](executores-lenguaje.md).

## Despacho por nombre

El motor pide un paso por `nombre`; el ejecutor lo ata a una función. El
despacho es el **único** punto donde el nombre del cable se ata a código
(hoy `pasos_demo::despacha`):

```rust
match nombre {
    "conectar_equipo" => conectar(intento),
    "medir_voltaje"   => medir_voltaje(intento),
    ...
    _ => ResultadoStep::nuevo("desconocido", "error", "paso no reconocido"),
}
```

Un nombre desconocido es `error`, **no pánico**: una secuencia mal escrita
no tumba el ejecutor (RF-12).

## Step types built-in (MVP)

| Tipo | Qué hace | MVP |
|---|---|---|
| **pass/fail** | Hace algo y reporta `paso`/`fallo` sin medida. El más simple. | ✅ hecho (M3) |
| **limit test** | Mide y compara contra high/low o comparación → `paso`/`fallo`. | ✅ hecho (M3) |
| **action** | Ejecuta una acción (mover un fixture, abrir un relé); el estado es `paso` si no hubo `error`. | MVP-parcial · hecho (M3) |
| **sequence call** | Invoca otra secuencia anidada. | MVP-parcial · hecho (M4b) |
| **statement** | Evalúa una expresión del expression engine (asignación). | MVP-parcial · hecho (M4-núcleo) |
| **pass_fail (por expresión)** | El **motor** evalúa una `condicion` booleana sobre variables ya pobladas → `paso`/`fallo`. El veredicto **compuesto**. | ✅ hecho (post-MVP, ADR-0018) |

Los built-in son **comportamientos** del lado del ejecutor, no del motor:
el motor sigue siendo genérico (ADR-0005).

### Cómo se encarnan en M3

- **pass/fail** y **action** no necesitan lógica nueva: son pasos normales que
  devuelven `paso`/`fallo`/`error` (con o sin medida). `pasos_demo::verificar_led`
  es pass/fail; `pasos_demo::abrir_rele` es action.
- **limit test** se habilita con los **límites como datos** (RF-29,
  [limites-y-estados.md](limites-y-estados.md)): el paso mide y devuelve
  `valor_medido`; el motor evalúa el `Limite` del YAML y produce el estado
  (ADR-0008). No hace falta un paso "limit test" dedicado ni tocar el
  contrato — cualquier paso que mida puede llevar un límite declarado.
- **sequence call** y **statement** quedan para M4: dependen, respectivamente,
  de la infraestructura de subsecuencias y del *expression engine* (RF-35).
  **statement** se implementó en M4-núcleo; **sequence call**, en M4b (ver
  abajo).

### El veredicto compuesto (`tipo: pass_fail`, ADR-0018)

Las dos vías anteriores fallan sobre **un** paso y **una** medida: el paso lo
decide, o el motor evalúa el `limite` de su medida. El criterio de aceptación
que **combina varias medidas** —el que un ingeniero escribe al final de la
secuencia— es un paso `pass_fail`:

```yaml
- name: verificar_dut
  type: pass_fail
  condition: 'locals.v > 4.9 && locals.v < 5.1 && locals.temp < 50.0'
```

Lo evalúa el **motor**, no el paso (mismo patrón que `limite` y `precondicion`):
`true` → `paso`, `false` → `fallo`, no-Bool → `error`. Bool estricto, sin
truthiness. Es el análogo del step type `Pass/Fail Test` de TestStand, cuyo
data source es una expresión booleana.

`statement` se queda **sólo con asignación**, a propósito: cada construcción
hace una cosa, y así olvidar un `=` sigue siendo un error de sintaxis en vez de
un cambio silencioso de significado. Un `pass_fail` no admite `reintentos > 1`
(evalúa una expresión pura: el veredicto no cambia entre intentos), ni
`asigna`, ni `limite`, ni `ejecutor` — todos son error al cargar.

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

## Registro y descubrimiento de pasos (MVP-parcial, aplazado a post-M3)

Hoy el despacho es un `match` hardcodeado. Para que un ejecutor pueda
**descubrir** qué pasos ofrece, y para que un editor los liste, hace falta
un **registro** de pasos:

- **Propuesta:** un ejecutor expone el catálogo de pasos que despacha (nombre
  +, post-MVP, su firma). El motor/editor lo consulta.
- Esto es la base de la **introspección de firma** que necesita el editor
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