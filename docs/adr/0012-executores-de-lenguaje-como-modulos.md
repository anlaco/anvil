# ADR-0012: Executores de lenguaje como módulos distribuidos

- **Estado:** Aceptada
- **Fecha:** 2026-08-03 (M5/alcance MVP extendido)
- **Relaciona:** ADR-0001, ADR-0003, ADR-0005, ADR-0006,
  ADR-0011, [arquitectura.md](../arquitectura.md),
  [modelo-de-pasos.md](../diseno/modelo-de-pasos.md),
  [integracion-instrumentos.md](../diseno/integracion-instrumentos.md),
  [contrato-grpc.md](../contrato-grpc.md)

## Contexto

Hasta M4b, Anvil se distribuye con **un único ejecutor de pasos**: el
`.wasm` `crates/ejecutor_pasos`, embebido en el binario nativo junto al
motor ([ADR-0011](0011-distribucion-un-binario-hospeda-wasmtime.md)), que
despacha por nombre a `pasos_demo` (pasos simulados compilados dentro).

Dos necesidades reales quedan fuera de eso:

1. **Bancos de prueba secuestrados por el SO.** Las DLLs/drivers del
   fabricante del instrumento atan el banco a un SO concreto (Windows 7/10,
   Ubuntu antiguo) durante 10+ años de mantenimiento. Anvil vive en un SO
   moderno y portable; pero el paso que toca hardware puede necesitar un SO
   que Anvil nunca va a ofrecer. Hoy el motor está hardcoded a un único
   endpoint en loopback (`crates/motor/src/bin/anvil.rs::conecta("127.0.0.1",
   9100)` y `anvil-host` rechaza IPs no-loopback), así que es imposible que
   un paso corra en otra máquina/SO.
2. **Pasos WASM propios sin recompilar.** El modelo `.vi` de TestStand: tú
   compilas tu módulo, lo guardas en un archivo, y la secuencia lo referencia
   por path — no recompilas el ejecutor. Hoy un paso WASM propio exige
   recompilar `crates/ejecutor_pasos`.

La tesis de producto ya anticipa la primera: "el adapter es gRPC: cualquier
lenguaje que hable el contrato protobuf es un adapter"
([ADR-0003](0003-pasos-por-grpc-por-nombre.md), `modelo-de-pasos.md`). Este
ADR materializa esa tesis como **producto**: Anvil construye y distribuye
esos ejecutores.

## Decisión

1. **Executores de lenguaje como módulos distribuidos.** Anvil distribuye,
   junto al binario, **ejecutores de lenguaje** — un servidor gRPC por
   sistema (Python, LabVIEW, MATLAB, …) que habla el contrato `paso.proto`.
   Viven como **módulos aparte** en el directorio `executores/`, licencia
   **Apache-2.0** (como `wasi-grpc`/`wasi-visa`, [ADR-0004](0004-licencia-dual-agpl-apache.md)):
   son piezas adoptables y extensibles, no parte del núcleo AGPL. El primero
   es **Python** (M5); LabVIEW/MATLAB/… son futuros. Son **alternativas**: se
   elige y se arranca el que haga falta; pueden correr a la vez y mezclarse
   en la misma secuencia.

2. **El ejecutor WASM sigue embebido y es el "de serie".** `crates/
   ejecutor_pasos` **se queda en `crates/`** (core, embebido en `anvil-host`,
   ADR-0011). WASM/Rust es el **lenguaje de serie** de un ejecutor de
   pruebas: zero-install (el binario trae todo), sin runtime de vendor, y es
   la apuesta filosófica (ADR-0001). La asimetría de layout es deliberada:
   WASM es core y de serie; los ejecutores de lenguaje son opt-in.

