# Diseño: Integración de instrumentos

> **Prioridad:** MVP-parcial. El adapter gRPC ya existe; SCPI/PyVISA
> nativo es post-MVP.

Cómo Anvil habla con hardware real. Trazable a [ADR-0003](../adr/0003-pasos-por-grpc-por-nombre.md)
y [ADR-0006](../adr/0006-wasi-grpc-propio.md).

## El adapter es gRPC

Un instrumento se accede a través de un **paso** que lo maneja. El paso se
invoca por gRPC por nombre (ADR-0003): el motor no sabe si el paso habla
SCPI, VISA, un REST privado o nada. El instrumento vive **detrás del paso**,
opaco al motor.

**Dónde corre el paso (ADR-0012):** en el ejecutor WASM embebido (built-in o
`.wasm` cargado por path), o en un **ejecutor de lenguaje** distribuido
(`executores/`, p. ej. Python) — que puede desplegarse en un **LID** (SO
legacy con aislamiento declarado) cuando las DLLs/drivers del fabricante lo
exijan. Anvil solo ve endpoints gRPC; ver
[executores-lenguaje.md](executores-lenguaje.md).

Hoy (`pasos_demo`) los pasos son simulados (no tocan hardware). La
frontera gRPC motor↔paso es real; la integración con el instrumento es
interna del paso.

## Estrategia por capas (propuesta)

```
Motor ──gRPC──▶ Paso (Rust/Python/…)
                  └─▶ lib de instrumento (SCPI/VISA/REST)
                        └─▶ Instrumento físico
```

1. **MVP-parcial — paso gRPC:** un paso Rust que envía comandos SCPI por
   TCP/serial y parsea la respuesta. El paso traduce `medir_voltaje` →
   comandos del instrumento concreto.
2. **post-MVP — `wasi-visa` (Apache):** una lib que abstrae VISA/SCPI al
   estilo PyVISA, linkable desde un paso. Permite escribir pasos de
   instrumento sin re implementar el transporte.
3. **post-MVP — perfiles YAML de instrumento:** un perfil por modelo (p. ej.
   un Keithley 2400) con sus comandos y un `SimBackend` determinista +
   record/replay (copiar de pytestlab, investigación §6) → **CI sin
   hardware**.

## Sim y record/replay (post-MVP, valor alto)

El dolor de TestStand: dependencias que van bien en Dev y se rompen en
runtime por el "Inplaceness Algorithm" (investigación §2). La respuesta de
Anvil: un **backend simulado** determinista y **record/replay** estricto
(`ReplayMismatchError` si la respuesta real difiere de la grabada). Permite
probar una secuencia en CI sin el instrumento, y detectar regresiones de
comunicación.

## Aislamiento y seguridad

Un paso que toca hardware es **riesgo físico** (RNF-06): un comando mal
enviado puede dañar equipo o ser peligroso. Mitigaciones:

- El paso es el **único** que conoce el instrumento; el motor nunca envía
  comandos directos.
- La semántica de **Cleanup siempre** garantiza que el paso de
  `desconectar_equipo` corre aun si el Main falla (un equipo encendido y
  olvidado es el peor caso).
- Recomendación: los pasos peligrosos deben tener **guardas internas**
  (limitar corrientes, confirmar estados) — ver [SECURITY.md](../../SECURITY.md).

## Out-of-scope

- Drivers NI / IVI nativos.
- Descubrimiento automático de instrumentos (plug-and-play) en el MVP.