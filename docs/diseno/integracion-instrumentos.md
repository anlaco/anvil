# Diseño: Integración de instrumentos

> **Prioridad:** MVP-parcial. **Implementado en M5**: adapter gRPC pulido
> con `pasos_scpi` (paso real SCPI/TCP + mock determinista, ver
> [ADR-0017](../adr/0017-adapter-grpc-de-instrumento-real-por-scpi-tcp.md)).
> PyVISA/`wasi-visa` nativo es post-MVP.

Cómo Anvil habla con hardware real. Trazable a [ADR-0003](../adr/0003-pasos-por-grpc-por-nombre.md)
y [ADR-0006](../adr/0006-wasi-grpc-propio.md).

## El adapter es gRPC

Un instrumento se accede a través de un **paso** que lo maneja. El paso se
invoca por gRPC por nombre (ADR-0003): el motor no sabe si el paso habla
SCPI, VISA, un REST privado o nada. El instrumento vive **detrás del paso**,
opaco al motor.

**Dónde corre el paso (ADR-0013):** en el ejecutor WASM embebido (built-in,
de serie, en loopback), o en un **ejecutor de lenguaje** distribuido
(`executores/`, p. ej. Python) que puede correr en loopback (M5-ext.1,
routing `ejecutores:`/`ejecutor:`) o, en el futuro, en un **LID** (SO legacy
con aislamiento declarado, aplazado a post-M5-ext) cuando las
DLLs/drivers del fabricante lo exijan. Anvil solo ve endpoints gRPC; ver
[executores-lenguaje.md](executores-lenguaje.md).

Hoy (`pasos_demo`) los pasos son simulados (no tocan hardware). La
frontera gRPC motor↔paso es real (M5-ext.1 la generaliza a varios
endpoints); la integración con el instrumento es interna del paso.

## Estrategia por capas (propuesta)

```
Motor ──gRPC──▶ Paso (Rust/Python/…)
                  └─▶ lib de instrumento (SCPI/VISA/REST)
                        └─▶ Instrumento físico
```

1. **MVP-parcial — paso gRPC (implementado en M5):** un paso Rust que envía
   comandos SCPI por TCP y parsea la respuesta. Vive en `crates/pasos_scpi`
   (`medir_voltaje_scpi`), se despacha por nombre y se testa contra un
   servidor TCP mock en loopback. La dirección va en `ANVIL_SCPI_ADDR`
   (default `127.0.0.1:5025`, loopback en el sandbox del host, ADR-0011).
   El paso traduce `medir_voltaje` → `MEASURE:VOLTAGE?` del instrumento
   concreto. El ejecutor compone adaptadores: prueba `pasos_scpi` primero,
   `pasos_demo` después.
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