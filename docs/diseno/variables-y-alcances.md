# Diseño: Variables y alcances

> **Prioridad:** MVP-parcial. **Locals / Parameters / FileGlobals
> implementados en M4-núcleo** (motor-side); **Parameters de entrada/salida
> by-reference** en M4b (sequence call); StationGlobals post-MVP. El cableo
> de variables al paso por el wire (`paso.proto`) es post-MVP: en MVP las
> variables viven en el motor y `asigna` vuelca `resultado.*` a `Locals`
> (ADR-0009). ADR-0010 cubre el sequence call.

Modelo mental de TestStand: "una hoja de cálculo para tests" — variables
tipadas con alcance, para cablear datos entre pasos sin código pegamento
([investigación](../investigacion/TestStand-y-competencia.md) §1.4). Anvil
adopta la jerarquía **sin** replicar el árbol de propiedades opaco de
TestStand.

## Scopes (propuesta)

| Scope | Visible en | Cuándo se crea | MVP |
|---|---|---|---|
| **Locals** | Una secuencia | Al iniciar su ejecución | MVP-parcial |
| **Parameters** | Secuencia llamada (entrada/salida by-reference) | Al invocar via sequence call (M4b) | MVP-parcial |
| **FileGlobals** | Todas las secuencias de un archivo | Al cargar el archivo | MVP-parcial |
| **StationGlobals** | Todas las secuencias de la estación | Persistente en la estación | post-MVP |
| **resultado** | **Sólo el `asigna` del propio paso** | Al volver el paso, con lo que devolvió | MVP-parcial |

`resultado` no es un scope de variables como los otros cuatro: es la ventana
—brevísima— por la que se lee lo que el paso acaba de devolver. Ver la regla
de alcance más abajo.

## En el formato de secuencia (YAML)

```yaml
name: basica
locals:
  voltaje_leido: 0.0
  # The one variable with no initial value: a reference (ADR-0022). It has no
  # literal form, so all the sequence can state is which executor its handle
  # will come from.
  rack: { type: reference, executor: bench }
parameters: {}            # al llamar desde otra secuencia
file_globals:
  lote: "A-2026-08"

setup:
  - name: conectar_equipo
    retries: 3
main:
  - name: medir_voltaje
    retries: 1
    assign: { voltaje_leido: "${result.measured_value}" }
```

## Reglas de acceso

- **Lectura:** un paso puede leer variables de su scope y de los superiores
  (Locals < Parameters < FileGlobals < StationGlobals).
- **Escritura:** un paso escribe su resultado en la variable indicada
  (`asigna`), y muta solo **Locals** de su secuencia (no FileGlobals ni
  StationGlobals — eso lo hace el motor, no el paso, para mantener el paso
  aislado por contrato). Ver el recorte de `Parameters` en sequence call, más
  abajo.
- **Tipado:** declaración con tipo básico (numérico, texto, booleano,
  **referencia**); la validación es al cargar (fail-fast). Sin el árbol de
  propiedades tipado recursivo de TestStand en el MVP.
