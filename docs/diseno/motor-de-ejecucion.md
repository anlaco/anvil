# Diseño: Motor de ejecución

> **Prioridad:** MVP (ya implementado en `crates/motor/src/lib.rs`).
> Este doc **formaliza** la semántica existente; no la inventa.

El núcleo de Anvil: cómo se recorre una secuencia. Trazable a
`crates/motor/src/lib.rs::ejecuta_secuencia`.

## Modelo

Una secuencia es `DefinicionSecuencia{nombre, pasos_setup, pasos_main,
pasos_cleanup}` (`crates/modelo/src/lib.rs`); cada paso es
`DefinicionPaso{nombre, tipo, module, reintentos, …}`, donde `tipo` dice
**cómo se juzga** el paso y `module` **qué llama** (ADR-0040). El motor la
recorre en tres fases fijas.

> **Process model (M5, ADR-0016):** el motor **no sabe** que vive en un PM.
> Un PM es una `DefinicionSecuencia` envoltorio cuyo `main` lleva un
> `sequence_call` a la secuencia del usuario; el cargador reescribe el
> placeholder `secuencia_usuario` al path del usuario y el motor ve un
> `Programa` corriente. La inyección la resuelve el cargador, no el núcleo.

## Semántica de ejecución (spec, no cambia)

1. **Setup** — corren *todos* los pasos. Si alguno **mueve el veredicto**, se
   marca `setup_ok = false`. No corta en el primero: el Setup prepara recursos
   y conviene intentar todos (p. ej. abrir varios instrumentos).
2. **Main** — solo corre **si el Setup fue bien**. Corta **en el primer
   fallo** (`break` tras registrar el resultado fallido). El resto del Main
   se salta.
3. **Cleanup** — corre **siempre**, haya ido bien el Setup/Main o no.

> Principio rector: **un equipo que se quedó encendido es peor que una
> secuencia que falló.** De ahí que el Cleanup sea incondicional.

«Mueve el veredicto» es un estado **por encima del mínimo de la escala de
severidad** (`motor::mueve_el_veredicto`), no una lista de estados: `pass`,
`skipped` y `done` son neutrales y no cortan nada (ADR-0040 §6). Leerlo de la
escala en vez de enumerarlo es lo que evita que un estado neutral nuevo corte
el Setup por omisión.

## Reintentos por paso

Cada `DefinicionPaso.reintentos` es el número **total** de intentos (1 =
sin reintentos). El motor reintenta mientras el paso no pase y queden
intentos (`ejecuta_con_reintentos`):

```
max = reintentos.max(1)        // nunca 0 intentos
intento = 1
resultado = ejecuta_paso(nombre, 1)
while !resultado.paso() && intento < max:
    intento += 1
    resultado = ejecuta_paso(nombre, intento)
```

