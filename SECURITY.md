# Seguridad

## Riesgo especial: Anvil opera hardware real

A diferencia de la mayoría del software, **Anvil controla instrumentos
físicos** (fuentes, multímetros, relés, fixtures). Un defecto en un paso
puede:

- **dañar equipo** (sobrecorriente, sobretensión, movimientos de fixture
  fuera de rango),
- ser un **riesgo de seguridad** para operadores (calor, piezas móviles,
  alta tensión),
- dejar un equipo en un **estado peligroso** (encendido, energizado) tras
  una secuencia fallida.

Por eso Anvil impone que el **Cleanup corre siempre** (ver
[`docs/diseno/motor-de-ejecucion.md`](docs/diseno/motor-de-ejecucion.md)) y
que el motor nunca envía comandos directos al hardware: todo acceso a
instrumentos vive **detrás de un paso** (ver
[`docs/diseno/integracion-instrumentos.md`](docs/diseno/integracion-instrumentos.md)).

## Responsabilidad del autor de pasos

Un paso que toca hardware es el **único** responsable de la seguridad de
ese hardware:

- Aplicar **guardas internas** (limitar corrientes/tensiones, confirmar
  estados antes de actuar).
- Diseñar el **Cleanup** del paso para dejar el equipo en un estado seguro.
- No asumir que la secuencia se completará: el Cleanup puede correr tras
  un fallo a mitad.

Anvil no verifica la seguridad de un paso: el motor es genérico y no conoce
el dominio ([ADR-0005](docs/adr/0005-motor-generico-dirigido-por-datos.md)).
La responsabilidad es del integrador.

## Versiones soportadas

Anvil está en **0.1.0 (pre-release)**. No hay versión estable todavía: solo
se aplican arreglos de seguridad a la rama `main` más reciente. Cuando haya
releases, esta sección listará las versiones soportadas.

| Versión | Soporte |
|---|---|
| `main` (0.1.0-dev) | ✅ |
| (sin releases estables todavía) | — |

## Reportar una vulnerabilidad

**No abras un issue público** para vulnerabilidades de seguridad.

Reporta en privado a **[AÑADIR EMAIL DE CONTACTO PARA SEGURIDAD]**. Incluye:

- Descripción del problema y su impacto (incluye riesgo **físico** si lo
  hay).
- Pasos para reproducirlo.
- Versión/commit afectado.

## Proceso

1. **Acuse de recibo** en 48 h.
2. **Evaluación** y plan de arreglo comunicado al reportero.
3. Una vez publicado el arreglo, **crédito** al reportero si lo desea.
4. Coordinación de divulgación si el problema afecta a integradores en
  producción.

> Mientras existan huecos en este `SECURITY.md` (`[AÑADIR ...]`), el contacto
> de seguridad se acuerda por el canal que el BDFL publique en el momento
> (ver [`GOVERNANCE.md`](GOVERNANCE.md)).