- **Destinos declarados:** el destino de `asigna` y los lvalues de
  `statement` (`locals.X`/`parameters.P`) deben estar declarados en su
  `locals:`/`parameters:` — el cargador lo rechaza si no (DEF-3 del informe
  de beta). Sin esto, un destino mal escrito o el nombre de un `parameter`
  crea una `Local` nueva en silencio en vez de fallar: el resto de la
  secuencia sigue leyendo la variable original, sin tocar, y el veredicto es
  el que no se pidió. Ver [informe-beta-2026-08.md](../qa/informe-beta-2026-08.md#def-3).
- **`resultado.*` sólo es visible dentro del `asigna` del propio paso.** No
  está disponible en `precondicion`, ni en la `condicion` de un `pass_fail`,
  ni en un `statement`. La razón es de secuencia temporal: `resultado` es lo
  que el paso **acaba de devolver**, y una precondición se evalúa *antes* de
  invocarlo — no hay nada que leer todavía. El motor lo liga justo antes del
  `asigna` y lo suelta justo después.

  Si necesitas una medida más allá de ese punto, vuélcala a un local y léela
  desde ahí:

  ```yaml
  main:
    - name: medir_voltaje
      assign: { v: '${result.measured_value}' }   # aquí sí
    - name: verificar
      precondition: 'locals.v > 4.5'               # y aquí se lee el local
  ```

  Usarlo fuera de `asigna` es **error de carga** desde #12. Antes no fallaba:
  valía `nothing`, así que `precondicion: 'resultado.valor_medido != nothing'`
  era un `false` constante, el paso se saltaba, y como `saltado` no degrada el
  agregado la secuencia **terminaba en verde**. En la primera campaña de beta
  ese patrón se propagó a 19 secuencias y 51 precondiciones. Ver
  [§5 del informe](../qa/informe-beta-2026-08.md#leccion).

- **Los campos de `resultado` son tres y cerrados**: `estado`, `mensaje` y
  `valor_medido`. Cualquier otro nombre es **error de carga** —lo ve
  `--validate`, sin ejecutar nada—, porque un `resultado.valor_meddio` no es un
  dato ausente: es un typo (ADR-0019, regla de detección, issue #27). Antes
  valía `nothing`, ese `nothing` se volcaba encima de la variable que decidía el
  veredicto, y la secuencia salía **en verde con la variable destruida**.

  Lo laxo sigue siendo el **valor**, no el nombre: `resultado.valor_medido` vale
  `nothing` si el paso no midió, que es legítimo. Lo que ya no vale es
  preguntar por un campo que no existe.

- **`asigna` no escribe si el paso dio `error`.** Sin resultado no hay nada que
  volcar, así que el destino **no se toca** y conserva su valor (ADR-0019,
  Regla 2). Que corriera era defendible; que machacara en silencio con un
  `nothing` la variable con valor bueno que el `cleanup` iba a leer para decidir
  si apagaba una fuente, no.

  Con `fallo` sí corre: ahí hay medida —la que incumplió el criterio—, y es
  justo la que interesa volcar. La distinción es la de la Regla 2 entera:
  `fallo` es del DUT, `error` es de Anvil.

  ```yaml
  locals:
    valor: 99.0
  main:
    - name: medir            # si este paso da `error`…
      assign: { valor: '${result.measured_value}' }
  cleanup:
    - name: comprobar        # …aquí `locals.valor` sigue siendo 99.0
      type: pass_fail
      condition: 'locals.valor == 99.0'
  ```

- **`asigna` sólo tiene sentido en un paso `grpc` o `sequence_call`.** En un
  `statement` o un `pass_fail` es **error de carga**: ninguno de los dos produce
  `resultado.*` que volcar, así que la `asigna` sería un no-op silencioso. Un
  `statement` asigna dentro de su propia sentencia (`locals.x = …`).

## Reference variables (ADR-0022)

A reference is a handle to an object an executor keeps for itself — a bench
session, an instrument connection: a thing with open sockets that cannot cross
the wire. It is declared, and only in `locals:`:

```yaml
locals:
  rack: { type: reference, executor: bench }
```

**Why the executor is part of the declaration.** It is the only thing that makes
"this handle is being passed to a step of another executor" decidable without
following it back through `assign`, subsequence `args` and the process model —
the data-flow analysis ADR-0021 declined to do. `inputs: { rack: '${locals.rack}' }`
is an expression, and the type of an expression is not guessed. Declared on the
variable, the check is one lookup and it can be seen by reading the file: it
runs with nothing connected, in plain `--validate`.

**It has no initial value**, and that is the point: a reference cannot be
written by hand, so until a step mints one there is nothing there. Reading it
before then and handing it to a step is refused where every other absent
parameter is.

**What can be written into it, and by whom.** Only the `assign` of a step served
by that executor, and only from `result.outputs.<name>`. A `statement` cannot:
a statement computes, and a reference is not computed. `result.measured_value`
cannot either: it is a number, and letting one land in a variable the file calls
a handle would make the type a label rather than a fact.

**Only in `locals:`**, and each refusal for its own reason. `file_globals:` are
the file's constants and the engine refuses to write them at all, so a handle
declared there could never be filled in. `parameters:` is the by-reference
channel of a `sequence_call`, and handing a rack to a subsequence is a decision
ADR-0022 does not take — allowing it here would be taking it by accident.

**What can be done with one**, and it is deliberately almost nothing: read it
out of a variable, and hand it to a step. Arithmetic, comparison, use as a
limit or as a verdict are all errors, and that refusal is the whole reason the
type exists — the mechanism already worked while a handle was just text.

## Por qué este recorte

El motor es genérico (ADR-0005): no conoce el dominio. Las variables son
**datos** en la secuencia que el paso recibe/produce vía el contrato. El
paso no lee variables "del motor" directamente: el motor **inyecta** los
valores relevantes en la petición (post-MVP, cuando el contrato lleve
parámetros tipados — ver [contrato-grpc.md](../contrato-grpc.md)) y recoge
el resultado. Así se preserva el aislamiento.

## Parameters entrada/salida by-reference (M4b)

Desde M4b, un **sequence call** cablea `parameters` de entrada **y** de
salida con la secuencia llamadora, como TestStand by-reference (default):

- El call mapea cada `Parameter` de la subsecuencia a un `locals.X` del
  padre: `parametros: { P: locals.X }`.
- **Entrada:** el motor copia `locals.X` → `parameters.P` al iniciar la
  subsecuencia.
- La subsecuencia **escribe en `parameters.P`** con `statement` (`asigna`
  escribe siempre en `locals`, aunque el nombre coincida con un `parameter`
  declarado — el cargador lo rechaza, ver arriba).
- **Salida:** al volver, el motor copia `parameters.P` (final) → `locals.X`
  (el mismo lvalue de la entrada). Un mismo `Parameter` es entrada y salida.

Esto relaja de forma **acotada** la regla "sólo se muta Locals" (ADR-0009):
la subsecuencia puede escribir en sus `parameters` (su contrato de retorno);
la **raíz** no (no tiene a quién devolver). El paso gRPC sigue **sin tocar**
variables del motor — el aislamiento del paso se mantiene. Ver ADR-0010.

Recortes MVP-parcial: los argumentos son sólo `locals.X` (by-reference). El
modo **by-value** (entrada sin retorno, para aislar) y el by-reference
transitivo (pasar `parameters.X`/`file_globals.X` del padre) quedan
post-MVP. Para pasar un valor calculado al call, se calcula antes en un
Local (con un `statement`) y se pasa ese Local por referencia.

## StationGlobals (post-MVP)

Persistencia por estación (configuración de la línea, calibración). Requiere
un almacén local y un modelo de concurrencia (escritura segura entre
secuencias paralelas). Por eso es post-MVP.

## Out-of-scope

- Árbol de propiedades recursivo tipado de TestStand (complejo, opaco).
- Referencias cruzadas con expresiones complejas en el MVP.