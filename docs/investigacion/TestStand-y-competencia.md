# Investigación: TestStand, competidores y requisitos del dominio

> Base citable para la documentación de producto de Anvil. Recoge lo que hace
> fuerte a NI TestStand, dónde sangra (con voz real de foros), el landscape
> open-source, el *white-space* donde Anvil puede ganar, los requisitos
> priorizados del dominio y las lecciones arquitectónicas.
>
> Fuentes: foro oficial NI Community (forums.ni.com), repos y docs oficiales
> de los competidores open-source. La voz de Reddit/Stack Overflow quedó
> pendiente por límite de uso (ver §8).

---

## 1. TestStand como adversario: qué lo hace especial

> Lo que sigue es la síntesis de **qué hace especial a TestStand** (no sus
> fallos), asentada en las palabras de NI (páginas *What is TestStand* y
> *Process Model Theory*) y en el mapa de conceptos de la investigación de
> dominio. Los puntos de dolor del §2 son la sombra de estas fortalezas.

### 1.1 Es un *test executive*, no un *test runner*

La distinción es el núcleo. NI define TestStand como *"off-the-shelf test
management software … that eliminates the need for time-intensive, in-house
sequencer development"* y lo categoriza como **Test Execution**. Un runner
(pytest, Robot) ejecuta código que tú escribes; TestStand es un **host** que
**orquesta *code modules* en lenguajes reales**: *"organize code modules from
various programming languages to run in a desired order"*, con *"adapters to
call code modules written in LabVIEW, C/C++, .NET, and Python, increasing
code-re-use and eliminating rework"*. La lógica de medición vive en esos
módulos; TestStand aporta la columna vertebral de producción (flujo,
variables, límites, reintentos, reporte, paralelismo, logging) **sin que tú
escribas el pegamento**. No escribes "un programa que prueba"; escribes
pruebas y TestStand las corre dentro de un harness de fábrica.

### 1.2 El process model: separar "el test" de "la línea" — la idea especial

ESTO es lo verdaderamente especial y lo que ningún OSS hace limpio. NI lo
dice así: *"Testing a UUT requires more than just executing a set of tests"*
— identificar el UUT, notificar pass/fail, loguear, generar reporte son
*"common operations"* que *"comprise a process model"*. Sin él, *"each test
sequence would need to provide the mechanism for these common tasks"*; con él,
*"Any modifications to the common operations need to be changed in only one
common location"*.

La separación es bidireccional: *"You can use a single process model with
several different test sequences"* y *"you can run a single test sequence
within several process models"* — la misma secuencia va de R&D a la fábrica
cambiando solo el modelo. La frase que lo condensa: ***"Thus the test process
can change but the tests executed remain the same."***

Tres modelos de fábrica — *"Each process model gives a different test
experience without requiring any modifications to the client sequence file"*:
- **Sequential** — un UUT a la vez.
- **Parallel** — varios UUTs simultáneos en fixtures independientes.
- **Batch** — muchos UUTs en el mismo fixture.

Y la idea arquitectónica que NI vende como *la* especial: el process model
**es él mismo una secuencia editable**, no infraestructura oculta —
*"By representing a process model as a sequence, it becomes simple to edit
and extremely flexible"*. Los **callbacks** dejan overridear comportamiento
del modelo desde la secuencia cliente sin tocar el modelo; los **entry
points** (Test UUTs, Single Pass) ofrecen modos de ejecución; los
**configuration entry points** exponen opciones (reporte, BD) como menús
auto-poblados — *"No code needs to be rewritten in operator interfaces to
add these options"*.

→ **Implicación para Anvil:** el process model es lo que hay que respetar y
lo que hay que **no** replicar 1:1 (es complejo y frágil, ver §2). El MVP
debe separar "la secuencia" de "cómo se corre en producción", pero con un
modelo Sequential simple y extensión por plug-ins, no heredando un process
model monolítico.

### 1.3 Authoring por no-programadores (el Sequence Editor)

