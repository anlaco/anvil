# Plan: M4-núcleo — Variables, control de flujo y expresiones

> **Alcance acordado:** M4-núcleo. NO incluye sequence call / subsecuencias (M4b),
> NO toca `paso.proto` (patrón ADR-0008). `step` mode se aplaza (WASI P2 sin espera fiable).

## Requisitos cubiertos
- **RF-35** Expression engine (sintaxis Python/MATLAB-like, AST acotado, sin deps externas — ADR-0001).
- **RF-31** Variables/scopes Locals/Parameters/FileGlobals, **motor-side** (el cableo al paso por el wire es post-MVP, ver `variables-y-alcances.md`).
- **RF-33** Precondición por step (motor evalúa antes de invocar; si falsa, se salta sin gastar intento).
- **RF-34** Control de flujo: `disable` y `pause_on_fail`. (`step` → post-MVP.)
- **RF-27 (parcial)** Statement: tipo de paso LOCAL (no gRPC) que ejecuta sentencias en el motor.

## Principios rectores
- ADR-0005: el motor es genérico, no conoce el dominio. El engine opera sobre `Value` y un trait `Entorno`; el motor implementa `Entorno`.
- ADR-0008 (precedente M3): el motor evalúa reglas declaradas como datos contra el resultado/estado del paso, **sin tocar el contrato gRPC**. M4 extiende este patrón de límites → expresiones/precondiciones/asignaciones.
- Fail-fast al cargar (consistente con `deny_unknown_fields` y la validación de límites en M3): las expresiones se **parsean a AST al cargar**, no en runtime.

---

## Arquitectura

Nuevo **crate `expr`** (lexer + AST + parser + evaluator). `modelo` guarda los ASTs (dependencia de tipos, no de lógica); `cargador` parsea al cargar; `motor` evalúa en runtime implementando el trait `Entorno`.

```
expr ── (ninguna dep externa; sólo alloc) ── compila a wasm32-wasip2
modelo  → depende de expr (tipos Expresion/Sentencia)
cargador → depende de modelo + expr (parsea texto→AST al cargar)
motor    → depende de modelo + expr (evalúa AST contra EntornoMotor)
result_sink, pasos_demo, ejecutor_pasos → SIN cambios
paso.proto / proto.rs → SIN cambios
```

## Pieza 1 — Crate `expr` (nuevo)

**`crates/expr/src/`**: `lib.rs`, `value.rs`, `ast.rs`, `lexer.rs`, `parser.rs`, `eval.rs`, `error.rs`. Tests de integración en `crates/expr/tests/`.

### `Value` (value.rs)
```rust
pub enum Value { Numero(f64), Bool(bool), Texto(String), Nulo }
```
Reglas de tipo estrictas (sin coerción silenciosa, sin truthiness implícita):
- `+ - * /` y unario `-`: sólo `Numero`→`Numero`. Mezcla de tipos → `ErrorExpr::Tipo`. Div/0 → `ErrorExpr::Evaluacion`.
- `== !=`: mismo tipo compara; tipos distintos → `false`/`true` (no error). `Nulo == Nulo` → `true`.
- `< > <= >=`: sólo `(Numero, Numero)`→`Bool`. Mezcla → error de tipo.
- `and or`: sólo `(Bool, Bool)`→`Bool`, con **cortocircuito** (`a and b` no evalúa `b` si `a==false`).
- `not`: `Bool`→`Bool`.
- `Nulo` en aritmética/comparación de orden → `ErrorExpr::Evaluacion("… es nulo")`. Esto fuerza precondiciones explícitas (`resultado.valor_medido != nulo`).

### `ast.rs`
Separar `Expresion` (produce `Value`, no muta) de `Sentencia` (muta Locals, no devuelve valor):
```rust
pub enum Expresion { Lit(Value), Var{scope: Scope, campo: String},
                     BinOp{op: BinOp, izq: Box<Expresion>, der: Box<Expresion>},
                     UnOp{op: UnOp, operando: Box<Expresion>} }
pub enum Sentencia { Assign{scope: Scope, campo: String, valor: Expresion} }
pub enum Scope { Locals, Parameters, FileGlobals, Resultado }
pub enum BinOp { Add,Sub,Mul,Div, Eq,Ne,Lt,Le,Gt,Ge, And,Or }
pub enum UnOp { Neg, Not }
```
Post-MVP (no ahora): `Call` (abs/min/max), `Index`, `StrInterp`, `Scope::ResultadosPrevios`.