3. **Cargador de `.wasm` por path (modelo `.vi`), MVP extendido.** El
   ejecutor embebido evoluciona de "despacha `pasos_demo` fijos" a
   "despacha built-in + módulos `.wasm` **cargados por path en runtime**".
   La secuencia referencia un `.wasm` propio; el ejecutor lo carga y lo
   despacha por nombre, sin recompilar. `pasos_demo` queda como librería de
   pasos built-in de serie. **Cada módulo cargado corre en su propio
   `Store`** (aislamiento entre pasos, coherente con el aislamiento entre
   guests de ADR-0011): un paso defectuoso no bloquea al resto.

4. **El motor despacha por nombre→endpoint.** `Motor::conecta` pasa de "un
   endpoint" a una **tabla nombre→endpoint**. Cada ejecutor sigue atando
   nombre→función (ADR-0003 se conserva; solo se extiende el *quién* atiende
   cada nombre, no el *cómo*). Una secuencia mezcla pasos del ejecutor
   embebido (built-in o `.wasm` cargado) y de ejecutores de lenguaje
   arrancados aparte.

5. **Relajación acotada del loopback de ADR-0011.** Se añade un modo
   "external": el motor puede conectar a IPs **no-loopback declaradas** en la
   configuración (no por defecto). El sandbox WASM del motor y del ejecutor
   local se conserva; el riesgo aceptado es la red hacia el endpoint
   declarado. Sin declaración explícita, el comportamiento sigue siendo
   loopback-only (compatibilidad con ADR-0011).

6. **`paso.proto` no se toca** (RNF-05 intacto). El wire se reusa hacia los
   ejecutores de lenguaje y hacia los módulos `.wasm` cargados; el routing
   y la carga son motor/ejecutor-side. El contrato **no** se versiona en esta
   fase (pendiente ya señalado en [contrato-grpc.md](../contrato-grpc.md)).

7. **LID (Legacy Isolation Domain) = patrón de despliegue, no un tipo de
   ejecutor.** Cualquier ejecutor de lenguaje (p. ej. Python) puede
   desplegarse en un **SO legacy** (Windows 7/10, VM, PC en red) con
   **aislamiento declarado**: solo las puertas de salida necesarias
   (instrumentos por red, ficheros pactados). Anvil ve un endpoint gRPC más;
   no sabe ni le importa el SO. **El mecanismo de aislamiento (contenedor/
   VM/firewall de SO) queda a definir al construir**: se fija el patrón, no
   la tecnología. La investigación de opciones está en
   [investigacion/aislamiento-lid.md](../investigacion/aislamiento-lid.md);
   la decisión final se toma al construir el primer LID real.

8. **Rendimiento: WASM compite.** wasmtime compila WASM **JIT a código
   nativo** (Cranelift), no lo interpreta: típicamente 1.5–2× de C/Rust
   nativo, y ~30–100× más rápido que Python puro. Frente a una DLL nativa
   paga un ~10–30% por el sandbox, despreciable frente al tiempo de un
   instrumento real (RNF-04). **No hay razón para "lo rápido en DLL, lo
   lento en WASM"**: la división rápido/lento no se justifica. Caveat: la
   primera invocación de un módulo paga la compilación JIT una sola vez; un
   **cache AOT** (wasmtime lo soporta) queda post-MVP.

## Por qué esta forma

- **Materializa la tesis** ("adapter gRPC en cualquier lenguaje") como
  producto distribuible, no como promesa.
- **Zero-install preservado** (ADR-0011): WASM de serie, ejecutores de
  lenguaje opt-in que se arrancan cuando se necesitan.
- **Sin deuda de wire**: `paso.proto` reusado hacia todo; no hay segundo
  contrato.
- **Sin runtime de vendor en el motor** (ADR-0003): el motor no necesita
  Python/LabVIEW/MATLAB; quien corre un ejecutor de lenguaje instala ese
  runtime en su máquina (su elección, no requisito de Anvil).
- **Aislamiento conservado y extendido**: sandbox WASM del núcleo (ADR-0001)
  + `Store` por módulo cargado + LID con puertas declaradas.
- **Descarga la condena del SO** del banco: el paso legacy vive donde debe,
  Anvil evoluciona libre.
