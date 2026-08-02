# Plan: M4b — Sequence call / subsecuencias

> **Alcance acordado:** M4b. Sequence call (RF-27) con **Parameters de
> entrada/salida by reference** (como TestStand) y **anidamiento del
> `ResultadoSecuencia`**. **NO toca `paso.proto`** (sequence call es
> motor-side, como `statement` en M4-núcleo: patrón ADR-0008 → ADR-0009 →
> ADR-0010).

## Decisiones de diseño (acordadas)

1. **Subsecuencias inline O por path.** Una subsecuencia puede declararse:
   - **Inline** dentro del mismo archivo, bajo un mapa `subsecuencias:` —
     útil cuando sólo la usa esa secuencia. Se referencia **por nombre**
     (`secuencia: init_comun`).
   - **En archivo aparte** — útil para reutilizarla desde varias
     secuencias. Se referencia **por path relativo**
     (`secuencia: ./medir_fuentes.yaml`).

   Un archivo expone su **secuencia raíz** (la de `nombre:`) como pública;
   sus `subsecuencias:` inline son **privadas del archivo** (sólo las puede
   invocar una secuencia de ese mismo archivo). Así un archivo = una
   secuencia pública reutilizable por path + subsecuencias locales por
   nombre. Coherente con `formato-de-secuencia.md` l.103: el `include` de
   YAML no es el mecanismo; el sequence call sí, y ahora por las dos rutas.

   **Convención para distinguir nombre vs path en `secuencia`:**
   - Si contiene `/` (o `\`) o termina en `.yaml`/`.yml` → **path**
     relativo al directorio del archivo que lo contiene → secuencia raíz
     de ese archivo.
   - Si no → **nombre** → se busca en `subsecuencias` de la secuencia que
     contiene el paso (inline). Fail-fast al cargar si no aparece en
     ninguna de las dos.

2. **Resultado anidado.** El sequence call produce **un** `ResultadoStep`
   cuyo `estado` es el agregado de la subsecuencia y que lleva sus
   sub-pasos anidados. El reporte textual los indenta (extensión aditiva
   de RNF-08); JSON los anida; CSV los aplanea como filas extra con
   `nombre_paso` prefijado (`test_fuentes/medir_canal_1`) **sin** añadir
   columnas (la cabecera congelada del CSV no cambia).

3. **`paso.proto` no cambia** (ADR-0008/0009): el motor orquesta la
   subsecuencia; el paso gRPC sigue sin saber que vive dentro de una
   subsecuencia. RNF-05 (contrato estable) se respeta.

4. **Parameters de entrada/salida by reference** (como TestStand). El
   Sequence Call mapea cada Parameter de la subsecuencia a una **variable
   local del padre** (`locals.X`):
   - **Entrada:** al iniciar la subsecuencia, el motor copia `locals.X`
     → `parameters.P`.
   - La subsecuencia **escribe en `parameters.P`** durante su ejecución
     (`asigna`/`statement`).
   - **Salida:** al volver, el motor copia `parameters.P` (final) → `locals.X`.
   - Un Parameter es **entrada y salida** (mismo nombre), como TestStand
     by-reference (default). Es el canal caller↔callee que reemplaza a
     FileGlobals/StationGlobals para devolver valores — la best practice
     de NI, que Anvil copia.

   **Relajación de "sólo se muta Locals" (acotada):** la regla de M4-núcleo
   (ADR-0009) prohibía escribir fuera de `locals` para mantener el **paso
   gRPC** aislado. Ese principio se mantiene: el paso gRPC **sigue** sin
   tocar variables. La subsecuencia, en cambio, es lógica motor-side (como
   `statement`/`asigna`, que ya mutan el entorno), y escribir en sus
   propios `parameters` es su contrato de retorno con el llamador. Así:
   `escribe(Scope::Parameters)` pasa a permitirse **sólo cuando el entorno
   pertenece a una subsecuencia** (flag `parameters_mutables`). La raíz no
   puede escribir en sus `parameters` (no tiene a quién devolver); sus
   `parameters` son de sólo lectura. `escribe(Scope::FileGlobals)` sigue
   prohibido siempre; `escribe(Scope::Resultado)` sigue sin mutarse
   directamente (el motor lo setea).

## Recortes MVP-parcial (señalados, no ocultos)
- **`parametros` sólo admite `locals.X`** (un lvalue local del padre).
  No se admiten literales ni expresiones generales como argumento: para
  pasar un valor calculado, se calcula antes en un Local (con un
  `statement` o `asigna`) y se pasa ese Local por referencia — como en
  TestStand by-reference. Pasar `parameters.X`/`file_globals.X` del padre
  como destino (by-reference transitivo) queda **post-MVP**.
- **Sin by-value explícito.** Todos los argumentos son by-reference
  (entrada+salida al mismo `locals.X`). El modo by-value (entrada sin
  retorno, para aislar) de TestStand queda **post-MVP**: hoy, si no
  quieres que la subsecuencia te pise la variable, le pasas un Local
  temporal dedicado.
- **Sequence call sin reintentos ni límite.** Un sequence call no mide:
  no tiene `valor_medido` y su `estado` es el agregado de la subsecuencia.
  Se valida al cargar que **no** declare `reintentos > 1` ni `limite`
  (fail-fast, consistente con `deny_unknown_fields`).
- **Recursión limitada.** Profundidad máxima de anidamiento (64) como
  defensa ante un ciclo que escapara a la detección al cargar. No es la
  barrera principal: los ciclos se detectan al cargar.
- **Valores escalares.** Los Parameters son `Value` (número/bool/texto/
  nulo), no estructuras. Árboles de propiedades tipados recursivos siguen
  post-MVP, igual que el resto de los scopes.

## Requisitos cubiertos
- **RF-27** Sequence call: invoca otra secuencia como un paso, con
  Parameters de entrada/salida reales y anidamiento del
  `ResultadoSecuencia`.
- **RF-31** Parameters: dejan de estar "vacíos y reservados" (M4-núcleo)
  y se cablean como entrada/salida by reference con la subsecuencia.

## Principios rectores
- ADR-0005: el motor es genérico y **no conoce el sistema de ficheros**.
  El cargador resuelve y valida todas las subsecuencias (inline y
  externas) al cargar; el motor sólo recorre `DefinicionSecuencia` ya
  resueltas. Por eso el motor no gana dependencia de `cargador` ni de
  `std::fs`.
- ADR-0008/0009: el motor orquesta la subsecuencia contra su propio
  `EntornoMotor`; el paso por gRPC no se entera. El contrato no cambia.
- El paso gRPC sigue aislado: nunca muta variables del motor. La
  subsecuencia (motor-side) puede además mutar sus `parameters` (su
  contrato de retorno); la raíz, no.
- Fail-fast al cargar (consistente con M1–M4): los nombres se resuelven
  contra inline, los paths se resuelven contra archivos, los ciclos se
  detectan, los argumentos son lvalues válidos y se chequean contra la
  firma **antes** de ejecutar nada.

---

## Arquitectura

El modelo es **recursivo en `DefinicionSecuencia`**: una secuencia puede
llevar `subsecuencias` inline (mapa nombre → `DefinicionSecuencia`). El
`Programa` agrupa la raíz + los archivos externos cargados. El cargador
construye y valida el `Programa`; el motor lo recorre.

```
modelo  += DefinicionSecuencia { subsecuencias: HashMap<String, DefinicionSecuencia> }
         += Programa { raiz: DefinicionSecuencia, archivos: HashMap<String, DefinicionSecuencia> }
         += TipoPaso::SequenceCall, DefinicionPaso { secuencia, parametros }
         += ResultadoStep { sub_pasos: Option<Vec<ResultadoStep>> }
motor    += ejecuta_programa(&Programa)  +  ejecuta_secuencia_interna (recursiva, flag es_raiz)
         += EntornoMotor::desde_definicion_con_argumentos(def, args, parameters_mutables)
         +  escribe(Scope::Parameters) permitido si parameters_mutables
cargador += cargar_programa_de_archivo (resuelve nombre→inline / path→archivo, valida firma y lvalues, DFS de ciclos)
result_sink  → consola/json anidan; csv aplanea (sin columnas nuevas)
paso.proto / proto.rs → SIN cambios
```

## Pieza 1 — Modelo (`crates/modelo/src/lib.rs`)

### `DefinicionSecuencia` gana subsecuencias inline
```rust
pub struct DefinicionSecuencia {
    pub nombre: String,
    pub pasos_setup: Vec<DefinicionPaso>,
    pub pasos_main: Vec<DefinicionPaso>,
    pub pasos_cleanup: Vec<DefinicionPaso>,
    pub locals: HashMap<String, ValorDefinicion>,
    pub parameters: HashMap<String, ValorDefinicion>,
    pub file_globals: HashMap<String, ValorDefinicion>,
    /// Subsecuencias inline declaradas en el mismo archivo, invocables
    /// por nombre desde cualquier secuencia de ese archivo. Privadas del
    /// archivo: no se exponen a otros archivos. (M4b)
    pub subsecuencias: HashMap<String, DefinicionSecuencia>,
}
```
`Default` sigue derivado; `basica_datos.rs` y los tests con
`..Default::default()` no se rompen (`subsecuencias` vacío). No se añade
`salidas`: los `parameters` son el canal de entrada/salida.

### `TipoPaso` gana `SequenceCall`
```rust
pub enum TipoPaso {
    #[default] Grpc,
    Statement,
    SequenceCall,
}
```

### `Argumento` (lvalue del padre que se pasa por referencia)
```rust
/// Un argumento de un sequence call: mapea un Parameter de la subsecuencia
/// a una **variable local del padre** (`locals.X`). Es by-reference: al
/// iniciar, el motor copia `locals.X` → `parameters.P`; al volver, copia
/// `parameters.P` (final) → `locals.X`. `origen` debe ser una
/// `Expresion::Var{scope: Locals, campo}` (validado al cargar); el motor
/// la lee para la entrada y escribe en el mismo campo para la salida.
pub struct Argumento {
    pub param: String,          // nombre del Parameter de la subsecuencia
    pub origen: expr::Expresion, // lvalue: Var{Locals, campo}
}
```

### `DefinicionPaso` gana campos
```rust
pub secuencia: Option<String>,                  // nombre inline o path relativo (si SequenceCall)
pub parametros: Option<Vec<Argumento>>,         // by-reference: locals.X ↔ parameters.P
```
`nuevo`/`con_limite` los rellenan `None` (compat total).

### `ResultadoStep` gana sub-pasos anidados
```rust
pub sub_pasos: Option<Vec<ResultadoStep>>,      // None salvo en un sequence call
```
- `ResultadoStep::nuevo` inicializa `sub_pasos: None`; `medido`/`medido_valor`
  heredan el `None` vía `..ResultadoStep::nuevo(..)`.
- `ResultadoSecuencia::estado()` **no cambia**: el `ResultadoStep` del
  sequence call ya trae el `estado` agregado; `estado()` sigue mirando
  `p.estado`.
- `ResultadoSecuencia::reporte_a` ahora indenta sub-pasos (ver Pieza 4).
  El test `reporte_a_congela_el_formato` (sin sub-pasos) sigue pasando.
- **No se añaden salidas al `ResultadoStep`**: el retorno va por
  by-reference (copia a `locals` del padre), no por el resultado.

### `Programa` (nuevo)
```rust
pub struct Programa {
    pub raiz: DefinicionSecuencia,
    /// Archivos externos cargados, keyed por path normalizado. El valor es
    /// la secuencia raíz de ese archivo (su `nombre:`); las subsecuencias
    /// inline de cada archivo viven dentro de su `DefinicionSecuencia`.
    pub archivos: HashMap<String, DefinicionSecuencia>,
}
```
El motor lo consume; el cargador lo construye. `Programa` no sabe de YAML
ni de fs: es el resultado ya resuelto.

### Dependencias
`crates/modelo` ya depende de `expr` (desde M4-núcleo). **Sin nuevas
deps.** `paso.proto`/`proto.rs` **sin cambios**.

---

## Pieza 2 — Motor (`crates/motor/src/lib.rs` + `entorno.rs`)

### `EntornoMotor` gana `parameters_mutables`
```rust
pub struct EntornoMotor {
    locals: HashMap<String, Value>,
    parameters: HashMap<String, Value>,
    file_globals: HashMap<String, Value>,
    resultado: Option<ResultadoStep>,
    parameters_mutables: bool,   // true sólo en subsecuencias
}
pub fn desde_definicion_con_argumentos(
    def: &DefinicionSecuencia,
    argumentos: HashMap<String, Value>,
    parameters_mutables: bool,
) -> Self;
```
`desde_definicion` (existente, para la raíz) fija
`parameters_mutables = false`. Materializa `locals`/`file_globals` desde
`def`; `parameters` = `argumentos`.

### `EntornoMotor::escribe` relajado para subsecuencias
```rust
fn escribe(&mut self, scope: Scope, campo: &str, valor: Value) -> Result<(), ErrorExpr> {
    match scope {
        Scope::Locals => { self.locals.insert(..); Ok(()) }
        Scope::Parameters if self.parameters_mutables => {
            self.parameters.insert(..); Ok(())
        }
        // raíz: parameters no escribibles (no hay a quién devolver).
        // file_globals nunca; resultado nunca (el motor lo setea).
        _ => Err(ErrorExpr::entorno(.., "no se puede escribir en '{scope}.{campo}'")),
    }
}
```
- **Raíz:** escribir en `parameters` → error (como hoy). "Sólo se muta
  Locals" se mantiene para la raíz.
- **Subsecuencia:** escribir en `parameters` → ok (contrato de retorno).
  `locals` siempre escribible; `file_globals`/`resultado` nunca.
- `lee` sin cambios (`parameters` legible siempre, como hoy).

### Resolución de la subsecuencia (motor-side, a partir de datos ya cargados)
`ejecuta_secuencia_interna(def, sink, programa, profundidad, es_raiz)` recibe
la `DefinicionSecuencia` en curso. Para un paso `SequenceCall`:
- **Por nombre** (no es path): `def.subsecuencias.get(secuencia)` → la
  inline del archivo actual. (El cargador ya validó que existe.)
- **Por path**: `programa.archivos.get(path_normalizado)` → la raíz de
  ese archivo. (El cargador ya cargó el archivo y lo registró.)

El motor **no** abre ficheros: ambos caminos leen del `Programa` ya
construido. La normalización del path la hace el cargador; el motor usa el
string tal cual aparece en el `Programa` (clave canónica).

### API pública: `ejecuta_programa` + interna recursiva
```rust
pub fn ejecuta_programa(&mut self, programa: &Programa, sink: &mut impl ResultSink)
    -> Result<ResultadoSecuencia, Error>;
```
- `ejecuta_secuencia(&DefinicionSecuencia, sink)` se **preserva** para
  compat con `basica_datos.rs` y los tests sin subsecuencias — delega en
  `ejecuta_secuencia_interna` con `archivos` vacíos, profundidad 0 y
  `es_raiz = true`.
- `ejecuta_secuencia_interna(def, sink, programa, profundidad, es_raiz)`:
  - `EntornoMotor` con `parameters_mutables = !es_raiz` (raíz: false;
    subsecuencia: true). Los `parameters` de la raíz vienen de su
    declaración (defaults); los de la subsecuencia, de los argumentos.
  - `on_inicio_secuencia` **sólo si `es_raiz`**; `on_fin_secuencia` igual.
    Así los sinks de formato (que renderizan en `on_fin_secuencia`) no
    imprimen la subsecuencia por separado: sólo la raíz dispara el render.
    Los hooks de paso (`on_inicio_paso`/`on_resultado`/`on_fin_paso`) **sí**
    se disparan siempre, al sink real, para que un futuro sink de streaming
    vea también los pasos de la subsecuencia en vivo.
  - Recorre Setup/Main/Cleanup con `corre_un_paso`.
  - `profundidad + 1 > 64` → el paso sequence call se registra como
    `"error"` con `"anidamiento demasiado profundo (>64)"` (no panic).
  - Devuelve el `ResultadoSecuencia` **y** el `EntornoMotor` de la sub (el
    motor necesita leer `parameters` finales para la copia de vuelta).

### `corre_un_paso`: rama `SequenceCall`
Tras `disable` y `precondicion` (que **sí** aplican a un sequence call),
la rama nueva:
```rust
TipoPaso::SequenceCall => ejecuta_sequence_call(p, def_en_curso, ent, programa, sink, profundidad)?,
```
`ejecuta_sequence_call`:
1. **Resuelve la subsecuencia** (nombre → `def_en_curso.subsecuencias`;
   path → `programa.archivos`). Si faltara → `ResultadoStep` `"error"`
   "subsecuencia no resuelta" (defense in depth; el cargador ya validó).
2. **Profundidad**: si `profundidad + 1 > 64` → `"error"`.
3. **Entrada (by-reference):** para cada `Argumento { param, origen }`:
   - `origen` es `Expresion::Var{scope: Locals, campo}` (validado al
     cargar). `v = ent.lee(Locals, campo)` → `Value`. Si el `locals.campo`
     no existe en el padre → error de evaluación → el call se registra
     `"error"` con `"sequence call '{n}': argumento '{param}': locals.{campo} no existe"`;
     no se invoca la subsec.
   - Acumula `argumentos: HashMap<String, Value>` con `param → v`.
4. **Ejecuta la subsecuencia** con `ejecuta_secuencia_interna(sub,
   argumentos, programa, sink, profundidad+1, es_raiz=false)`. Como
   `es_raiz=false`: no dispara `on_inicio/on_fin_secuencia` (sin doble
   render), pero sí los hooks de paso (streaming futuro); y construye el
   `EntornoMotor` de la sub con `parameters_mutables=true`. Devuelve
   `(ResultadoSecuencia sub, EntornoMotor env_sub)`.
5. **Salida (by-reference):** para cada `Argumento { param, origen }`:
   - `v_final = env_sub.parameters.get(param)` (el valor final del
     Parameter tras la subsecuencia). Si el Parameter no está (defense in
     depth) → se omite (no se copia).
   - `ent.escribe(Locals, campo, v_final)` (escribe en el `locals.campo`
     del padre). `escribe(Locals)` siempre permitido. Si falla → el call
     se marca `"error"` con `"sequence call '{n}': salida '{param}': {e}"`.
6. **`asigna`** (si el paso lo declara, opcional): `ent.set_resultado(r_clone)`
   donde `r` es el `ResultadoStep` del call (ver paso 7); por cada
   `Asignacion`, `eval(expr)` → `ent.escribe(Locals, var, v)`. Útil para
   volcar `resultado.estado` del call a una Local del padre. La salida
   **de datos** ya se hizo por by-reference en el paso 5; `asigna` es
   sólo para el estado agregado. Falla → `"error"` como en M4-núcleo.
   `ent.limpia_resultado()`.
7. **Construye el `ResultadoStep` del padre**:
   ```rust
   ResultadoStep {
       nombre: p.nombre,
       estado: sub.estado(),         // agregado de la subsecuencia
       mensaje: format!("sequence call '{}' → {}", p.secuencia, sub.estado()),
       valor_medido: None,
       sub_pasos: Some(sub.pasos),   // anidamiento
       ..campos de límite None
   }
   ```
8. Devuelve el `ResultadoStep` para que `corre_un_paso` dispare
   `on_resultado`/`on_fin_paso` del padre.

> **Por qué flag `es_raiz` y no un sink mudo:** un sink mudo ejecutaría la
> subsecuencia sin disparar ningún hook, así un futuro sink de streaming
> no vería sus pasos. Con `es_raiz=false` los hooks de paso sí llegan al
> sink real (streaming en vivo ve la subsecuencia), y los sinks de
> formato (que sólo renderizan en `on_fin_secuencia`, que la sub no
> dispara) no duplican reporte. Es más limpio y futuro-compatible.

---

## Pieza 3 — Cargador (`crates/cargador/src/lib.rs`)

### `SecuenciaYaml` gana `subsecuencias`
```rust
#[serde(deny_unknown_fields)]
struct SecuenciaYaml {
    nombre: String,
    setup: Vec<PasoYaml>, main: Vec<PasoYaml>, cleanup: Vec<PasoYaml>,
    locals: HashMap<String, ValorYaml>,
    parameters: HashMap<String, ValorYaml>,
    file_globals: HashMap<String, ValorYaml>,
    subsecuencias: HashMap<String, SecuenciaYaml>,  // inline, por nombre (M4b)
}
```
`SecuenciaYaml` es ahora **recursivo** (un valor del mapa es otra
`SecuenciaYaml`). noyalib lo soporta. `subsecuencias` es opcional (default
vacío). No se añade `salidas`.

### `PasoYaml` += `secuencia: Option<String>`, `parametros: Option<HashMap<String,String>>` (texto→AST de un lvalue). **Mantiene `deny_unknown_fields`.**

### `PasoYaml::a_definicion`
- `tipo: "sequence_call"` → `TipoPaso::SequenceCall`.
- Parsea `parametros` (texto→AST con `expr::parse_expresion` + `extraer_expr`)
  → `Vec<Argumento>`. **Valida que cada expr sea `Expresion::Var{scope:
  Locals, campo}`** (lvalue local puro); si no → error:
  `"el argumento '{param}' del sequence call '{n}' debe ser una variable local (locals.X); by-reference no admite expresiones"`.
- Cross-field (fail-fast):
  - `SequenceCall` **sin** `secuencia` → error.
  - `SequenceCall` con `statement` → error (reservado para `statement`).
  - `SequenceCall` con `limite` → error ("un sequence call no mide").
  - `SequenceCall` con `reintentos > 1` → error ("un sequence call no
    admite reintentos; sus pasos internos ya declaran los suyos").
  - `Grpc`/`Statement` con `secuencia` o `parametros` → error (reservado
    para `sequence_call`).

### `cargar_programa_de_archivo(ruta) -> Result<Programa, ErrorCarga>` (nuevo)
Resolución recursiva (inline por nombre + externa por path):
- `cargar_de_archivo(ruta)` se **preserva** (devuelve la
  `DefinicionSecuencia` raíz de un archivo, sin resolver calls — útil para
  tests y `basica_datos`). Internamente parsea `subsecuencias`.
- `cargar_programa_de_archivo`:
  1. Carga la raíz (parsea su YAML, sus `subsecuencias` inline, sus pasos).
  2. Construye el `Programa` con `raiz` y un mapa `archivos` vacío.
  3. **DFS de resolución**: para cada secuencia cargable (la raíz y cada
     archivo externo que aparezca), recorre los pasos Setup/Main/Cleanup y
     los de **todas** sus `subsecuencias` inline. Por cada `SequenceCall`:
     - **Por nombre** → busca en `subsecuencias` de la secuencia que
       contiene el paso. Si no existe → error con el nombre del paso y
       el nombre buscado.
     - **Por path** → resuelve relativo al directorio del archivo que
       contiene el paso (`Path::parent` + join), normaliza el path (clave
       canónica). Si ya está en `archivos`, reutiliza (DAG: un mismo
       archivo puede referenciarse desde varios padres). Si no, lo carga
       (parsea su YAML + sus `subsecuencias` inline) y lo registra.
  4. **Validación de lvalues:** cada `Argumento.origen` (`Var{Locals,
     campo}`) debe referenciar un `locals.campo` **declarado** en la
     secuencia que contiene el paso (el padre). Si no → error con el
     nombre del paso, el parámetro y el local faltante. (El motor
     también lo rechazaría en runtime; fail-fast al cargar es mejor.)
  5. **Validación de firma:** al enlazar un `SequenceCall`, las claves de
     `parametros` del paso deben ser **exactamente** las de `parameters`
     de la subsecuencia (ni más ni menos). Sobran/faltan → error con el
     nombre del paso y el destino. (El tipo declarado es orientativo; el
     chequeo de tipo real es del expression engine en runtime.)
  6. **Detección de ciclos:** DFS sobre el **grafo de llamadas** (nodos =
     `(archivo, secuencia)` — donde `secuencia` es "raíz" o un nombre
     inline; aristas = cada `SequenceCall`, por nombre o por path). Si al
     visitar un nodo ya está en el camino en curso →
     `ErrorCarga::Validacion("ciclo de subsecuencias: A → B → A")`.
     Reutilizar una subsecuencia ya *completada* en otra rama **no** es ciclo.
  7. Devuelve el `Programa`.

### Función auxiliar: `es_path(secuencia: &str) -> bool`
`true` si contiene `/` o `\`, o termina en `.yaml`/`.yml`. Decide nombre
vs path. Se usa en `a_definicion` (sólo para validar coherencia) y en la
resolución del cargador/motor.

### Tests afectados (rotura esperada, trivial)
- Los tests que construyen `DefinicionSecuencia`/`DefinicionPaso` como
  literal → ya usan `..Default::default()`/`nuevo`; los nuevos campos son
  `None`/vacío por defecto → sin rotura.
- `campo_desconocido_es_error`: revisar que el campo raro que usa no sea
  uno ahora válido (`secuencia`/`parametros`/`subsecuencias`); usar `foo: bar`.
- Nuevos tests:
  - Subsecuencia **inline** invocada por nombre → `Programa` correcto.
  - Subsecuencia **externa** invocada por path → archivo cargado en `archivos`.
  - Inline + externa en el mismo archivo, ambas invocadas.
  - Path relativo a subdirectorio.
  - Path no encontrado → error; nombre no definido → error.
  - Ciclo por path (A→B→A) → error; ciclo por nombre (inline A→B→A) → error.
  - Firma con clave de más/menos → error.
  - Argumento que no es `locals.X` (es una expresión o `file_globals.X`)
    → error; `locals.X` no declarado en el padre → error.
  - `deny_unknown_fields` sigue rechazando campos raros en paso y en
    `subsecuencias` (una inline con campo raro → error).
  - `reintentos>1`/`limite` en sequence call → error.

---

## Pieza 4 — Reporte y sinks

### `ResultadoSecuencia::reporte_a` (consola, RNF-08)
Refactor a función recursiva `escribe_paso(w, p, nivel)`:
```
=== nombre: estado ===            # nivel 0
  [estado] nombre: mensaje         # nivel 0 (2 espacios)
    [estado] nombre: mensaje        # nivel 1 (4 espacios, sub-paso)
```
- Pasos sin `sub_pasos` → idéntico a hoy (el test congelado no cambia).
- Sub-pasos indentados +2 espacios por nivel.
- Nuevo test `reporte_anida_sub_pasos` congela:
  ```
  === basica: fallo ===
    [fallo] test_fuentes: sequence call './medir_fuentes.yaml' → fallo
      [paso] medir_canal_1: ok
      [fallo] medir_canal_2: fuera de rango
      [paso] desconectar: ok
  ```

### `SinkJson`
`paso_a_json` anida `"sub_pasos": [...]` (array de objetos, recursivo) si
`sub_pasos.is_some()`. Test nuevo verifica el árbol anidado.

### `SinkCsv`
**Sin columnas nuevas** (la cabecera congelada no cambia). Cuando un paso
tiene `sub_pasos`, se emite la fila del call (estado agregado,
`nombre_paso = test_fuentes`) y a continuación una fila por sub-paso con
`nombre_paso = test_fuentes/medir_canal_1` (prefijo `padre/hijo`,
recursivo). El test `cabecera_y_una_fila_por_paso` (sin sub-pasos) sigue
pasando; test nuevo congela el aplanado.

---

## Pieza 5 — Ejemplos

- `ejemplos/subsecuencia.yaml` (padre): demuestra **ambas** rutas y el
  cableo by-reference de ida/vuelta.
  ```yaml
  nombre: basica
  locals: { canal_in: 0.5, ok_init: false, estado_fuentes: "" }
  subsecuencias:
    init_comun:                      # inline, por nombre, privada de este archivo
      parameters: { canal: 0.0, lista_ok: false }
      main:
        - nombre: preparar_canal
          tipo: statement
          statement: 'parameters.lista_ok = (parameters.canal >= 0.0)'
  main:
    - nombre: preparar
      tipo: sequence_call
      secuencia: init_comun              # nombre → inline
      parametros: { canal: locals.canal_in, lista_ok: locals.ok_init }
      # al volver: locals.ok_init = parameters.lista_ok (final)
    - nombre: test_fuentes
      tipo: sequence_call
      secuencia: ./medir_fuentes.yaml    # path → archivo externo
      parametros: { canal: locals.canal_in }
      # al volver: locals.canal_in = parameters.canal (final; la subsecuencia
      # podría haberlo modificado)
  ```
- `ejemplos/medir_fuentes.yaml` (hija, archivo externo público):
  ```yaml
  nombre: medir_fuentes
  parameters: { canal: 0.0 }
  main:
    - nombre: medir_canal
      tipo: grpc
      reintentos: 1
      limite: { tipo: rango, min: 4.5, max: 5.5 }
  cleanup:
    - nombre: desconectar
      tipo: grpc
      reintentos: 1
  ```
  Documenta el resultado esperado: estado agregado propagado, anidamiento
  en reporte/JSON/CSV, y `locals.ok_init`/`locals.canal_in` actualizados
  por by-reference tras los calls.

---

## Pieza 6 — Docs y ADR
- **Nuevo ADR-0010**: *Sequence call: el motor orquesta subsecuencias
  declaradas inline (por nombre) o en archivo aparte (por path); el
  cargador resuelve, valida firma/lvalues y detecta ciclos; `paso.proto`
  no cambia; Parameters entrada/salida by-reference (como TestStand),
  relajando "sólo se muta Locals" de forma acotada (sólo subsecuencias;
  el paso gRPC sigue aislado); el retorno va por copia a `locals` del
  padre, no por el resultado.* Relaciona ADR-0005/0008/0009,
  `contrato-grpc.md`, `variables-y-alcances.md`, `modelo-de-pasos.md`.
- `diseno/modelo-de-pasos.md`: sequence call "aplazado a M4" → "Implementado
  en M4b (inline y por path; Parameters entrada/salida by-reference)".
- `diseno/variables-y-alcances.md`: Parameters "al invocar via sequence
  call" → "Implementado en M4b (entrada/salida by-reference a `locals` del
  padre, como TestStand)"; documentar la relajación acotada de "sólo se
  muta Locals" y que by-value y by-reference transitivo quedan post-MVP.
- `diseno/formato-de-secuencia.md`: documentar `tipo: sequence_call`,
  `secuencia` (nombre o path), `parametros` (`locals.X` by-reference),
  `subsecuencias:` a nivel de archivo; anotar recortes (sin
  reintentos/limite; argumentos sólo `locals.X`) y la convención
  nombre-vs-path.
- `diseno/motor-de-ejecucion.md`: anotar sequence call (motor-side,
  anidamiento, resolución nombre/path, profundidad máxima, flag `es_raiz`,
  by-reference ida/vuelta).
- `diseno/reportes.md`: anotar anidamiento en consola/JSON y aplanado en CSV.
- `requisitos.md` y `roadmap.md`: marcar RF-27 (sequence call) y RF-31
  (Parameters entrada/salida) como implementados en M4b.

---

## Orden de implementación (minimiza rotura, cada paso compila y testea solo)
1. `modelo`: `DefinicionSecuencia` + `subsecuencias`, `TipoPaso::SequenceCall`,
   `Argumento`, `DefinicionPaso` + `secuencia`/`parametros`, `ResultadoStep`
   + `sub_pasos`, `Programa`. Tests: defaults sin rotura, `estado()` sin
   cambios, reporte anidado congelado.
2. `motor/entorno.rs`: flag `parameters_mutables`,
   `desde_definicion_con_argumentos`, `escribe(Scope::Parameters)` relajado.
   Tests puros (escribir parameters en subsecuencia ok; en raíz error).
3. `motor/lib.rs`: `ejecuta_secuencia_interna` (con flag `es_raiz`),
   `ejecuta_programa`, `ejecuta_sequence_call` (resolución nombre/path,
   entrada by-reference, salida by-reference, `asigna` para estado),
   profundidad. `ejecuta_secuencia` existente delega. Tests con un
   `Programa` construido a mano y subsecuencias de `statement` (sin red).
4. `cargador`: `SecuenciaYaml` + `subsecuencias`, `PasoYaml` + campos,
   `a_definicion` con `sequence_call` (validar lvalue `locals.X`),
   `es_path`, `cargar_programa_de_archivo` (resolución inline/externa,
   validación de lvalues, firma, ciclos). Tests.
5. `result_sink`: `reporte_a` recursivo, `paso_a_json` anidado, CSV
   aplanado. Tests congelados.
6. `anvil.rs`: `cargar_programa_de_archivo` + `ejecuta_programa`.
7. `ejemplos/subsecuencia.yaml` + `medir_fuentes.yaml`.
8. Docs + ADR-0010 + requisitos/roadmap.

## Verificación end-to-end
- `cargo test -p modelo` — `sub_pasos` default `None`; `subsecuencias`
  default vacío; `estado()` sin cambios; reporte anidado congelado;
  `Programa` construcción.
- `cargo test -p motor` — `ejecuta_sequence_call` con subsecuencia de
  `statement` (sin gRPC) inline y externa: estado agregado propagado al
  `ResultadoStep` padre; entrada by-reference (`locals.X`→`parameters.P`);
  la subsecuencia escribe en `parameters.P` (relajado); salida
  by-reference (`parameters.P`→`locals.X` del padre, verificable leyendo
  `locals` del padre tras el call); error de argumento (local inexistente)
  → `"error"`; escribir `parameters` desde la raíz → error; profundidad
  >64 → `"error"`; flag `es_raiz` no dispara `on_fin_secuencia` en la sub.
- `cargo test -p cargador` — carga inline + externa; path relativo a
  subdirectorio; path no encontrado → error; nombre no definido → error;
  ciclo por path y por nombre → error; firma con clave de más/menos →
  error; argumento no `locals.X` (expresión/`file_globals.X`) → error;
  `locals.X` no declarado en el padre → error; `deny_unknown_fields` en
  paso y en `subsecuencias` inline; `reintentos>1`/`limite` en sequence
  call → error.
- `cargo test -p result_sink` — consola anidada congelada; JSON anidado;
  CSV aplanado sin columnas nuevas; tests congelados previos intactos.
- `cargo build --target wasm32-wasip2` — sin deps nuevas (ADR-0001).
- Smoke manual (ejecutor en `127.0.0.1:9100`):
  ```
  wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
    target/wasm32-wasip2/debug/anvil.wasm ejemplos/subsecuencia.yaml \
    --json /tmp/out.json --csv /tmp/out.csv
  ```
  Verificar reporte con `preparar` y `test_fuentes` anidados, JSON con
  `sub_pasos`, CSV con filas `test_fuentes/medir_canal`.

## Fuera de M4b (post-MVP)
- **By-value explícito**: argumentos de sólo-entrada (sin retorno) para
  aislar, como TestStand by-value.
- **By-reference transitivo**: pasar `parameters.X`/`file_globals.X` del
  padre como lvalue del argumento (no sólo `locals.X`).
- **Expresiones/literales como argumento de entrada** (con by-value).
- Sequence call con `reintentos` (re-correr la subsecuencia entera).
- Detección de firma por tipo (argumento numérico a un parameter textual
  → error al cargar, no en runtime).
- Valores estructurados (records/listas) en Parameters, no sólo escalares.
- Subsecuencias inline anidadas más allá de un nivel (el modelo lo
  permite de forma natural; se podría acotar en la validación si complica).
- StationGlobals; paso `step` interactivo; introspección de firma para el
  editor visual.