### `lexer.rs`
Tokens con `Span` (offset+longitud) para errores posicionales. Palabras clave reservadas: `and or not nulo true false` + scopes `locals parameters file_globals resultado`. (No se pueden usar como nombres de campo.)

### `parser.rs` — recursive descent a mano (sin nom/lalrpop, ADR-0001)
Precedencia (menor→mayor): `or` < `and` < `not`(prefijo) < comparaciones **no asociativas** (`a<b<c` → error) < `+ -` < `* /` < unario `-` < `.`(campo) < átomo.
```rust
pub fn parse_expresion(src: &str) -> Result<Expresion, ErrorExpr>;   // precondición
pub fn parse_sentencias(src: &str) -> Result<Vec<Sentencia>, ErrorExpr>; // asigna / statement
```
`asigna` admite dos formas equivalentes: `voltaje_leido: resultado.valor_medido` y `voltaje_leido: "${resultado.valor_medido}"` (interpolación sólo si **toda** la cadena es `${…}`; parcial → error, post-MVP).

### `eval.rs`
```rust
pub trait Entorno {
    fn lee(&self, scope: Scope, campo: &str) -> Result<Value, ErrorExpr>;
    fn escribe(&mut self, scope: Scope, campo: &str, valor: Value) -> Result<(), ErrorExpr>;
}
pub fn eval(expr: &Expresion, env: &impl Entorno) -> Result<Value, ErrorExpr>;            // sólo lectura
pub fn eval_sentencias(stmts: &[Sentencia], env: &mut impl Entorno) -> Result<(), ErrorExpr>; // escritura
```
**Decisión:** trait `Entorno` con `(Scope, campo)` (type-safe para el MVP). `eval` toma `&impl Entorno`; `eval_sentencias` toma `&mut impl Entorno` (refuerza que las precondiciones no mutan). El motor rechaza `escribe` en `scope != Locals` → `ErrorExpr::Entorno` (regla "sólo se muta Locals" hecha valer en runtime).

### `error.rs`
```rust
pub struct ErrorExpr { pub kind: ErrorKind, pub pos: usize, pub len: usize, pub mensaje: String }
pub enum ErrorKind { Sintaxis, Lexico, Tipo, Evaluacion, Entorno }
```
`Display` con caret. **El engine nunca pánico** — toda API devuelve `Result`.

---

## Pieza 2 — Modelo (`crates/modelo/src/lib.rs`)

### `DefinicionSecuencia` gana scopes
```rust
pub enum ValorDefinicion { Numero(f64), Texto(String), Bool(bool) }  // literal declarado en YAML
pub struct DefinicionSecuencia {
    nombre: String,
    pasos_setup: Vec<DefinicionPaso>, pasos_main: Vec<DefinicionPaso>, pasos_cleanup: Vec<DefinicionPaso>,
    pub locals: HashMap<String, ValorDefinicion>,
    pub parameters: HashMap<String, ValorDefinicion>,   // M4-núcleo: vacío (sin sequence call)
    pub file_globals: HashMap<String, ValorDefinicion>,
}
```
Derivar `Default` para `DefinicionSecuencia` y usar `..Default::default()` en los tests que la construyen literalmente.

### `DefinicionPaso` gana campos (struct plano, NO enum de variantes — preserva ~59 tests)
```rust
pub enum TipoPaso { Grpc, Statement }  // Default = Grpc
pub struct Asignacion { pub var: String, pub expr: expr::Expresion }
pub struct DefinicionPaso {
    nombre: String, reintentos: u32, limite: Option<Limite>,
    pub disable: bool,                                          // RF-34, default false
    pub pause_on_fail: bool,                                    // RF-34, default false
    pub precondicion: Option<expr::Expresion>,                  // RF-33
    pub asigna: Option<Vec<Asignacion>>,                        // RF-31, tras paso Grpc
    pub tipo: TipoPaso,                                          // RF-27, default Grpc
    pub statement: Option<Vec<expr::Sentencia>>,                // RF-27, si tipo==Statement
}
```
`nuevo`/`con_limite` rellenan defaults (compat total con `basica_datos.rs` y tests existentes).