Lo que se reintenta es la **respuesta del módulo**, no el veredicto del paso:
el tipo y el límite se juzgan después, en `corre_un_paso`, una sola vez
(ADR-0040). Así un fallo de límite **no consume un reintento**
([#76](https://github.com/anlaco/anvil/issues/76)) — repetir la medida no
cambiaría el umbral.

El `intento` (desde 1) viaja al paso en `PeticionPaso.intento`. Un paso lo
usa para simular fallos transitorios (ver `demo/connect` en
`ejemplos/departamento/demo`: falla el 1, pasa el 2+).

> **Decisión:** un paso que falla consume reintentos; un paso que da
> `error` también se reintenta (el motor solo distingue `paso` del resto).
> Es deliberado: `error` de comunicación puede ser transitorio. Si un paso
> quiere ser *no-reintentable*, ese control será responsabilidad del paso
> (post-MVP, vía metadatos del paso).

## Agregado de estado

El resultado de la secuencia es `ResultadoSecuencia` con todos los
`ResultadoStep`. El estado agregado:

- `error` si algún paso dio `error`.
- si no, `fail` si alguno dio `fail`.
- si no, `pass`.

`skipped` y `done` quedan **fuera** de la escala: una secuencia entera de
pasos `done` agrega a `pass`. Y hay un agregado que ningún paso devuelve,
`inconclusive`, cuando la secuencia declara un `pass_fail` en `main` y ninguno
llegó a evaluarse (ADR-0019).

Un `error` manda sobre un `fail` aunque llegue antes (testeado en
`modelo/src/lib.rs`). Ver [limites-y-estados.md](limites-y-estados.md).

## Errores del motor vs. fallos del paso

- **Fallo del paso** (`estado = "fail"`): resultado válido, no corta la
  ejecución del motor (sí corta el Main).
- **Error del motor** (`Error::Red` / `Error::Protobuf`): la comunicación
  se rompió. La secuencia se interrumpe (`anvil` sale con código != 0). **No** se confunde con un paso que falla (RF-11).

## Control de flujo (MVP-parcial)

Estándar en todo ATE comercial. **Implementado en M4-núcleo**:

- **disable:** marcar un paso como saltado (no se invoca) sin borrarlo de la
  secuencia. Se registra con estado `"skipped"` (neutral en el agregado). Un
  paso deshabilitado sigue declarando su `executor`: apagarlo no deja de
  hacerlo un paso que llama a uno.
- **pause-on-fail:** detener la ejecución al primer fallo para inspección
  interactiva. En headless (M4-núcleo) **corta la fase en curso** al fallar —
  en Setup corta el bucle (que por defecto corre todos); en Main refuerza el
  corte en primer fallo (que ya corta); en **Cleanup no corta** (respeta el
  principio "un equipo encendido es peor que una secuencia que falló"). El
  modo interactivo "espera input" es **post-MVP** (WASI P2 no ofrece espera
  fiable).
- **step:** ejecutar paso a paso. **Pendiente (post-MVP)**: requiere un
  mecanismo de espera/pausa que WASI P2 no da de forma fiable. El cargador
  sigue rechazando el campo `step` (`deny_unknown_fields`); se dejará para
  cuando haya un modelo de espera o una UI.

Además, M4 añade la **precondición** por paso (RF-33): el motor evalúa una
expresión antes de invocar el paso; si es falsa, lo salta sin gastar intento.
Y el paso `statement` (RF-27), local (sin gRPC), que ejecuta sentencias del
lenguaje de expresiones contra el entorno. Ver
[motor-de-expresiones.md](motor-de-expresiones.md) y ADR-0009.

**M4b** añade el paso `sequence_call` (RF-27), también motor-side (sin gRPC):
invoca otra secuencia como un paso y anida su `ResultadoSecuencia` en el
`ResultadoStep` del call. El cargador resuelve la subsecuencia por nombre
(inline) o por path (archivo externo) al cargar; el motor no abre ficheros
(ADR-0005). Los `parametros` son by-reference: copia `locals.X` del padre →
`parameters.P` al iniciar y `parameters.P` (final) → `locals.X` al volver
(como TestStand). La subsecuencia se ejecuta con `es_raiz=false`: no dispara
`on_inicio/on_fin_secuencia` (sin doble render), pero sí los hooks de paso.
Profundidad máxima (64) como red de seguridad ante un ciclo que escapara al
cargador. Ver ADR-0010.

Atributos de `DefinicionPaso` (campos YAML):

```yaml
main:
  - name: medir_voltaje
    type: numeric_limit   # obligatorio: action | pass_fail | numeric_limit
                          #              | statement | sequence_call
    module: dmm/measure_voltage   # qué llama; con él, `executor`
    executor: bench
    retries: 1
    disable: false        # si true, se salta (estado "skipped")
    pause_on_fail: false  # si true y falla, detiene la fase
    precondition: 'locals.contador > 0'  # si falsa, se salta sin intento
    limit: { comparison: GELE, low: 4.5, high: 5.5, units: V }  # numeric_limit
    value: '${result.outputs.pico}'  # numeric_limit: qué número juzgar;
                                     # por defecto, la medida del módulo
    condition: 'locals.v > 4.9'  # sólo pass_fail
    statement: 'locals.x = 1'    # sólo statement
    sequence: init               # sólo sequence_call (nombre o path)
    inputs: { p: locals.x } # by-value con `module`; by-reference en sequence_call
    assign:               # vuelca result.* a Locals; corre ANTES del juicio
      voltaje: '${result.measured_value}'
```

## Determinismo

La ejecución es **secuencial y sin concurrencia implícita** en el MVP: para
la misma secuencia y los mismos pasos, el orden y el número de intentos son
reproducibles (RNF-03). El paralelismo es post-MVP y exigirá cancelación
jerárquica para no romper el Cleanup garantizado
([proceso-de-test.md](proceso-de-test.md)).

## No incluye (post-MVP / out-of-scope)

- Paralelismo y modelos Parallel/Batch.
- Substeps (Pre/Run/Post) — ligado a custom step types.
- Debugger visual.