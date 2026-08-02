# Diseño: Motor de expresiones

> **Prioridad:** MVP-parcial. **Subconjunto MVP implementado en M4-núcleo**
> (`crates/expr`); avanzado post-MVP.

Expresiones para precondiciones, postcondiciones, límites y asignaciones
**sin código pegamento** (investigación §1.4). Resuelve el cableado de datos
entre pasos y la lógica condicional en una secuencia que es *datos*
(ADR-0002).

## Decisión clave: sintaxis Julia, NO C-like

TestStand usa un expression engine con **sintaxis tipo C**. Anvil **no la
copia**: apunta a **Julia**, el estándar técnico moderno, porque es limpio,
estrictamente tipado y coherente. Es una **divergencia deliberada** de TestStand
para bajar la barrera de adopción y alinearse con un lenguaje que los
ingenieros técnicos actuales reconocen.

Consecuencias del estilo Julia:

- Asignación con `=`.
- Comparaciones `==`, `!=`, `<`, `>`, `<=`, `>=`, **encadenables** como en
  Julia: `1 < x < 10` ≡ `1 < x && x < 10`.
- Operadores lógicos `&&`, `||`, `!` (Julia), con cortocircuito.
- Ausencia se escribe `nothing` (como en Julia).
- Acceso a campos con `.`: `locals.voltaje_leido`, `resultado.valor_medido`.
- **Sin truthiness implícita** (igual que Julia): `if x` exige `Bool`; un
  no-bool es error de tipo. Una precondición debe ser bool.

## Subconjunto MVP (implementado en M4-núcleo)

Lo mínimo para que una secuencia sea útil sin volver a meter código:

- **Asignación:** `locals.voltaje_leido = resultado.valor_medido`
- **Aritmética:** `+ - * /` sobre números.
- **Comparación y lógica:** `== != < > <= >=` (encadenables) y `&& || !`.
- **Ausencia:** `nothing` (p. ej. `resultado.valor_medido != nothing`).
- **Acceso a variables por scope:** `locals.*`, `parameters.*`,
  `file_globals.*` (ver [variables-y-alcances.md](variables-y-alcances.md)).
- **Acceso al resultado del paso:** `resultado.estado`, `resultado.valor_medido`.

## Dónde se evalúa

- **Precondición** (por paso): `if precondicion → se ejecuta; si no, se
  salta` (RF-33). El motor evalúa **antes** de invocar el paso; si es falsa,
  no gasta un intento.
- **Postcondición / límite:** evalúa el resultado contra un umbral
  (alternativa al `limite` declarado — ver [limites-y-estados.md](limites-y-estados.md)).
- **Asignación:** tras el paso, vuelca campos del resultado a variables.

El motor **evalúa** las expresiones (es dato), pero **no** conoce el
dominio del paso: solo manipula variables y el `resultado` genérico
(ADR-0005 se respeta).

## Implementación (propuesta)

Un intérprete de expresiones **pequeño y determinista**, sin dependencias
externas pesadas (para compilar a WASM, ADR-0001). Sin `eval` de código
arbitrario: las expresiones son un AST acotado, no un lenguaje Turing
completo en el MVP.

### Implementación en M4-núcleo (`crates/expr`)

- **`Value`**: `Numero(f64)`, `Bool(bool)`, `Texto(String)`, `Nulo` (ausencia,
  escrito `nothing` en el lenguaje). Reglas de tipo **estrictas** (sin
  coerción silenciosa ni truthiness, como Julia): `+ - * /` y comparaciones de
  orden sólo sobre `Numero`; `&&`/`||`/`!` sólo sobre `Bool` (con
  cortocircuito); `==`/`!=` entre tipos distintos → `false`/`true` (no error).
  `Nulo` en aritmética/orden → error (fuerza precondiciones explícitas tipo
  `resultado.valor_medido != nothing`); `Nulo == Nulo` → `true`.
- **AST**: `Expresion` (produce `Value`, no muta) separada de `Sentencia`
  (muta `Locals`). `Assign` es una sentencia, no una expresión.
- **Parser** recursive-descent a mano (sin `nom`/`lalrpop`, ADR-0001).
  Precedencia (igual que Julia): `||` < `&&` < comparaciones **encadenables**
  (`a < b < c` ≡ `(a<b) && (b<c)`) < `+ -` < `* /` < unarios `-`/`!` < `.` <
  átomo. Palabras clave reservadas: `true`, `false`, `nothing` (no pueden ser
  campos). Operadores lógicos: `&&`, `||`, `!` (no `and`/`or`/`not`).
- **`Entorno`** (trait en `expr`): `lee(scope, campo)` / `escribe(...)`. El
  motor lo implementa (`EntornoMotor`, `crates/motor/src/entorno.rs`): lectura
  laxa de `resultado.*`, estricta de `locals`/`parameters`/`file_globals`;
  escritura sólo en `Locals`. El engine **nunca pánico**: toda API devuelve
  `Result<_, ErrorExpr>` con posición en el texto fuente.
- **Parseo al cargar** (fail-fast): el cargador parsea `precondicion`/
  `asigna`/`statement` a AST al cargar; un error de sintaxis →
  `ErrorCarga::Validacion` con el nombre del paso (ADR-0009).

Ver ADR-0009 para la decisión de que el motor evalúa contra su entorno y el
cableo al paso por el wire es post-MVP.

## Avanzado (post-MVP)

- Funciones (p. ej. `abs()`, `min()`, `max()`, conversiones de unidades).
- Cadenas de texto y formateo.
- Acceso a resultados de pasos anteriores por nombre.
- Expresiones en bucles/condicionales de flujo (si se añaden).

## Out-of-scope

- Un lenguaje de scripting completo embebido (Lua, Python embebido): las
  expresiones son **datos acotados**, no código arbitrario. Si se necesita
  lógica compleja, va en un **paso** (código real, any language, ADR-0003).