### Estado "saltado" (decisión)
- El paso saltado (por `disable` o precondición falsa) **sí** se registra en `ResultadoSecuencia.pasos` con `estado = "saltado"` y mensaje `"disable"` / `"precondición falsa"`. Sin campos nuevos en `ResultadoStep`.
- `estado()` **no** cuenta saltado ni como fallo ni error: secuencia con sólo saltados → `"paso"`.
- RNF-08: `"saltado"` es un nuevo **valor** de `estado`, no cambio de formato. El test `reporte_a_congela_el_formato` (usa `paso`/`fallo`) sigue pasando. Se añade test nuevo que congela la línea `  [saltado] X: disable` y se documenta la extensión en `reportes.md`.
- Error de expresión (precondición mal evaluada, asigna que falla, statement que falla) → `estado = "error"` con `mensaje = "precondición: {e}"` (sin campos nuevos; `paso.proto` intacto). Cuenta como `error` en el agregado.

### Dependencias
`crates/modelo/Cargo.toml` += `expr = { workspace = true }`. Es la primera vez que `modelo` depende de lógica; justificado: guarda ASTs parseados. `paso.proto`/`proto.rs` **sin cambios**.

---

## Pieza 3 — Motor (`crates/motor/src/lib.rs` + nuevo `entorno.rs`)

### `crates/motor/src/entorno.rs` (nuevo)
```rust
pub struct EntornoMotor {
    locals: HashMap<String, Value>, parameters: HashMap<String, Value>,
    file_globals: HashMap<String, Value>, resultado: Option<ResultadoStep>,
}
impl EntornoMotor {
    pub fn desde_definicion(def: &DefinicionSecuencia) -> Self;  // materializa ValorDefinicion→Value
    pub fn set_resultado(&mut self, r: ResultadoStep);
    pub fn limpia_resultado(&mut self);
}
impl expr::Entorno for EntornoMotor { /* lee/escribe con la política de scopes */ }
```
- `lee("resultado","valor_medido")` → `Numero` o `Nulo` (laxa: campo ausente → Nulo).
- `lee("locals"/"parameters"/"file_globals", k)` → estricta: campo no declarado → `ErrorExpr::Entorno` (fail-fast, consistente con la validación al cargar).
- `escribe`: sólo `Locals`; `parameters`/`file_globals` → `ErrorExpr::Entorno`.

### Flujo modificado de `ejecuta_secuencia`
Factorizar `corre_un_paso(p, &mut entorno, sink)` compartido por Setup/Main/Cleanup. Pseudocódigo por paso:
1. **`disable`** → `ResultadoStep::nuevo(n, "saltado", "disable")`; `on_resultado`; `on_fin_paso`; return. (No gasta intento.)
2. **`precondicion`** (si `Some`): `entorno.limpia_resultado()`; `eval(pre, &entorno)`.
   - `Ok(Bool(true))` → continúa.
   - `Ok(Bool(false))` → `"saltado"`, mensaje `"precondición falsa"`; return (no gasta intento).
   - `Ok(no-Bool)` → `"error"`, `"precondición: se esperaba bool"`; return. **(Bool estricto, sin truthiness.)**
   - `Err(e)` → `"error"`, `"precondición: {e}"`; return.
3. **Según `tipo`**:
   - `Statement` → `eval_sentencias(&statement, &mut entorno)`; `Ok`→`"paso"` ("statement ok"), `Err`→`"error"`. **No invoca gRPC.**
   - `Grpc` → flujo M3 actual: `ejecuta_con_reintentos(p)` (con `aplicar_limite` dentro).