*"An interactive environment to build your test sequence and more clearly
visualize execution flow"*: drag-and-drop de pasos, *"implement conditional
logic to modify sequence flow"*. En fabricación el equipo de test muchas
veces no es un equipo de software: que un técnico/EE authorice la secuencia
**sin escribir código** es exactamente lo que se compra. Ahí están el editor
comercial de OpenTAP y el hueco que ningún OSS cubre (§4, white-space nº1).

### 1.4 Step types + expression engine + sistema de propiedades

- **Step types** (built-in: Pass/Fail, Numeric Limit, Action, Sequence Call,
  Statement, Label, Property Loader, Synchronization…) y **custom step types**
  con substeps (Edit/Pre-Step/Post-Step/OnNewStep) para encapsular
  comportamiento repetitivo.
- **Expression engine** (sintaxis tipo C): precondiciones, postcondiciones,
  límites, asignaciones, parámetros a code modules.
- **Jerarquía de variables** con scopes (Locals / Parameters / FileGlobals /
  StationGlobals) — un árbol de propiedades tipado y auditable, el modelo
  mental de *"una hoja de cálculo para tests"*.

Permite cablear datos entre pasos y declarar límites **sin código pegamento**,
de forma auditable. (Los dolores del §2 — custom step types torpes, "override"
que no es lo que parece, documentación fragmentada — son la sombra de esta
potencia.)

> **Fuentes §1.4:** *Custom Step Type Development Best Practices* (NI, en §9);
> *Expressions* (NI Knowledge Article kA0VU000000BoXl0AK);
> *Variables — NI TestStand* (NI Community, td-p/3890418).

### 1.5 Resultados y reporte como columna vertebral, no como addon

Cada paso vierte su resultado a un `ResultList` que el generador de reportes
y el logger de BD consumen. NI: *"built-in, automatic reporting and
capabilities for database logging … HTML, XML, ATML, and ASCII"*. No
escribes código de reporte: es **arquitectónico**. Por eso la gente sufre el
XSLT y el schema de BD rígido (§2): porque el reporting es estructural, no
eludible.

> **Fuentes §1.5:** *What is TestStand* (NI, en §9, cita de reporting);
> *Using Databases and Reports with TestStand* (NI);
> *Report Generation and Customization* (NI, en §9). El `ResultList` y el
> report object model son conceptos del *System and Architecture Overview*
> (NI, en §9).

### 1.6 UIMsgs: desacoplo motor ↔ Operator Interface

Los User Interface Messages desacoplan el motor de la UI de operador: el
motor postea mensajes (trace, estado, errores) que cualquier Operator
Interface consume; los no soportados se ignoran. Así una UI corre
cualquier secuencia y viceversa. Es lo que permite tener un Sequence Editor
(dev) y Operator Interfaces (prod) intercambiables. Post-MVP para Anvil
(requiere UI gráfica).

> **Fuente §1.6:** *UI Messages* (NI Knowledge Article kA03q000000x3tWCAQ).

### 1.7 El moat (distinto de "lo bueno"): ecosistema + lock-in LabVIEW

