# Licencia

Manifiesto de la estrategia de licencia de Anvil. Decisión de fondo:
[ADR-0004](adr/0004-licencia-dual-agpl-apache.md). Texto base: sección
Licencia del `README.md` y [`LICENSE`](../LICENSE).

## La estrategia dual

| Pieza | Licencia | Por qué |
|---|---|---|
| **anvil** (motor, ejecutor, modelo, pasos_demo) | **AGPL-3.0-or-later** | Es el producto: se *usa*, no se linka. La AGPL impide que alguien lo cierre y lo revenda. |
| `wasi-grpc`, `wasi-visa` | **Apache-2.0** | Se linkan en código ajeno (los pasos). Queremos que se adopten como referencia. |
| `executores/` (ejecutores de lenguaje) | **Apache-2.0** | Su SDK entra dentro del código de quien escribe un paso: `from anvil_step import step`. Es el mismo caso que `wasi-grpc`. Licencia propia en [`executores/LICENSE`](../executores/LICENSE), porque el `LICENSE` de la raíz es AGPL y sin una explícita ahí mandaría ése. |
| Interfaces WIT | **Apache-2.0** | Igual: referencia adoptable. |

El boundary es claro: **lo que se *usa* es AGPL; lo que se *linka* es Apache.**

## Preguntas frecuentes (las que importan para adoptar)

### ¿Puede una empresa usar Anvil?

**Sí.** Usar Anvil —correrlo para probar tus productos en tu planta— **no
activa ninguna obligación AGPL**. La AGPL obliga a publicar fuente solo
cuando **modificas** Anvil y **lo ofreces a terceros como servicio** (SaaS)
sobre una red. Una empresa que:

- corre Anvil tal cual, o
- corre Anvil con sus propias secuencias de test, o
- parchea Anvil para uso interno (sin ofrecerlo a terceros),

**no tiene que publicar nada.** Su secreto industrial —las secuencias, los
límites, el know-how de producto— es suyo y sigue siéndolo.

### ¿Las secuencias son obra derivada de Anvil?

**No.** Las secuencias de test son **datos** que le pasas al secuenciador,
no un trabajo derivado del programa ([ADR-0002](adr/0002-secuencia-como-datos.md)).
Análogamente, un documento `.docx` no es obra derivada de Word. Los límites
de aceptación y la lógica de test que hay en una secuencia son tuyos.

Esto es **decisivo** para adopción empresarial: una empresa puede usar Anvil
sin exponer su proceso de test.

### Un paso (plugin) con licencia X, ¿es compatible?

El paso se invoca **por gRPC**, no se linka en Anvil (ADR-0003). Así que la
licencia del paso es **asunto del autor del paso**, no de Anvil: Anvil no
combina su código con el del paso en ningún binario.

- El paso puede ser **cualquier licencia** (MIT, BSD, Apache, GPL, propietaria):
  al ser un proceso separado tras un contrato, Anvil no se contamina.
- Si el paso **linka** `wasi-grpc`/`wasi-visa` (Apache-2.0), esa combinación
  es compatible: Apache es compatible como dependencia con prácticamente
  cualquier licencia (incluida GPL, por la excepción de patente y la
  compatibilidad ascendente). El paso resultante queda bajo la licencia que
  su autor elija.
- **Contribuir** un paso al repo de Anvil sí exigiría que sea compatible con
  AGPL (pues entraría en el producto). Eso se decide caso a caso.

Resumen: **escribir y correr un paso propio no te obliga a nada con la
AGPL de Anvil.**

### ¿Y si quiero modificar Anvil y ofrecerlo como servicio?

Aquí la AGPL **sí** aplica: si modificas Anvil y lo ofreces a terceros como
servicio en red, debes publicar el fuente modificado (incluido por la red).
Es **intencional**: protege el producto de que lo cierren y revendan. Si tu
caso es ofrecer Anvil modificado comercialmente y no quieres publicar, se
necesitará un **acuerdo de licencia comercial** (dual licensing). Hoy no hay
uno; se valorará cuando llegue el caso.

## Contribuir (CLA/DCO)

- Las contribuciones a Anvil (el producto AGPL) requieren **firmar** cada
  commit (DCO — *Developer Certificate of Origin*) para confirmar autoría y
  derecho a licenciar bajo AGPL-3.0-or-later.
- No se exige CLA pesada en esta fase; el DCO basta. Ver
  [`CONTRIBUTING.md`](../CONTRIBUTING.md).
- Loscopyright del producto son de sus mantenedores (ver
  [`GOVERNANCE.md`](../GOVERNANCE.md)).

## Por qué no otra licencia

- **Todo Apache permisivo:** no protege el producto; una marca lo cierra,
  empaqueta y compite contra el proyecto. Descartado.
- **Todo GPL/AGPL:** contagia a quien linka las libs (`wasi-grpc`/`wasi-visa`),
  matando la adopción de pasos en terceros. Descartado.
- **MPL-2.0 (estilo OpenTAP):** copyleft a nivel de archivo; más débil que
  AGPL para proteger el producto integrado. Descartado.

La dual AGPL/Apache es **la diferenciación de licencia** del landscape
([investigación](investigacion/TestStand-y-competencia.md) §4 punto 7):
nadie la usa (OpenTAP=MPL, Semi-ATE=GPL fuerte, resto=Apache sin
protección).

> **Nota de posicionamiento:** Flojoy ya es AGPL-3.0 → la licencia **no**
> es lo que diferencia a Anvil. La tesis es gRPC multilenguaje + WASM
> aislado (ver [vision.md](vision.md)).