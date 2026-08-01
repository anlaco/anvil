# ADR-0004: Licencia dual AGPL producto / Apache librerías

- **Estado:** Aceptada (decisión pre-existente, formalizada aquí)
- **Fecha:** pre-prototipo

## Contexto

Anvil quiere dos cosas en tensión:

1. **Proteger el producto** frente a que alguien lo cierre y lo revenda
   (el problema de un secuenciador puramente permisivo: una marca puede
   empaquetarlo, cerrarlo y competir contra el proyecto original).
2. **No contagiar** a quien integra las librerías: un paso de test se
   *linka* con las libs de Anvil; si esas libs fueran copyleft, el código
   del paso se contagiaría y las empresas huirían.

Además, las empresas suelen **prohibir AGPL** en sus dependencias, así que
la elección de licencia afecta directamente a la adopción empresarial
([investigación](../investigacion/TestStand-y-competencia.md) §4 punto 7).

## Decisión

Estrategia **dual**, declarada en `Cargo.toml` (`license =
"AGPL-3.0-or-later"`) y el `README.md`:

| Pieza | Licencia | Por qué |
|---|---|---|
| **anvil** (el producto) | **AGPL-3.0-or-later** | Se *usa*, no se linka. La AGPL impide cerrarlo y revenderlo. |
| Interfaces WIT, `wasi-grpc`, `wasi-visa` | **Apache-2.0** | Se linkan en código ajeno; queremos que se adopten como referencia. |

**Las secuencias de test no son obra derivada** de Anvil: son *datos* que se
le pasan al secuenciador (ADR-0002). Los límites de aceptación y el know-how
de producto que hay en una secuencia son del usuario y siguen siéndolo.

## Consecuencias

**Positivas:**

- El producto queda protegido (no pueden cerrarlo y revenderlo).
- Quien escribe un paso no recibe contagio: linka libs Apache, su código es
  suyo.
- Las secuencias industriales del usuario no se contagian → una empresa
  puede usar Anvil sin exponer su secreto de test.
- Diferenciación: nadie en el landscape usa esta dual (OpenTAP=MPL,
  Semi-ATE=GPL fuerte, resto=Apache sin protección).

**Negativas:**

- Una empresa que quiera *modificar Anvil y ofrecerlo como servicio* está
  obligada por AGPL a publicar sus cambios. Es intencional.
- AGPL asusta a legal corporativo aunque no les aplique (ellos *usan* el
  secuenciador, no lo linkan) → requiere [licencia.md](../licencia.md) claro
  respondiendo a las preguntas frecuentes.

**Neutras:**

- Flojoy ya es AGPL-3.0 → la licencia **no** es la diferenciación de Anvil
  (ver [vision.md](../vision.md)); la tesis es gRPC multilenguaje + WASM
  aislado.

## Alternativas descartadas

- **Todo Apache permisivo:** no protege el producto (lo cierran y revenden).
- **Todo GPL/AGPL:** contagia a quien linka las libs → mata la adopción de
  pasos en terceros.
- **MPL (estilo OpenTAP):** file-level copyleft; no protege el producto
  integrado tan fuerte como AGPL.

## Enlaces

- [licencia.md](../licencia.md), [ADR-0002](0002-secuencia-como-datos.md),
  [vision.md](../vision.md).