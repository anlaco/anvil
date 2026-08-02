# ADR-0009: Las expresiones, precondiciones y asignaciones las evalúa el motor contra su entorno, no el paso

- **Estado:** Aceptada
- **Fecha:** 2026-08-02 (M4-núcleo)
- **Relaciona:** ADR-0005, ADR-0008, [contrato-grpc.md](../contrato-grpc.md),
  [variables-y-alcances.md](../diseno/variables-y-alcances.md),
  [motor-de-expresiones.md](../diseno/motor-de-expresiones.md)

## Contexto

M3 (ADR-0008) sentó el precedente: el paso devuelve la medida y el **motor**
evalúa el `Limite` declarado como dato, produciendo el estado final sin tocar el
contrato `paso.proto`. Ese patrón funcionó para los límites (RF-29).

M4 necesita lógica condicional y cableado de datos **sin volver a meter código**
en la secuencia (ADR-0002): precondiciones por paso (RF-33), asignaciones de
campos del resultado a variables (RF-31) y un tipo de paso `statement` que
ejecuta una expresión localmente (RF-27). Todas estas son **expresiones** en un
lenguaje acotado de sintaxis **Julia** (RF-35), el estándar técnico moderno,
no código arbitrario.

La pregunta es quién evalúa esas expresiones y dónde vive el estado (las
variables), respetando ADR-0005 (motor genérico, no conoce el dominio) y el
contrato `paso.proto` (ADR-0003).

## Decisión

El **motor** evalúa las expresiones contra un **entorno de variables** que él
mismo posee y mantiene. Las expresiones son datos en el YAML; el contrato
`paso.proto` **no cambia** — los parámetros tipados no viajan por el wire en
MVP.

Concretamente:

- Un **expression engine** (`crates/expr`) implementa el lenguaje: lexer + AST +
  parser + evaluator. No depende de `modelo` ni conoce `ResultadoStep`; opera
  sobre un `Value` propio y un trait `Entorno` que el motor implementa
  (`EntornoMotor`, en `crates/motor/src/entorno.rs`).
- El **cargador** parsea las expresiones a AST **al cargar** (fail-fast,
  consistente con `deny_unknown_fields` y con la validación de límites en M3).
  Un error de sintaxis se reporta con el nombre del paso, no a mitad de una
  corrida.
- El **motor** evalúa:
  - **Precondición** (RF-33) antes de invocar el paso; si es falsa, lo salta sin
    gastar intento. Bool estricto: un no-bool es error de definición.
  - **Asigna** (RF-31) tras un paso `Grpc`, volcando campos de `resultado` a
    `Locals`.
  - **Statement** (RF-27) como paso local que ejecuta sentencias sin gRPC.
- El entorno (`EntornoMotor`) tiene los scopes `locals`/`parameters`/
  `file_globals` y el `resultado` del paso en curso. Lectura: `resultado.*`
  laxa (campo ausente → `Nulo`); los demás scopes estrictos (campo no declarado
  al cargar → error). Escritura: sólo `Locals` (regla "el paso sólo muta
  Locals" hecha valer en runtime).

## Reglas de estado

- Un paso saltado (por `disable` o precondición falsa) se registra con estado
  `"saltado"` y es **neutral** en el agregado `error > fallo > paso`: no cuenta
  como fallo ni error. Es una extensión aditiva del valor de `estado`, no del
  formato del reporte (RNF-08 se respeta; se documenta la extensión en
  `reportes.md`).
- Un error de expresión (precondición mal evaluada, `asigna` que falla,
  `statement` que falla) produce estado `"error"`, sin añadir campos a
  `ResultadoStep` ni a `paso.proto`. Es un fallo de **definición**, peor que un
  fallo de criterio: a diferencia del límite (que sólo empeora `paso`→`fallo`),
  un error de expresión sí puede convertir `paso` en `error`.

## Compatibilidad con ADR-0005

Una expresión declarada como dato no es conocimiento del dominio: el motor
manipula `Value`s y el `resultado` genérico, pero sigue sin saber si 4.2 es un
voltaje. El engine vive en `expr` (sin depender de `modelo`); el motor
implementa `Entorno`. ADR-0005 se respeta: el motor es genérico.

## Por qué el cableo al paso es post-MVP

En M4-núcleo las variables viven **en el motor**; el paso gRPC no las recibe
por el wire. `variables-y-alcances.md` ya prevía que el motor "inyecta los
valores relevantes en la petición (post-MVP, cuando el contrato lleve
parámetros tipados)". Eso requiere cambiar `paso.proto` (añadir parámetros
tipados) y es trabajo de un milestone posterior; aquí se aplaza. Mientras
tanto, `precondicion`/`asigna`/`statement` ya permiten cablear datos entre
pasos y meter lógica condicional sin volver a meter código en la secuencia.

## Alternativas descartadas

- **Pasar las variables al paso por `paso.proto`**: rompe el contrato y
  requiere parámetros tipados; aplazado (ver arriba).
- **Un lenguaje de scripting embebido (Lua/Python)**: las expresiones son
  datos acotados, no código arbitrario. Si hace falta lógica compleja, va en un
  paso (código real, ADR-0003). `motor-de-expresiones.md` lo deja fuera de
  scope.
- **Parsear las expresiones en runtime**: rompe el fail-fast; un error de
  sintaxis en una precondición alcanzable sólo en producción tardía es un
  fallo caro y evitable. ADR-0008 ya validaba los límites al cargar; M4 sigue el
  mismo patrón.
- **Truthiness implícita**: rechazada (igual que Julia). La precondición debe
  ser `Bool`; un no-bool es error de tipo. Evita bugs silenciosos en un
  lenguaje de datos de test.