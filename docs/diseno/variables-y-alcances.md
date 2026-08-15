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
nombre: basica
locals:
  voltaje_leido: 0.0
parameters: {}            # al llamar desde otra secuencia
file_globals:
  lote: "A-2026-08"

setup:
  - nombre: conectar_equipo
    reintentos: 3
main:
  - nombre: medir_voltaje
    reintentos: 1
    asigna: { voltaje_leido: "${resultado.valor_medido}" }
```

## Reglas de acceso

- **Lectura:** un paso puede leer variables de su scope y de los superiores
  (Locals < Parameters < FileGlobals < StationGlobals).
- **Escritura:** un paso escribe su resultado en la variable indicada
  (`asigna`), y muta solo **Locals** de su secuencia (no FileGlobals ni
  StationGlobals — eso lo hace el motor, no el paso, para mantener el paso
  aislado por contrato). Ver el recorte de `Parameters` en sequence call, más
  abajo.
- **Tipado:** declaración con tipo básico (numérico, texto, booleano); la
  validación es al cargar (fail-fast). Sin el árbol de propiedades tipado
  recursivo de TestStand en el MVP.
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
    - nombre: medir_voltaje
      asigna: { v: '${resultado.valor_medido}' }   # aquí sí
    - nombre: verificar
      precondicion: 'locals.v > 4.5'               # y aquí se lee el local
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
    - nombre: medir            # si este paso da `error`…
      asigna: { valor: '${resultado.valor_medido}' }
  cleanup:
    - nombre: comprobar        # …aquí `locals.valor` sigue siendo 99.0
      tipo: pass_fail
      condicion: 'locals.valor == 99.0'
  ```

- **`asigna` sólo tiene sentido en un paso `grpc` o `sequence_call`.** En un
  `statement` o un `pass_fail` es **error de carga**: ninguno de los dos produce
  `resultado.*` que volcar, así que la `asigna` sería un no-op silencioso. Un
  `statement` asigna dentro de su propia sentencia (`locals.x = …`).

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