- **Prepara la distribución futura**: cada módulo es autocontenido y
  versionable → descargable desde la UI cuando exista (post-MVP).

## Recortes y compromisos

- **Cache AOT** de módulos `.wasm`: post-MVP.
- **Sidecar de configuración** de la tabla `ejecutores:` (fichero aparte
  reutilizable entre secuencias): post-MVP. En MVP va **embebida en el YAML**
  + override por flag `--ejecutor nombre=host:puerto` (mismo patrón que los
  límites: embebido primero, sidecar después, [ADR-0008](0008-limites-evaluados-por-el-motor.md)).
- **Descubrimiento automático** de ejecutores, balanceo y reconnect por
  endpoint: post-MVP (solo el reintento por paso existente, RF-07).
- **Descargables desde la UI**: post-MVP; la estructura de módulos lo permite
  sin rediseño.
- El ejecutor embebido sigue siendo **stateless entre llamadas**; los
  módulos `.wasm` cargados se re-cargan por invocación o se cachean en su
  `Store` (detalle de implementación, sin impacto en el contrato).

## Consecuencias

**Positivas:**

- La demo de producto demuestra la encapsulación completa: paso built-in +
  paso `.wasm` propio + paso en ejecutor Python (o LID en SO legacy), todo en
  la misma secuencia.
- El cargador `.wasm` da el modelo `.vi` de TestStand (compilar y referenciar,
  sin recompilar el ejecutor), con la ventaja añadida del sandbox.
- Anvil compite con TestStand en el escenario legacy: se integra con el banco
  viejo en vez de exigir migrarlo.

**Negativas:**

- El cargador `.wasm` por path y el routing multi-endpoint son **trabajo
  nuevo en el motor y el ejecutor** (M5), y se suman al alcance MVP extendido.
- La relajación del loopback añade una superficie de red configurable; se
  mitiga con "solo endpoints declarados" y sandbox WASM intacto.
- Mantener ejecutores de lenguaje es deuda sostenida (uno por sistema);
  aceptada: son módulos pequeños y Apache (adoptables por la comunidad).

**Neutras:**

- `executores/` convive con `crates/` (core Rust), `packaging/` (binario
  nativo) y `ejemplos/` (secuencias): cada pieza en su sitio.
- Los ejecutores de lenguaje usan **gRPC nativo de su ecosistema**
  (`grpcio`, `tonic`, …), no `wasi-grpc` (esa es solo para WASM, ADR-0006).

## Alternativas descartadas

- **Pasarela/proxy como componente aparte** entre motor y ejecutores:
  innecesaria — el ejecutor de lenguaje ya es gRPC; un proxy añade un salto
  y un componente sin ganancia (ADR-0003 ya da el aislamiento).
- **Mover el ejecutor WASM a `executores/`** (simetría de layout): rompe el
  zero-install (habría que arrancar un ejecutor antes de que cualquier
  secuencia funcione) y devalúa WASM al nivel de "un lenguaje más" cuando es
  la apuesta de fondo (ADR-0001).
- **Extender `paso.proto`** para ejecutores remotos o módulos `.wasm`:
  rompe RNF-05 sin necesidad — el routing y la carga son del lado del motor/
  ejecutor, no del wire.
- **"Lo rápido en DLL, lo lento en WASM"**: falsa división; WASM es JIT
  (no interpretado) y el cuello de botella real es el instrumento (RNF-04).
- **Aislamiento del LID vía WASM**: imposible — el LID debe correr DLLs
  nativas; su aislamiento es de red/FS declarados (contenedor/VM/firewall),
  no un sandbox de instrucciones.

## Enlaces

- [diseno/executores-lenguaje.md](../diseno/executores-lenguaje.md) (diseño
  y demo M5), [ADR-0001](0001-rust-wasm.md), [ADR-0003](0003-pasos-por-grpc-por-nombre.md),
  [ADR-0011](0011-distribucion-un-binario-hospeda-wasmtime.md),
  [contrato-grpc.md](../contrato-grpc.md), [glosario.md](../glosario.md).
