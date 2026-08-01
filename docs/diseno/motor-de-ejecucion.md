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

## Control de flujo (MVP-parcial, pendiente)

Estándar en todo ATE comercial; **no implementado aún**:

- **pause-on-fail:** detener la ejecución al primer fallo para inspección
  interactiva (en headless, espera input).
- **step:** ejecutar paso a paso.
- **disable:** marcar un paso como saltado (no se invoca) sin borrarlo de la
  secuencia.

Estos son atributos de `DefinicionPaso` (o del paso en YAML) que el motor
respeta. Propuesta de campos:

```yaml
pasos_main:
  - nombre: medir_voltaje
    reintentos: 1
    disable: false        # si true, se salta
    pause_on_fail: false  # si true, detiene al fallar
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