4. **`asigna`** (sólo Grpc, si `Some`): `entorno.set_resultado(r.clone())`; por cada `Asignacion`, `eval(expr)` → `entorno.escribe(Locals, var, v)`. Si `Err` → `r.mensaje += " (asigna {var}: {e})"` y **estado→"error"** (fallo de definición). `entorno.limpia_resultado()`.
5. **`pause_on_fail`**: si `p.pause_on_fail && es_fallo(r)` → tras registrar, `break` del bucle. En Main refuerza el corte en primer fallo (Main ya corta); en Setup/Cleanup **sí** corta el bucle (por defecto no cortan) y, si era Setup, `setup_ok=false`.
6. `on_resultado(&r)`; `on_fin_paso(p)`.

**Trazabilidad del sink:** todo paso (incluido saltado) dispara `on_inicio_paso`→`on_resultado`→`on_fin_paso`. Los sinks de formato renderizan en `on_fin_secuencia` desde el `ResultadoSecuencia` ya agregado (incluye saltados) → aparecen en consola/JSON/CSV sin tocar `result_sink`.

---

## Pieza 4 — Cargador (`crates/cargador/src/lib.rs`)

### `SecuenciaYaml` += `locals`/`parameters`/`file_globals` (`Option`/default `HashMap`)
### `PasoYaml` += `disable: bool`, `pause_on_fail: bool`, `precondicion: Option<String>`, `asigna: Option<HashMap<String,String>>`, `tipo: String` (default `"grpc"`), `statement: Option<String>`. **Mantiene `deny_unknown_fields`.**
### `ValorYaml` (`#[serde(untagged)]`: `Bool` antes que `Numero` antes que `Texto`) → `ValorDefinicion`.
### `PasoYaml::a_definicion` (ya es `Result`) ahora:
- Parsea `precondicion` con `expr::parse_expresion` → `ErrorCarga::Validacion("precondición del paso '{n}': {e}")`.
- Parsea cada `asigna` value con `expr::parse_expresion` → `Asignacion{var, expr}`.
- Parsea `statement` con `expr::parse_sentencias` → `Vec<Sentencia>`.
- Valida `tipo`: `"grpc"`/`"statement"` (otro → error). Cross-field: `Statement` sin `statement` → error; `Grpc` con `statement` → error.

### Tests afectados (rotura esperada, trivial)
- `campo_desconocido_es_error` (l.406): hoy usa `disable: true` que **ya será válido** → cambiar a un campo raro (`foo: bar`) y actualizar comentario.
- `la_traduccion_coincide_con_el_ejemplo_en_codigo` (l.330): construye `DefinicionSecuencia` como literal → añadir `..Default::default()` o los 3 mapas vacíos.
- Resto de tests y ejemplos (`basica.yaml`, `limites.yaml`) siguen pasando (defaults).

---

## Pieza 5 — Ejemplo `ejemplos/variables.yaml` (nuevo)
Ejercita: `file_globals` (texto + número), `locals` (numérico + bool), `parameters: {}`, un **statement** (`tipo: statement`, `statement: 'locals.ok = false'`), un paso Grpc con `precondicion` + `limite` + `asigna` (vuelca `resultado.valor_medido` y `resultado.estado == "paso"`), un paso `disable: true`, y un paso `pause_on_fail: true`. Documenta el resultado esperado (estado agregado `fallo`, locals finales, líneas de reporte).

---

## Pieza 6 — Docs y ADR
- **Nuevo ADR-0009**: *Las expresiones, precondiciones y asignaciones las evalúa el motor contra su entorno, no el paso* (extiende ADR-0008; registra que el `Entorno` vive en el motor en MVP y el cableo al wire es post-MVP). Relaciona ADR-0005/0008, `contrato-grpc.md`, `variables-y-alcances.md`, `motor-de-expresiones.md`.
- `docs/diseno/motor-de-expresiones.md`: "Propuesta (no implementado)" → "Implementado en M4-núcleo (subconjunto MVP)".
- `docs/diseno/variables-y-alcances.md`: idem; marcar sequence call/StationGlobals post-MVP.
- `docs/diseno/motor-de-ejecucion.md`: "Control de flujo (pendiente)" → "Implementado en M4-núcleo (disable, pause_on_fail, precondición, statement)"; marcar `step` como post-MVP (sin espera fiable en WASI P2).
- `docs/diseno/formato-de-secuencia.md`: documentar `locals`/`parameters`/`file_globals`/`disable`/`pause_on_fail`/`precondicion`/`asigna`/`tipo`/`statement` en el schema.
- `docs/diseno/reportes.md`: anotar que M4 añade el estado `"saltado"` (extensión aditiva de RNF-08).
- `docs/requisitos.md` y `docs/roadmap.md`: marcar RF-31/33/34/35(parcial 27) como implementados en M4-núcleo; señalar que sequence call queda para M4b.

