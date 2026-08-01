# ADR-0002: La secuencia es datos, no código

- **Estado:** Aceptada (decisión pre-existente, formalizada aquí)
- **Fecha:** pre-prototipo

## Contexto

En TestStand la secuencia es un archivo `.seq` binario y, en la práctica,
buena parte del comportamiento vive en code modules y callbacks que se
toquean a mano. Eso dificulta versionar, diffar y revisar qué hace una línea
de test. En los secuenciadores Python (OpenHTF, Litmus), la secuencia *es*
código Python: potente pero opaco para un técnico que authorice sin
programar.

Anvil quiere que la secuencia sea **revisable como cualquier fuente** y que
un motor genérico la recorra sin saber nada del dominio (ADR-0005).

## Decisión

La secuencia es **datos**, no código. Se modela como
`DefinicionSecuencia{nombre, pasos_setup, pasos_main, pasos_cleanup}` en
`crates/modelo/src/lib.rs`, donde cada paso es `DefinicionPaso{nombre,
reintentos}`. El motor la recorre sin interpretar nada del dominio.

El objetivo de entrada es **YAML** (diffable, legible por no-programadores).
Hoy la secuencia se construye en código
(`crates/motor/src/bin/basica_datos.rs`); el cargador YAML es
[pendiente](../diseno/formato-de-secuencia.md) (RF-20).

## Consecuencias

**Positivas:**

- Las secuencias se versionan y se revisan en Git como cualquier fuente:
  `git diff` muestra qué cambió en una línea de test.
- Un motor genérico (ADR-0005) puede recorrerlas sin acoplarse al dominio.
- La secuencia **no es obra derivada** del secuenciador → no contagia
  licencia (clave para [licencia.md](../licencia.md), ADR-0004).

**Negativas:**

- Lo que hoy es un `match` en código (p. ej. `despacha` en `pasos_demo`)
  necesita un esquema y un cargador para expresarse desde fuera → trabajo
  de diseño (formato-de-secuencia.md).
- La lógica condicional no puede vivir "en la secuencia" como código: hace
  falta un *expression engine* (RF-35) para precondiciones/límites sin
  volver a meter código.

**Neutras:**

- La secuencia es datos *para el motor*; los pasos siguen siendo código en
  cualquier lenguaje (ADR-0003). No se prohibe programar, se separa el
  *qué correr* del *cómo se ejecuta*.

## Alternativas descartadas

- **Secuencia como código (estilo OpenHTF):** potente, pero pierde
  diffabilidad y authoring por no-programadores.
- **`.seq` binario (estilo TestStand):** opaco al control de versiones.

## Enlaces

- [ADR-0005](0005-motor-generico-dirigido-por-datos.md),
  [ADR-0004](0004-licencia-dual-agpl-apache.md),
  [diseno/formato-de-secuencia.md](../diseno/formato-de-secuencia.md).