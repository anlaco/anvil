# Diseño: Motor de expresiones

> **Prioridad:** MVP-parcial. **Propuesta** (no implementado). Subconjunto
> en MVP, avanzado post-MVP.

Expresiones para precondiciones, postcondiciones, límites y asignaciones
**sin código pegamento** (investigación §1.4). Resuelve el cableado de datos
entre pasos y la lógica condicional en una secuencia que es *datos*
(ADR-0002).

## Decisión clave: sintaxis Python/Scilab/MATLAB-like, NO C-like

TestStand usa un expression engine con **sintaxis tipo C**. Anvil **no la
copia**: apunta a una sintaxis familiar para ingenieros de test, cercana a
**Python / Scilab / MATLAB** (lo que esa audiencia ya maneja), porque son
los lenguajes que los ingenieros de test usan más. Es una **divergencia
deliberada** de TestStand para bajar la barrera de adopción.

Consecuencias del estilo:

- Asignación con `=` (Python) o sin declarar tipo.
- Comparaciones `==`, `!=`, `<`, `>`, `<=`, `>=` (comunes a ambos).
- Operadores lógicos legibles: `and`, `or`, `not` (Python) preferidos sobre
  `&&`/`||`/`!` (C) — más legible para no-programadores.
- Indexado y campos con `.` (Python/Scilab): `locals.voltaje_leido`,
  `resultado.valor_medido`.

## Subconjunto MVP (propuesta)

Lo mínimo para que una secuencia sea útil sin volver a meter código:

- **Asignación:** `locals.voltaje_leido = resultado.valor_medido`
- **Aritmética:** `+ - * /` sobre números.
- **Comparación y lógica:** `== != < > <= >=` y `and or not`.
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

## Avanzado (post-MVP)

- Funciones (p. ej. `abs()`, `min()`, `max()`, conversiones de unidades).
- Cadenas de texto y formateo.
- Acceso a resultados de pasos anteriores por nombre.
- Expresiones en bucles/condicionales de flujo (si se añaden).

## Out-of-scope

- Un lenguaje de scripting completo embebido (Lua, Python embebido): las
  expresiones son **datos acotados**, no código arbitrario. Si se necesita
  lógica compleja, va en un **paso** (código real, any language, ADR-0003).