---

## Orden de implementación (minimiza rotura, cada paso compila y testea solo)
1. Crate `expr` (esqueleto → `value`+`error` → `lexer` → `ast` → `parser` → `eval` con `EntornoMock`). Tests puros sin gRPC. Verifica que compila a `wasm32-wasip2` sin deps externas.
2. `Cargo.toml` workspace: añadir `expr` a members y `[workspace.dependencies]`.
3. `modelo`: `ValorDefinicion`, `TipoPaso`, `Asignacion`, ampliar `DefinicionPaso`/`DefinicionSecuencia` (+`Default`), ampliar `estado()` (saltado neutral). Test que congela línea saltada.
4. `cargador`: `ValorYaml`, ampliar `SecuenciaYaml`/`PasoYaml`, `a_definicion` con parseo `expr`, materializar mapas. Arreglar los 2 tests rotos. Nuevos tests de carga.
5. `motor/entorno.rs`: `EntornoMotor` + impl `Entorno` + `ValorDefinicion::a_value`.
6. `motor/lib.rs`: `mod entorno`; factorizar `corre_un_paso`, `ejecuta_statement`, precondición, asigna, `pause_on_fail`. Tests con entorno construido a mano (sin gRPC).
7. `ejemplos/variables.yaml`.
8. Docs + ADR-0009 + requisitos/roadmap.

## Verificación end-to-end
- `cargo test -p expr` — parser (precedencia, no-asociatividad de comparaciones, `and/or/not`, scopes, asignación, errores posicionales) y evaluator (`EntornoMock`: aritmética, div/0, `Nulo` en aritmética→error, cortocircuito, tipos, escritura sólo Locals).
- `cargo test -p modelo` — `estado()` con saltado neutral; reporte congelado + nueva línea saltado; constructores con defaults.
- `cargo test -p cargador` — carga de `variables.yaml`, precondición/asigna/statement parseados, errores de sintaxis→`ErrorCarga::Validacion` con nombre de paso, cross-field statement/grpc, `deny_unknown_fields` sigue rechazando campos raros.
- `cargo test -p motor` — `EntornoMotor` pura: precondición falsa→saltado, precondición no-bool→error, statement local, asigna vuelca Locals, asigna a parameters→error, `disable`→saltado, `pause_on_fail` corta Setup.
- `cargo build --target wasm32-wasip2 -p expr` — confirma sin deps externas (ADR-0001).
- Smoke manual (requiere `wasi-grpc` y `ejecutor_pasos` corriendo en `127.0.0.1:9100`):
  ```
  wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
    target/wasm32-wasip2/debug/anvil.wasm ejemplos/variables.yaml --json /tmp/out.json
  ```
  Verificar reporte textual con `  [saltado] paso_obsoleto: disable`, JSON/CSV con `estado="saltado"`, y que `locals.voltaje_leido` quedó volcada (inspeccionar vía un statement de diagnóstico `locals.voltaje_leido` si se quiere; el reporte no muestra locals hoy — opcional post-MVP).

## Fuera de M4-núcleo (post-MVP / M4b)
- Sequence call / subsecuencias (M4b) → `parameters` queda vacío y reservado.
- Cableo de variables al paso por `paso.proto` (parámetros tipados en el wire).
- `step` mode interactivo (necesita modelo de espera en WASI P2 o UI).
- StationGlobals; árbol de propiedades recursivo tipado de TestStand.
- Funciones `abs/min/max`; interpolación parcial de strings; listas/records; acceso a resultados de pasos anteriores por nombre; comparaciones encadenadas (`a<b<c`); concatenación de strings con `+`.