Lo **bueno** son §1.1–1.6. Lo **difícil de reemplazar** es otra cosa:
décadas de VIs, drivers NI, plantillas, integradores certificados, mano de
obra formada, y el **acoplamiento a LabVIEW** que hace que salir cuestee
reescribir. NI lo reconoce implícitamente en su argumento build-vs-buy: el
TCO de TestStand es *"medium (license cost but low development/maintenance)"*
frente a un framework propio de *"high: development and maintenance cost"*.
Ese lock-in es a la vez la fortaleza de TestStand (retiene) y su mayor
fractura (la gente quiere salir — §2, "quitarse LabVIEW para recortar
licencias").

---

## 2. Dónde sangra TestStand (voz real de forums.ni.com) → oportunidades

| Área | El dolor real (cita) | Oportunidad Anvil |
|---|---|---|
| **Process model personalizado** | "a customized model will not get any future enhancements or bug fixes from NI unless you manually merge them in" — James_Grey (NI). Consenso: prefiere plug-ins sobre tocar el modelo. | Extensión por **plug-ins/ResultSinks**, no heredando un process model monolítico. |
| **Process model frágil** | Cambiar callbacks rompe todas las secuencias existentes; solo es seguro customizar *antes* de escribir secuencias — MBengths. | Contrato **versionado y estable**; extensión sin mutar el núcleo. |
| **Reportes ATML** | "found all customizations not so friendly"; XSLT opaco; recurren a ChatGPT o a un segundo reporte a mano — Tomerfl, Laurent_B. | Reporte como **dato abierto** (Parquet/JSON/CSV) + plantillas legibles, no XSLT. |
| **Database logging** | Schema fijo rígido; Locals inaccesibles desde la config; conexión cacheada que **rompe con corte de red sin auto-retry**; muchos lo bypasean — Exle, Vaaben. | `ResultSink` desacoplado con **reintento/reconexión** y schema configurable. |
| **Deployment** | "There is no systematic deployment approach"; builds de 30-70 min; monolítico; desconecta el código del source control; copian ficheros a mano — mateusz_owczarek, mwatkins, ~jiggawax. | Anvil = **WASM portable**: `wasmtime run anvil.wasm` + secuencia YAML. Sin instalador, sin recompilar todo. |
| **Lock-in LabVIEW** | En producción, con el adapter en runtime, **se lanza el LabVIEW Dev System igual**; bug sin arreglar de TS2020 a TS2023Q4 — PragmaTest. La gente quiere "quitarse LabVIEW para recortar licencias" — pawhan11. | Pasos por **gRPC en cualquier lenguaje**; ningún runtime de vendor atado. |
| **Dependencias compiladas ocultas** | VIs que van bien en Dev se rompen en runtime por el "Inplaceness Algorithm"; TestStand resuelve .NET/packed-libs distinto al IDE — Daniel-E, JoseRivero. | Pasos **aislados tras un contrato protobuf**; el interior del paso es opaco al motor. |
| **"Paralelo" que no aísla** | DLLs compartidas serializan sockets; conexiones TCP/SSH en conflicto; VIs no-reentrant degradan a secuencial en silencio — ebalci, PerW, Sowndarya. | WASM + un proceso/instancia por socket → **aislamiento real**; o concurrencia con cancelación jerárquica. |
| **UX confusa Batch vs Parallel** | Las opciones de sync del Batch aparecen en la UI del Parallel donde **no hacen nada** — Oli_Wachno. | Un solo modelo de ejecución simple primero; paralelismo explícito y honesto después. |
| **Documentación fragmentada** | Comportamiento clave repartido entre Help, User Manual y Reference Manual; ejemplos antiguos desaparecidos — david_jenkinson, giovanni.alfamation. | Docs **cohesionadas** desde el día 1. |

### 2.1 Coste y lock-in: el driver nº1 de migración

El coste de licencia es la razón principal por la que se buscan alternativas,
sobre todo en líneas multi-estación (datos NI / Bloomy 2023, TofuPilot):

| Concepto | Coste |
|---|---|
| TestStand Base Deployment License (perpetua, **por estación**) | **$793/estación** |
| TestStand Debug Deployment License (perpetua, por estación) | **$3,011/estación** |
| TestStand Development System (suscripción anual) | **$2,380/año** |
| NI Test Workflow Pro (TestStand + LabVIEW dev) | $3,995/año |
| All-in por seat/año (TofuPilot) | **~$4,310/seat/año** |

Más: licencia Windows por estación, formación especializada ($2K+/dev),
suscripción opcional WATS para analítica. Una línea de 10 estaciones suma
miles solo en deployment, además del Windows de cada una. **Anvil (WASM,
cross-platform, sin licencia por estación) ataca directamente este coste.**
El hilo de NI Community de 2014 ya lo documentaba: una empresa buscaba
alternativas porque "TestStand deployment license costs were out of budget
for multi-station factory deployment".

---

## 3. El landscape open-source

| Proyecto | Leng | Licencia | Secuencia | Reporte | Paralelismo | UI | Madurez | Límite vs TestStand |
|---|---|---|---|---|---|---|---|---|
| **OpenTAP** | C#/.NET | MPL-2.0 | XML `.TapPlan` | **ResultListener** (CSV/SQLite/Postgres) hilo propio | TapThreads + `ParallelStep` + abort jerárquico | CLI + **editor comercial** (KS8400) | ~235★, Keysight | Editor comercial; C#-céntrico; ecosistema chico |
| **Litmus** | Python | Apache-2.0 | YAML + pytest | **STDF/HDF5/TDMS/MDF4** + Parquet | "parallel sites" genérico | **UI web** de operador | v0.1.0, 1 dev | Inmaduro; sin plugins; motor ligero |
| **Semi-ATE** | Python | **GPL-2.0** | Wizards Spyder | STDF por MQTT | multisite hardware | Spyder IDE | ~65★ | GPL fuerte asusta; anclado a Spyder |
| **Robot Framework** | Python | Apache-2.0 | keyword tables `.robot` | XML/HTML + listeners | pabot (procesos) | RIDE | **11.6k★** | No es ATE: sin instrumentos/STDF nativos |
| **cocotb / VUnit** | Py+HDL | BSD/MPL | coroutines / VHDL | JUnit XML | sims paralelas | headless | 2.5k/839★ | No son secuenciadores ATE |
| **pytestlab** | Python | Apache-2.0 | código | HDF5/SQLite | `@session.task` | CLI/Jupyter | ~6★ | **Perfiles YAML + sim + record/replay**; sin STDF/editor |
| **pytation / sapas** | Python | Apache/MIT | código / YAML+CSV | ZIP / genérico | no / no | PySide6 / TUI | muy verde | Sin SCPI/STDF/editor visual |
| **OpenHTF** | Python | Apache-2.0 | fases Python (phase-based) | limits/units + **web operator UI** | limitado (paralelo DUT débil) | **web UI** | ~640★, Google | El reemplazo directo más cercano; comunidad chica |
| **Flojoy** | Python+TS | **AGPL-3.0** | bloques visuales | pass/fail + Flojoy Cloud | no | **editor visual** (Electron) | early, 1 dev | Python 3.11 only; posible pivot a "Nominal"; **mismo AGPL que Anvil** |
| **TestFlow** | web | freemium | texto → secuencia (AI) | browser | no | browser (AI-native) | nuevo | Genera secuencias desde lenguaje natural; muy nuevo |

> **Nota de posicionamiento:** Flojoy ya es un secuenciador de test **AGPL-3.0
> con editor visual** — comparte filosofía de licencia con Anvil. La
> diferencia clave: Flojoy es **Python/Electron mono-lenguaje**, Anvil apuesta
> por **pasos por gRPC en cualquier lenguaje + runtime WASM aislado**. OpenHTF
> es el reemplazo conceptual más cercano a TestStand (fases + limits + UI web)
> pero sin editor visual ni aislamiento.

---

## 4. White-space: dónde puede ganar Anvil (lo que NADIE cubre)

Ningún open-source combina estos siete puntos — y TestStand defiende el
primero con un editor comercial:

1. **Editor visual de flujo abierto** (branching/looping/decisión por step,
   editable en runtime sin recompilar). OpenTAP lo tiene pero comercial; el
   resto no lo tiene.
2. **Pasos por contrato gRPC tipado, lenguaje-agnóstico.** OpenTAP es
   C#-céntrico; todos los Python son mono-lenguaje; Semi-ATE usa MQTT sin
   contrato tipado.
3. **Runtime WASM portable y aislado.** Nadie sandboxea los pasos. OpenTAP
   corre .NET in-process (full trust); Python en el mismo intérprete.
4. **YAML diffable** como secuencia (no `.seq` binario, no código, no XML).
5. **ResultSinks industriales** (STDF + Parquet) como dato abierto consultable.
6. **Paralelismo con cancelación jerárquica** modelado de verdad (OpenTAP sí;
   el resto implícito o externo).
7. **Licencia dual AGPL/Apache** que protege el producto sin contagiar a
   quien integra las libs. Nadie la usa (OpenTAP=MPL, Semi-ATE=GPL fuerte,
   resto=Apache permisivo sin protección).

---

## 5. Requisitos reales del dominio (priorizados para un MVP)

Extraído de la voz de foros NI (usuarios reales de TestStand en producción).

**Must (MVP):**
- Semántica Setup→Main→Cleanup con **Cleanup garantizado** — *ya en Anvil*.
- **Reintentos por paso** con `intento` comunicado al paso — *ya en Anvil*.
- Estados **paso/fallo/error** con agregado (error > fallo) — *ya en Anvil*.
- **Contrato gRPC del paso** estable y versionado (`paso.proto`) — *ya en Anvil*.
- **Secuencia como dato** (YAML diffable) — pendiente: hoy se construye en código.
- **ResultSink desacoplado** (consola/JSON/CSV/SQLite) con reintento — pendiente: hoy es un `println!`.
- **Headless/CLI** primero (operator UI web = post-MVP).

**Should (MVP-parcial):**
- Límites high/low/comparación como **datos first-class** (no aserciones ad-hoc).
- Precondición por step.
- Variables Locals/Parameters/FileGlobals.
- Property loader (límites desde fichero externo) — separa datos de test del flujo.
- Formatos de reporte: **JSON + CSV + HTML** (STDF/ATML = post-MVP por sector).
- Empaquetado/versionado de pasos.
- Control de flujo básico: **pause-on-fail, step, disable** de pasos (estándar en todo ATE comercial).

**Could (post-MVP):**
- Process model Sequential + paralelismo con cancelación jerárquica.
- STDF/ATML exporters (semiconductora/aerospace).
- Perfiles YAML de instrumentos + sim + record/replay (copiar de pytestlab).
- Operator UI web (copiar de Litmus) con **roles** (admin/engineer/technician/operator) y login separado del SO — estándar en ATE comercial (Astronics/Advantest, ProDSP).
- **Online limit editing** (cambiar límites sin re-deploy) — demandado en producción.
- **MES/ERP integration** y trazabilidad por serial number.
- **Golden-sample / Jidoka** supervision y monitoring en tiempo real.
- Expression engine (subconjunto).
- Custom step types.

**Won't (al menos en v1):**
- Replicar el process model de TestStand 1:1 (Parallel/Batch con sus callbacks/entry points).
- Integración con LabVIEW/CVI.
- Debugger visual completo.
- UIMsgs (requiere UI gráfica).

---

## 6. Lecciones arquitectónicas concretas para Anvil

- **Copiar de OpenTAP:** `ResultListener` desacoplado en hilo propio con
  lifecycle `on_plan_start→on_step_start→on_result→on_step_end→on_plan_end`;
  jerarquía Resource (DUT/Instrument) con `open`/`close` paralelo y
  `Pre/Run/Post`; TapThreads con abort jerárquico (`CancellationToken` en
  Rust async).
- **Copiar de pytestlab:** perfiles YAML de instrumentos con validación de
  schema + `SimBackend` determinista + record/replay estricto
  (`ReplayMismatchError`) → CI sin hardware.
- **Copiar de Litmus:** exporters industriales (STDF) como ResultSink
  first-class; resultados como dato abierto (Parquet/DuckDB); operator UI web.
- **Adoptar:** YAML para secuencias (con sidecar de límites); gRPC+protobuf
  como contrato de paso (posición propia, no cubierta).
- **Evitar:** editor atado a licencia comercial; núcleo GPL fuerte; aserciones
  ad-hoc como límites; XML/HTML genérico como único reporte; regenerar código
  desde wizards; paralelismo implícito.

---

## 7. Posicionamiento recomendado (una frase)

> **Anvil es el secuenciador de test de producción open-source: secuencia =
> datos en YAML, pasos en cualquier lenguaje tras un contrato gRPC, motor en
> WASM portable y aislado, resultados como dato abierto — la alternativa a
> TestStand sin el lock-in de vendor ni el editor cerrado.**

---

## 8. Limitaciones de esta investigación

- **Conseguido:** foro oficial NI (33 hilos reales con citas y URLs — process
  models, reportes/BD/deployment, LabVIEW/paralelismo), mapeo de competidores
  open-source (OpenTAP, Litmus, Semi-ATE, Robot Framework, cocotb, VUnit,
  pytestlab, pytation, sapas, **OpenHTF, Flojoy, TestFlow**), análisis de
  dominio, **costes reales de licencia de TestStand** y **must-haves de ATE
  de producción** (de ATE comercial: Astronics/Advantest ActivATE, ProDSP,
  Gubo OneTest.SLT).
- **La voz de Reddit/Stack Overflow** (que cayó por límite de uso 429) se
  compensó con LAVA forum, Hacker News (hilo de Flojoy), LeCroy Owners Group
  y NI Community (hilos de alternativas open-source). El ángel de
  practicantes queda razonablemente cubierto; si se quiere más profundidad en
  r/labview y r/embedded, se retoma cuando se libere el límite del backend.
- **Hallazgo de posicionamiento:** Flojoy ya es un secuenciador AGPL-3.0 con
  editor visual — Anvil no es el único en esa franja. La diferenciación se
  sostiene en **gRPC multilenguaje + WASM aislado**, no en la licencia ni en
  el editor visual per se.

---

## 9. Fuentes

**Foro NI Community (forums.ni.com) — hilos citados:**
- Modifying process model vs plug-in vs add-ons — https://forums.ni.com/t5/NI-TestStand/Modifying-process-model-vs-using-plug-in-vs-using-add-ons/td-p/3950365
- Where to insert more AddExtraResult methods — https://forums.ni.com/t5/NI-TestStand/Where-to-insert-more-AddExtraResult-methods/td-p/4258044
- Which part of the process model to avoid when changed — https://forums.ni.com/t5/NI-TestStand/Which-part-of-the-process-model-to-avoid-when-changed/td-p/3882143
- Calling a custom substep — https://forums.ni.com/t5/NI-TestStand/Calling-a-custom-substep/td-p/422370
- TestStand API C# adding an OnNewStep substep — https://forums.ni.com/t5/NI-TestStand/TestStand-API-C-adding-an-OnNewStep-substep/td-p/3789123
- Custom TestStand Step Type (C# Example) — https://forums.ni.com/t5/NI-TestStand/Custom-TestStand-Step-Type-C-Example/td-p/635635
- How to create Custom Step Types: Is there any example? — https://forums.ni.com/t5/NI-TestStand/How-to-create-Custom-Step-Types-Is-there-any-example/td-p/3575016
- How to develop a custom step type with a pane using C# — https://forums.ni.com/t5/NI-TestStand/How-to-develop-a-custom-step-type-with-a-pane-using-C-and-load/td-p/4415725
- Can I override Engine Callback in Process Model? — https://forums.ni.com/t5/NI-TestStand/Can-I-override-Engine-Callback-in-Process-Model/td-p/1047443
- Engine Callbacks Confusion question — https://forums.ni.com/t5/NI-TestStand/Engine-Callbacks-Confusion-question/td-p/3193203
- Passing Sequence arguments to an execution using a process model — https://forums.ni.com/t5/NI-TestStand/Passing-Sequence-arguments-to-an-execution-using-a-process-model/td-p/712960
- Order of events in a step? — https://forums.ni.com/t5/NI-TestStand/order-of-events-in-a-step/td-p/915156
- Create new and better report on TestStand — https://forums.ni.com/t5/NI-TestStand/Create-new-and-better-report-on-TestStand/td-p/4459376
- NI TestStand 2024 Q4 'ReportOptions' Customization — https://forums.ni.com/t5/NI-TestStand/NI-TestStand-2024-Q4-ReportOptions-Customization/td-p/4477237
- Custom Report Path and Operator Field in Reports — https://forums.ni.com/t5/NI-TestStand/Custom-Report-Path-and-Operator-Field-in-Reports/td-p/4440008
- Logging data to a custom database scheme — https://forums.ni.com/t5/NI-TestStand/Logging-data-to-a-custom-database-scheme/td-p/3863809
- Customize Database schema, and collect data in TestStand — https://forums.ni.com/t5/NI-TestStand/Customize-Database-schema-and-collect-data-in-teststand/td-p/3617109
- Database logging — how to retry/reconnect after network or server problem — https://forums.ni.com/t5/NI-TestStand/Database-logging-how-to-retry-reconnect-after-network-or-server/td-p/3337420
- Error when logging to MySQL database after windows update — https://forums.ni.com/t5/NI-TestStand/Error-when-logging-to-MySQL-database-after-windows-update/td-p/4400844
- Deployment methods for distributing test stations - discussion — https://forums.ni.com/t5/NI-TestStand/Deployment-methods-for-distributing-test-stations-discussion/td-p/4086504
- errors in TestStand Deployment process — https://forums.ni.com/t5/NI-TestStand/errors-in-TestStand-Deployment-process/td-p/4173568
- deployment issues — https://forums.ni.com/t5/NI-TestStand/deployment-issues/td-p/3208154
- Deployment utility slow build time — https://forums.ni.com/t5/NI-TestStand/Deployment-utility-slow-build-time/td-p/3946140
- TestStand package deployment installation directory — https://forums.ni.com/t5/NI-TestStand/TestStand-package-deployment-installation-directory/td-p/4417111
- TestStand Base Install Location Wrong — https://forums.ni.com/t5/NI-TestStand/TestStand-Base-Install-Location-Wrong/td-p/4318663
- Deployment Of Custom UI And Sequence Files — https://forums.ni.com/t5/NI-TestStand/Deployment-Of-Custom-UI-And-Sequence-Files/td-p/3806924
- Improvement of LabVIEW Packed Libraries TestStand options (Idea Exchange) — https://forums.ni.com/t5/NI-TestStand-Idea-Exchange/Improvement-of-LabVIEW-Packed-Libraries-TestStand-options/idi-p/4307527
- LabView TestStand interoperability with LabView runtime adapter — https://forums.ni.com/t5/NI-TestStand/LabView-TestStand-interoperability-with-LabView-runtime-adapter/td-p/3151251
- TestStand missing dependencies on VI that runs correctly in LabVIEW — https://forums.ni.com/t5/NI-TestStand/TestStand-missing-dependencies-on-VI-that-runs-correctly-in/td-p/4144683
- Two parallel executions calling a DLL function — https://forums.ni.com/t5/NI-TestStand/Two-parallel-executions-calling-a-DLL-function/td-p/1334738
- Teststand parallel execution (batch model, reentrant VIs) — https://forums.ni.com/t5/NI-TestStand/Teststand-parallel-execution/td-p/4070264
- Issues with parallel execution (batch model TCP/SSH timeouts) — https://forums.ni.com/t5/NI-TestStand/Issues-with-parallel-execution/td-p/3276486
- Lock Synchronization (batch vs parallel model confusion) — https://forums.ni.com/t5/NI-TestStand/Lock-Synchronization/td-p/4424999
- Parallel or Sequential Process models — https://forums.ni.com/t5/NI-TestStand/Parallel-or-Sequential-Process-models/td-p/3215840

**Docs oficiales NI TestStand:**
- What is TestStand (definición de test executive, build-vs-buy) — https://www.ni.com/en/shop/electronic-test-instrumentation/application-software-for-electronic-test-and-instrumentation-category/what-is-teststand.html
- Process Model Theory (separación secuencia vs. process model — la idea especial) — https://www.ni.com/en/shop/electronic-test-instrumentation/application-software-for-electronic-test-and-instrumentation-category/what-is-teststand/process-model-theory.html
- System and Architecture Overview — https://docs-be.ni.com/bundle/teststand-system-and-architecture-overview/raw/resource/enus/373457f.pdf
- Advanced Architecture Series — https://www.ni.com/en/support/documentation/supplemental/08/teststand-advanced-architecture-series.html
- Custom Step Type Development Best Practices — https://www.ni.com/en/support/documentation/supplemental/08/teststand-custom-step-type-development-best-practices.html
- Report Generation and Customization — https://www.ni.com/en/support/documentation/supplemental/08/teststand-report-generation-and-customization.html
- Using Databases and Reports with TestStand — https://www.ni.com/en/shop/electronic-test-instrumentation/application-software-for-electronic-test-and-instrumentation-category/what-is-teststand/using-databases-and-reports-with-teststand.html
- Process Model Development and Customization — https://www.ni.com/en/support/documentation/supplemental/08/teststand-process-model-development-and-customization.html
- System Deployment Best Practices — https://www.ni.com/en/support/documentation/supplemental/08/teststand-system-deployment-best-practices.html
- Expressions (NI Knowledge Article) — https://knowledge.ni.com/KnowledgeArticleDetails?id=kA0VU000000BoXl0AK
- UI Messages (NI Knowledge Article) — https://knowledge.ni.com/KnowledgeArticleDetails?id=kA03q000000x3tWCAQ
- Variables — NI TestStand (NI Community) — https://forums.ni.com/t5/NI-TestStand/Variables-NI-TestStand/td-p/3890418

**Competidores open-source (repos / docs):**
- OpenTAP — https://github.com/opentap/opentap · https://doc.opentap.io · ResultListener: https://doc.opentap.io/Developer%20Guide/Result%20Listener/Readme.html · ParallelStep: https://github.com/opentap/opentap/blob/main/BasicSteps/ParallelStep.cs
- Litmus — https://github.com/pragmatest-dev/litmus · https://pragmatest.com/litmus
- Semi-ATE — https://github.com/Semi-ATE/Semi-ATE · https://semi-ate.github.io/Semi-ATE/ · Sequencer spec: https://semi-ate.github.io/Semi-ATE/SequencerInterface.html
- Robot Framework — https://github.com/robotframework/robotframework · https://robotframework.org · RIDE: https://github.com/robotframework/RIDE · pabot: https://github.com/mkorpela/pabot
- cocotb — https://github.com/cocotb/cocotb · https://docs.cocotb.org
- VUnit — https://github.com/VUnit/vunit · https://vunit.github.io
- pytation — https://github.com/jetperch/pytation · https://jetperch.github.io/pytation/
- sapas — https://github.com/kumamodo/sapas
- pytestlab — https://github.com/labiium/pytestlab · https://pytestlab.org
- OpenHTF (Google) — https://github.com/google/openhtf
- Flojoy — https://github.com/flojoy-ai/studio · https://flojoy.ai
- TestFlow — https://testflowinc.com/blog/ni-teststand-alternatives

**Coste de licencia y alternativas (voz de practicantes):**
- NI TestStand licensing — https://www.ni.com/en/shop/electronic-test-instrumentation/application-software-for-electronic-test-and-instrumentation-category/what-is-teststand/select-license
- Bloomy — Navigating NI's New Software Licensing Model — https://www.bloomy.com/support/blog/navigating-nis-new-software-licensing-model
- TofuPilot — TestStand Alternatives for Manufacturing Test — https://www.tofupilot.com/guides/teststand-alternatives-with-tofupilot
- NI — Test Executive Software: Build or Buy? — https://www.ni.com/en/shop/electronic-test-instrumentation/application-software-for-electronic-test-and-instrumentation-category/what-is-teststand/test-executive-software---build-or-buy--a-financial-comparison-u.html
- LAVA forum — Open source alternatives to TestStand? — https://lavag.org/topic/22024-open-source-alternatives-to-teststand/
- Hacker News — Free, Open-Source Alternative to LabVIEW and TestStand (Flojoy) — https://news.ycombinator.com/item?id=39555507
- NI Community — Open Source Test Executive (Medulla/ViPER) — https://forums.ni.com/t5/LabVIEW/Open-Source-Test-Executive/td-p/4178876
- NI Community — Alternative LabVIEW Test Executive/Sequence Tools (2014, coste de deployment) — https://forums.ni.com/t5/LabVIEW/Alternative-LabVIEW-Test-Executive-Sequence-Tools-Available/td-p/2865546

**ATE comercial (must-haves de producción):**
- Astronics/Advantest ActivATE — https://www.astronics.com/productinfo?productgroup=Test+%26+Measurement&subitem=ActivATE+Test+Management+Software · https://www.advantest.com/en/products/component-test-system/system-level-test-systems/activate/
- ProDSP ATE Supervisor — https://prodsp.hu/en/products/software/209-ate-supervisor
- Gubo OneTest.SLT — https://www.guwave.com/en/OneTest_SLT/