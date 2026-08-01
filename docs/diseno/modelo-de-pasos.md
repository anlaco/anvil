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
| **pass/fail** | Hace algo y reporta `paso`/`fallo` sin medida. El más simple. | ✅ |
| **limit test** | Mide y compara contra high/low o comparación → `paso`/`fallo`. | ✅ |
| **action** | Ejecuta una acción (mover un fixture, abrir un relé); el estado es `paso` si no hubo `error`. | MVP-parcial |
| **sequence call** | Invoca otra secuencia anidada. | MVP-parcial |
| **statement** | Evalúa una expresión del expression engine (asignación). | MVP-parcial |

Los built-in son **comportamientos** del lado del ejecutor, no del motor:
el motor sigue siendo genérico (ADR-0005).

## Registro y descubrimiento de pasos (MVP-parcial, pendiente)

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

## Versionado de pasos (MVP-parcial, pendiente)

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