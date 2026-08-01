# Visión

> **Anvil es el secuenciador de test de producción open-source: secuencia =
> datos en YAML, pasos en cualquier lenguaje tras un contrato gRPC, motor en
> WASM portable y aislado, resultados como dato abierto — la alternativa a
> TestStand sin el lock-in de vendor ni el editor cerrado.**

## El problema

NI TestStand es el *test executive* de facto de la industria: orquesta *code
modules* en lenguajes reales (LabVIEW, C/C++, .NET, Python) sin que tú
escribas el pegamento — flujo, variables, límites, reintentos, reporte,
paralelismo. Esa columna vertebral es lo que se compra. Pero sangra en lo
que cuesta migrar:

- **Coste de licencia por estación** (~$4,310/seat/año todo incluido), el
  driver nº1 de migración en líneas multi-estación.
- **Lock-in de LabVIEW**: en producción se lanza el Dev System igual; salir
  cuesta reescribir.
- **Deployment monolítico** (builds de 30–70 min, sin approach sistemático,
  ficheros copiados a mano).
- **Reportes/BD rígidos** (XSLT opaco, schema de BD fijo, conexiones que
  rompen sin auto-retry) y **process model frágil** (tocar callbacks rompe
  todas las secuencias existentes).
- **Paralelismo que no aísla** (DLLs compartidas, sockets en conflicto).

Las fuentes y citas literales del foro NI están en
[`investigacion/TestStand-y-competencia.md`](investigacion/TestStand-y-competencia.md) §2.

## La propuesta de valor

Anvil rescata la idea buena de TestStand —*el secuenciador es el host que
orquesta, no un runner que ejecuta tu código*— y la reconstruye sin su
carga:

- **Secuencia = datos (YAML diffable)**, no un `.seq` binario ni código. Se
  versiona, se revisa y se diffa como cualquier fuente.
- **Pasos por gRPC por su nombre**, en cualquier lenguaje. El adapter *es*
  gRPC: no hay código de pegamento en el motor ni runtime de vendor atado.
- **Motor en WASM portable y aislado**: `wasmtime run anvil.wasm` + una
  secuencia YAML, sin instalador ni recompilar todo. Cada paso corre tras un
  contrato protobuf opaco al motor.
- **Resultados como dato abierto** (JSON/CSV/SQLite; STDF/Parquet post-MVP),
  no XSLT.
- **Licencia dual AGPL/Apache** que protege el producto sin contagiar a
  quien integra las librerías.

## Análisis competitivo

Resumen del landscape (detalle en investigación §3):

