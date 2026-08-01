# ADR-0005: Motor genérico dirigido por datos

- **Estado:** Aceptada (decisión pre-existente, formalizada aquí)
- **Fecha:** pre-prototipo

## Contexto

Si el motor sabe qué hace cada paso (qué instrumento toca, qué medida
compara, qué límite aplica), entonces el motor es específico del dominio:
cada tipo de paso nuevo exige tocar el secuenciador. TestStand padece esto:
tocar el process model o los step types rompe secuencias existentes
([investigación](../investigacion/TestStand-y-competencia.md) §2,
process model frágil).

Anvil ya decidió que la secuencia es datos (ADR-0002) y que los pasos se
invocan por gRPC por nombre (ADR-0003). Falta cerrar la consecuencia
lógica: el motor **no conoce el dominio**.

## Decisión

El **motor es genérico y dirigido por datos**: recorre una
`DefinicionSecuencia`, pide cada paso por nombre al ejecutor (gRPC), aplica
la semántica de ejecución (Setup/Main/Cleanup, reintentos, agregado de
estados) y **no sabe qué hace cada paso**. Lo único que interpreta del
resultado es el `estado` (`paso`/`fallo`/`error`) y, para el agregado, nada
más.

Implementación: `crates/motor/src/lib.rs`. El único punto donde un nombre
se ata a una función es el despacho del ejecutor
(`crates/pasos_demo/src/lib.rs::despacha`), fuera del motor.

## Consecuencias

**Positivas:**

- Añadir tipos de paso no toca el motor: la lógica de cada paso vive en su
  lado del contrato. El motor no cambia cuando el dominio crece.
- El motor es **determinista y testeable** sin hardware: solo depende de la
  secuencia y de los `estado` que devuelven los pasos (mockables).
- La semántica de ejecución (RF-01..RF-13) vive en un solo sitio y es la
  spec del producto, no repartida por tipos de paso.

**Negativas:**

- El motor **no puede** aplicar lógica de dominio (p. ej. un límite
  especializado) por sí mismo: esa lógica debe vivir en el paso o en un
  *expression engine* (RF-35). Es el precio del aislamiento.
- El `estado` como texto (no enum) es lo único que el motor entiende del
  resultado; tipos de resultado más ricos viven en el lado del paso y son
  opacos para el agregado.

**Neutras:**

- "Genérico" no significa "tonto": el motor sí aplica la semántica
  (corte en 1er fallo, Cleanup siempre, reintentos, agregado). Lo que no
  hace es *interpretar el dominio* de cada paso.

## Alternativas descartadas

- **Motor con conocimiento del dominio (estilo TestStand con step types
  integrados):** acopla el secuenciador a cada tipo de paso; frágil ante
  cambios.
- **Motor que ejecuta código (estilo OpenHTF fases Python):** pierde
  aislamiento y la separación datos/código (ADR-0002).

## Enlaces

- [ADR-0002](0002-secuencia-como-datos.md),
  [ADR-0003](0003-pasos-por-grpc-por-nombre.md),
  [diseno/motor-de-ejecucion.md](../diseno/motor-de-ejecucion.md).