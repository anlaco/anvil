# Diseño: Motor de ejecución

> **Prioridad:** MVP (ya implementado en `crates/motor/src/lib.rs`).
> Este doc **formaliza** la semántica existente; no la inventa.

El núcleo de Anvil: cómo se recorre una secuencia. Trazable a
`crates/motor/src/lib.rs::ejecuta_secuencia`.

## Modelo

Una secuencia es `DefinicionSecuencia{nombre, pasos_setup, pasos_main,
pasos_cleanup}` (`crates/modelo/src/lib.rs`); cada paso es
`DefinicionPaso{nombre, reintentos}`. El motor la recorre en tres fases
fijas.

> **Process model (M5, ADR-0016):** el motor **no sabe** que vive en un PM.
> Un PM es una `DefinicionSecuencia` envoltorio cuyo `main` lleva un
> `sequence_call` a la secuencia del usuario; el cargador reescribe el
> placeholder `secuencia_usuario` al path del usuario y el motor ve un
> `Programa` corriente. La inyección la resuelve el cargador, no el núcleo.

## Semántica de ejecución (spec, no cambia)

1. **Setup** — corren *todos* los pasos. Si alguno **no pasa**, se marca
   `setup_ok = false`. No corta en el primero: el Setup prepara recursos y
   conviene intentar todos (p. ej. abrir varios instrumentos).
2. **Main** — solo corre **si el Setup fue bien**. Corta **en el primer
   fallo** (`break` tras registrar el resultado fallido). El resto del Main
   se salta.
3. **Cleanup** — corre **siempre**, haya ido bien el Setup/Main o no.

> Principio rector: **un equipo que se quedó encendido es peor que una
> secuencia que falló.** De ahí que el Cleanup sea incondicional.

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

El `intento` (desde 1) viaja al paso en `PeticionPaso.intento`. Un paso lo
usa para simular fallos transitorios (ver `pasos_demo::conectar`: falla el
1, pasa el 2+).

> **Decisión:** un paso que falla consume reintentos; un paso que da
> `error` también se reintenta (el motor solo distingue `paso` del resto).
> Es deliberado: `error` de comunicación puede ser transitorio. Si un paso
> quiere ser *no-reintentable*, ese control será responsabilidad del paso
> (post-MVP, vía metadatos del paso).

## Agregado de estado

El resultado de la secuencia es `ResultadoSecuencia` con todos los
`ResultadoStep`. El estado agregado:

- `error` si algún paso dio `error`.
- si no, `fallo` si alguno dio `fallo`.
- si no, `paso`.

Un `error` manda sobre un `fallo` aunque llegue antes (testeado en
`modelo/src/lib.rs`). Ver [limites-y-estados.md](limites-y-estados.md).

## Errores del motor vs. fallos del paso

- **Fallo del paso** (`estado = "fallo"`): resultado válido, no corta la
  ejecución del motor (sí corta el Main).
- **Error del motor** (`Error::Red` / `Error::Protobuf`): la comunicación
  se rompió. La secuencia se interrumpe (`basica_datos.rs` sale con código
  != 0). **No** se confunde con un paso que falla (RF-11).

## Control de flujo (MVP-parcial)

Estándar en todo ATE comercial. **Implementado en M4-núcleo**:

- **disable:** marcar un paso como saltado (no se invoca) sin borrarlo de la
  secuencia. Se registra con estado `"saltado"` (neutral en el agregado).
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
pasos_main:
  - nombre: medir_voltaje
    reintentos: 1
    disable: false        # si true, se salta (estado "saltado")
    pause_on_fail: false  # si true y falla, detiene la fase
    precondicion: 'locals.contador > 0'  # si falsa, se salta sin intento
    tipo: grpc            # o "statement" (paso local, sin gRPC)
                          # o "sequence_call" (invoca subsecuencia, M4b)
    statement: 'locals.x = 1'   # sólo si tipo: statement
    secuencia: init           # sólo si tipo: sequence_call (nombre o path)
    parametros: { p: locals.x } # sólo si sequence_call (by-reference)
    asigna:               # si tipo: grpc o sequence_call; vuelca resultado.* a Locals
      voltaje: '${resultado.valor_medido}'
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