| Proyecto | Límite vs. Anvil |
|---|---|
| **NI TestStand** | El adversario. Fuerte en process model + ecosistema; sangra en coste, lock-in LabVIEW, deployment, reportes rígidos. |
| **OpenTAP** | Lo más cercano en seriedad (C#/.NET, MPL-2.0, editor comercial). C#-céntrico; pasos in-process full-trust; ecosistema chico. |
| **OpenHTF** | El reemplazo conceptual más cercano (fases + limits + web UI), pero Python mono-lenguaje, sin editor visual ni aislamiento. |
| **Flojoy** | **Ya es AGPL-3.0 con editor visual.** Python/Electron mono-lenguaje. *Mismo AGPL que Anvil.* |
| **Litmus / Semi-ATE / pytestlab / otros** | Inmaduros, Python-céntricos, sin contrato tipado ni aislamiento. |

### Dónde se sostiene la diferenciación (cuidado)

**Flojoy ya es un secuenciador AGPL-3.0 con editor visual.** Anvil **no** se
diferencia por la licencia ni por tener un editor visual per se. La
diferencia se sostiene en lo que nadie combina (investigación §4):

1. **Pasos por contrato gRPC tipado, lenguaje-agnóstico.** OpenTAP es
   C#-céntrico; todos los Python son mono-lenguaje; Semi-ATE usa MQTT sin
   contrato tipado.
2. **Runtime WASM portable y aislado.** Nadie sandboxea los pasos (OpenTAP
   corre .NET in-process; Python en el mismo intérprete).
3. **YAML diffable** como secuencia (no `.seq` binario, no código, no XML).
4. **ResultSinks industriales** (STDF + Parquet) como dato abierto.
5. **Paralelismo con cancelación jerárquica** modelado de verdad (post-MVP).
6. **Licencia dual AGPL/Apache** que protege sin contagiar a quien integra
   las libs (OpenTAP=MPL, Semi-ATE=GPL fuerte, resto=Apache sin protección).

El editor visual de flujo abierto (white-space nº1) es un *podría* futuro,
no la tesis: OpenTAP ya lo tiene comercial y Flojoy ya lo tiene en OSS. La
tesis es **gRPC multilenguaje + WASM aislado + YAML diffable + ResultSinks
industriales**.

## Público objetivo

- **Ingenieros de test que vienen de TestStand**, en líneas multi-estación
  donde el coste por seat y el lock-in LabVIEW pesan.
- **Equipos que quieren pasos en más de un lenguaje** sin atarse a un vendor.
- **Entornos que necesitan deployment portable** (una estación Linux, no un
  Windows + instalador por puesto).

## Invariants del producto

Lo que define a Anvil y no se negocia sin un ADR que lo reemplace:

- La secuencia es **datos, no código** ([ADR-0002](adr/0002-secuencia-como-datos.md)).
- Cada paso se invoca **por gRPC por su nombre**, nunca por llamada directa
  ([ADR-0003](adr/0003-pasos-por-grpc-por-nombre.md)).
- El **motor no sabe qué hace cada paso**; despacha por nombre
  ([ADR-0005](adr/0005-motor-generico-dirigido-por-datos.md)).
- El runtime es **WASM portable y aislado** ([ADR-0001](adr/0001-rust-wasm.md)).
- **Licencia dual** AGPL producto / Apache librerías
  ([ADR-0004](adr/0004-licencia-dual-agpl-apache.md)).
- Los **resultados son dato abierto**, no un reporte opaco.

## Alcance MVP vs. post-MVP

**MVP** (lo que ya existe como prototipo o entra en v1):

- Semántica Setup→Main→Cleanup con Cleanup garantizado y corte en 1er fallo.
- Reintentos por paso (el `intento` llega al paso).
- Estados `paso`/`fallo`/`error` con agregado `error > fallo`.
- Contrato gRPC del paso estable y versionado (`paso.proto`).
- Secuencia como datos (hoy construida en código; falta el cargador YAML).
- ResultSink desacoplado (consola/JSON/CSV/SQLite) con reintento — hoy es un
  `println!`.
- Headless/CLI primero.

**Post-MVP** (explícitamente fuera de v1):

- Process model Sequential + paralelismo con cancelación jerárquica (MVP =
  Sequential simple + plug-ins, **no** el process model de TestStand 1:1).
- Editor visual de flujo abierto.
- Operator UI web + UIMsgs.
- STDF/ATML exporters, MES/ERP, trazabilidad por serial number.
- Expression engine avanzado (subconjunto en MVP), custom step types,
  StationGlobals, property loader.

**Out-of-scope** (al menos en v1): replicar el process model de TestStand
1:1 (Parallel/Batch + callbacks + entry points), integración con
LabVIEW/CVI, debugger visual completo.

El detalle verificable está en [requisitos.md](requisitos.md); los hitos en
[roadmap.md](roadmap.md).

## Riesgos mayores

- **Hardware real.** Anvil opera instrumentos físicos: un bug puede dañar
  equipo o ser un riesgo de seguridad. Lo trata [SECURITY.md](../SECURITY.md)
  y la semántica de Cleanup siempre.
- **AGPL y adopción empresarial.** Las empresas suelen prohibir AGPL; la
  estrategia dual lo responde (ver [licencia.md](licencia.md)).
- **Determinismo/rendimiento en WASM.** El coste de una llamada gRPC local
  es irrelevante frente a un instrumento real, pero hay que verificar que
  WASM no introduzca no-determinismo en los reintentos (ver
  [arquitectura.md](arquitectura.md)).