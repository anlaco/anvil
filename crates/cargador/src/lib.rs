//! Cargador de secuencias desde YAML: lee un fichero de secuencia y lo
//! traduce a `modelo::DefinicionSecuencia`. El motor no cambia (ADR-0005):
//! aquí sólo producimos los datos que el motor ya sabe recorrer.
//!
//! El schema es un **subconjunto estricto** con `deny_unknown_fields`:
//! `nombre`, `reintentos`, `limite` (M3) y las tres secciones (`setup`, `main`,
//! `cleanup`); desde M4 también `locals`/`parameters`/`file_globals` a nivel
//! de secuencia y `disable`/`pause_on_fail`/`precondicion`/`asigna`/`tipo`/
//! `statement` por paso. Cualquier otro campo sigue rechazándose al cargar
//! (fail-fast) para que el schema crezca de forma deliberada.
//!
//! ## Expresiones parseadas al cargar (M4, RF-33/35)
//!
//! `precondicion`, `asigna` y `statement` son texto en el YAML que el
//! cargador **parsea a AST** aquí mismo, igual que ya validaba los límites en
//! M3 (fail-fast). Un error de sintaxis se reporta como `ErrorCarga::Validacion`
//! con el nombre del paso, no se descubre a mitad de una corrida.
//!
//! ## Property loader (M3, RF-30)
//!
//! Los límites pueden venir embebidos en la secuencia (`limite` por paso) o,
//! opcionalmente, desde un **fichero sidecar** (p. ej. `basica.limits.yaml`)
//! que el cargador inyecta por nombre de paso antes de ejecutar. El sidecar
//! **manda** sobre el límite embebido: es el mecanismo para cambiar umbrales
//! por lote/variante sin tocar la secuencia. Ver
//! [`aplicar_limites`](aplicar_limites) y ADR-0008.

use modelo::{
    Argumento, Asignacion, DefinicionEjecutor, DefinicionPaso, DefinicionSecuencia, EntradaPaso,
    Limite, Operador, Programa, TipoEjecutor, TipoPaso, ValorDefinicion,
};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Una secuencia como se lee del YAML, antes de traducirse al modelo del
/// motor. `deny_unknown_fields` hace que un campo no reconocido falle la
/// carga en vez de ignorarse en silencio.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecuenciaYaml {
    /// Nombre de la secuencia. Opcional: si se omite, una subsecuencia inline
    /// toma el nombre de su **clave** en `subsecuencias` (ver
    /// [`secuencia_yaml_a_definicion`]); la raíz, en cambio, debe tenerlo.
    #[serde(default)]
    name: String,
    /// Opcional: si no viene, no hay pasos de setup.
    #[serde(default)]
    setup: Vec<PasoYaml>,
    /// Requerido: es la medición. Que falte o esté vacío es error.
    main: Vec<PasoYaml>,
    /// Opcional: si no viene, no hay pasos de cleanup.
    #[serde(default)]
    cleanup: Vec<PasoYaml>,
    /// M4 (RF-31): variables locales de la secuencia. El tipo se infiere del
    /// escalar YAML (ver [`ValorYaml`]).
    #[serde(default)]
    locals: HashMap<String, ValorYaml>,
    /// M4 (RF-31): parámetros de entrada/salida. Vacíos en M4-núcleo (sin
    /// sequence call); reservados para M4b.
    #[serde(default)]
    parameters: HashMap<String, ValorYaml>,
    /// M4 (RF-31): globales del archivo, compartidas por todas las secuencias.
    #[serde(default)]
    file_globals: HashMap<String, ValorYaml>,
    /// M4b (RF-27): subsecuencias **inline** del archivo, invocables por
    /// nombre desde cualquier secuencia de este archivo. Privadas del
    /// archivo: los otros archivos invocan la secuencia raíz por path, no
    /// éstas. Es recursivo: una inline es otra `SecuenciaYaml` completa.
    #[serde(default)]
    subsequences: HashMap<String, SecuenciaYaml>,
    /// M5-ext.1 (RF-36.3): ejecutores declarados en el YAML. Sin esta
    /// sección, todo paso va al ejecutor embebido (default, compat M4b).
    #[serde(default)]
    executors: Vec<EjecutorYaml>,
}

/// Un ejecutor como se lee del YAML (`ejecutores:`), antes de traducirse a
/// `modelo::DefinicionEjecutor`. `deny_unknown_fields` (fail-fast) igual que
/// el resto del schema. La coherencia entre `tipo` y sus campos se valida en
/// [`EjecutorYaml::a_definicion`].
#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EjecutorYaml {
    name: String,
    /// `"embedded"` (default), `"wasm"` o `"grpc"`.
    ///
    /// `kind` y no `type`: `type` es palabra reservada de Rust. El
    /// `serde(rename)` es por el lenguaje, no por el idioma — en el YAML la
    /// clave es `type`.
    #[serde(default = "tipo_ejecutor_por_defecto", rename = "type")]
    kind: String,
    /// Sólo si `tipo == "wasm"`. Path relativo al directorio del YAML.
    #[serde(default)]
    path: Option<String>,
    /// Sólo si `tipo == "grpc"`. Host; puede ser no-loopback **sólo si se
    /// declara** (relajación acotada del loopback de ADR-0011).
    #[serde(default)]
    host: Option<String>,
    /// Sólo si `type == "grpc"`. Puerto.
    #[serde(default)]
    port: Option<u16>,
}

fn tipo_ejecutor_por_defecto() -> String {
    "embedded".into()
}

/// A variable declared in a scope of M4 (`locals:`, `parameters:`,
/// `file_globals:`).
///
/// Three of the four forms are a scalar and the type is read off it:
/// `true`→bool, `4.5`→number, `"A-2026"`→text. `untagged` tries the variants
/// in order; `Bool` first stops `true` being tried as an `f64`, and the map
/// last because no scalar can match it.
///
/// The fourth is a **declaration with no value** (ADR-0022 §3):
///
/// ```yaml
/// locals:
///   rack: { type: reference, executor: bench }
/// ```
///
/// A reference has no literal form — refusing one written by hand is one of
/// the four things the type exists to buy (ADR-0022 §1) — so the only thing a
/// sequence can state about the variable is which executor its handle will
/// come from.
#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
enum ValorYaml {
    Bool(bool),
    Numero(f64),
    Texto(String),
    Declaracion(DeclaracionYaml),
}

/// The `{ type: ..., executor: ... }` form of a variable declaration.
///
/// `deny_unknown_fields` is **not** used here, and that is deliberate: inside
/// an `untagged` enum it would make a typo fall through every variant and
/// surface as *"data did not match any variant"*, which names neither the
/// field nor the file. Unknown keys are collected instead and reported by
/// name, which is the diagnostic standard of the rest of this loader (#20).
#[derive(Debug, PartialEq, Clone, Deserialize)]
struct DeclaracionYaml {
    /// `type` is a Rust keyword; the rename is for the language, not the
    /// vocabulary — in the YAML the key is `type`, as everywhere else.
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    executor: Option<String>,
    #[serde(flatten)]
    otros: std::collections::BTreeMap<String, serde::de::IgnoredAny>,
}

/// The one `type` a declaration may state today. There is no `type: number`
/// because a number is written as a number: adding a long form for what a
/// scalar already says would be two ways to write one thing.
const TIPO_REFERENCIA: &str = "reference";

impl ValorYaml {
    /// The model's form of this declaration.
    ///
    /// `scope` and `secuencia` are only for the error messages, and they earn
    /// their place: "a reference cannot be declared here" is useless without
    /// saying where *here* is.
    fn a_definicion(
        self,
        nombre: &str,
        scope: &str,
        secuencia: &str,
    ) -> Result<ValorDefinicion, ErrorCarga> {
        let d = match self {
            ValorYaml::Bool(b) => return Ok(ValorDefinicion::Bool(b)),
            ValorYaml::Numero(x) => return Ok(ValorDefinicion::Numero(x)),
            ValorYaml::Texto(s) => return Ok(ValorDefinicion::Texto(s)),
            ValorYaml::Declaracion(d) => d,
        };
        if !d.otros.is_empty() {
            let claves: Vec<&str> = d.otros.keys().map(|k| k.as_str()).collect();
            return Err(ErrorCarga::Validacion(format!(
                "la declaración de '{scope}.{nombre}' en la secuencia '{secuencia}' trae \
                 campo(s) que no existen: {}. Una declaración de referencia es \
                 '{{ type: {TIPO_REFERENCIA}, executor: <nombre> }}'",
                claves.join(", ")
            )));
        }
        if d.kind != TIPO_REFERENCIA {
            let dicho = if d.kind.is_empty() {
                "no dice de qué tipo es".to_string()
            } else {
                format!("dice 'type: {}'", d.kind)
            };
            return Err(ErrorCarga::Validacion(format!(
                "'{scope}.{nombre}' de la secuencia '{secuencia}' se declara como un mapa y \
                 {dicho}. La única declaración que no es un escalar es \
                 '{{ type: {TIPO_REFERENCIA}, executor: <nombre> }}'; un número, un texto o \
                 un booleano se escriben como el escalar que son"
            )));
        }
        // A reference lives in `locals:` and nowhere else, and each refusal is
        // for its own reason. `file_globals` are the file's constants and the
        // engine refuses to write them at all, so a handle declared there
        // could never be filled in — a variable that is unusable by
        // construction. `parameters` is the by-reference channel of a
        // `sequence_call`, and handing a rack to a subsequence is a decision
        // ADR-0022 does not take (its §Open leaves the process model's channel
        // for a rack unresolved); allowing it here would be taking it by
        // accident.
        if scope != "locals" {
            return Err(ErrorCarga::Validacion(format!(
                "'{scope}.{nombre}' de la secuencia '{secuencia}' se declara de tipo \
                 '{TIPO_REFERENCIA}', y una referencia sólo se puede declarar en 'locals:'. \
                 En 'file_globals:' nunca se podría rellenar (son constantes del fichero y \
                 el motor rechaza escribirlas), y pasar una referencia a una subsecuencia \
                 por 'parameters:' no está decidido todavía (ADR-0022)"
            )));
        }
        let executor = d.executor.ok_or_else(|| {
            ErrorCarga::Validacion(format!(
                "'locals.{nombre}' de la secuencia '{secuencia}' se declara de tipo \
                 '{TIPO_REFERENCIA}' y no dice de qué ejecutor. El ejecutor es parte de la \
                 declaración: es lo único que permite rechazar antes de arrancar una \
                 referencia que se le pasa a un paso de otro ejecutor, donde no significa \
                 nada (ADR-0022 §3)"
            ))
        })?;
        Ok(ValorDefinicion::Reference { executor })
    }
}

/// One whole scope's declarations, translated with the scope's name in hand so
/// a bad one can say where it is.
fn declaraciones(
    mapa: HashMap<String, ValorYaml>,
    scope: &str,
    secuencia: &str,
) -> Result<HashMap<String, ValorDefinicion>, ErrorCarga> {
    let mut fuera = HashMap::with_capacity(mapa.len());
    for (k, v) in mapa {
        let d = v.a_definicion(&k, scope, secuencia)?;
        fuera.insert(k, d);
    }
    Ok(fuera)
}

/// Un paso como se lee del YAML. `reintentos` por defecto es 1 (un solo
/// tiro) si se omite. `limite` (desde M3, RF-29) es opcional: si lo trae, el
/// motor evalúa la regla contra la medida que devuelve el paso (ADR-0008).
///
/// Desde M4: `disable`/`pause_on_fail` (RF-34), `precondicion` (RF-33),
/// `asigna` (RF-31), `tipo`/`statement` (RF-27). Las expresiones (`precondicion`,
/// `asigna`, `statement`, `condicion`) vienen como texto y se parsean a AST en
/// [`PasoYaml::a_definicion`] (fail-fast).
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PasoYaml {
    name: String,
    #[serde(default = "reintentos_por_defecto")]
    retries: u32,
    #[serde(default)]
    limit: Option<LimiteYaml>,
    /// RF-34: si `true`, el motor salta el paso sin invocarlo.
    #[serde(default)]
    disable: bool,
    /// RF-34: si `true` y el paso falla, el motor detiene la fase en curso.
    #[serde(default)]
    pause_on_fail: bool,
    /// RF-33: expresión booleana; si es falsa, el paso se salta sin gastar
    /// intento. Texto → AST en `a_definicion`.
    #[serde(default)]
    precondition: Option<String>,
    /// RF-31: mapa `nombre_local -> expr`; el motor vuelca cada `expr` (sobre
    /// `resultado`/scopes) a la Local. Texto → AST en `a_definicion`.
    #[serde(default)]
    assign: Option<HashMap<String, String>>,
    /// RF-27: `"grpc"` (default), `"statement"`, `"sequence_call"` o
    /// `"pass_fail"`. `kind` por la palabra reservada de Rust; en el YAML la
    /// clave es `type`.
    #[serde(default = "tipo_por_defecto", rename = "type")]
    kind: String,
    /// RF-27: sentencia(s) a ejecutar si `tipo == "statement"`. Texto → AST.
    #[serde(default)]
    statement: Option<String>,
    /// RF-25 (ADR-0018): expresión booleana del veredicto si
    /// `tipo == "pass_fail"`. Texto → AST en `a_definicion`.
    #[serde(default)]
    condition: Option<String>,
    /// M4b (RF-27): destino del sequence call si `tipo == "sequence_call"`.
    /// Un **nombre** (subsecuencia inline del mismo archivo) o un **path
    /// relativo** (archivo externo); se distingue con [`es_path`]. Texto.
    #[serde(default)]
    sequence: Option<String>,
    /// ADR-0020: los parámetros **by-value** de un paso `grpc`. Mapa
    /// `nombre -> literal | "${expr}"`, que viaja en la petición.
    ///
    /// Es `ValorYaml` y no `String` para poder distinguir `channel: 2` de
    /// `channel: "2"`: el tipo del literal es el tipo que viaja por el cable.
    #[serde(default)]
    inputs: Option<HashMap<String, ValorYaml>>,
    /// M4b (RF-27): argumentos **by-reference** de un `sequence_call`, mapa
    /// `nombre_parameter -> "locals.X"`. Cada valor se parsea a AST y se
    /// valida como `Expresion::Var { scope: Locals, .. }` (un lvalue local).
    ///
    /// Se llamaba `parametros` igual que los de arriba, y esa colisión era
    /// una trampa: el mismo bloque copiado de un sitio al otro cambiaba de
    /// significado —aquí es una referencia, allí sería el texto literal—.
    /// Con dos nombres distintos, copiarlo da error de campo desconocido.
    #[serde(default)]
    args: Option<HashMap<String, ValorYaml>>,
    /// M5-ext.1 (RF-36.3): nombre del ejecutor que atiende este paso. Debe
    /// existir en `ejecutores` de la secuencia (fail-fast al cargar). Si se
    /// omite, el paso va al ejecutor embebido (default).
    #[serde(default)]
    executor: Option<String>,
}

fn reintentos_por_defecto() -> u32 {
    1
}

fn tipo_por_defecto() -> String {
    "grpc".into()
}

/// Un límite como se lee del YAML, antes de traducirse a `modelo::Limite`.
///
/// `deny_unknown_fields` hace que un campo que no pertenece al `tipo` declarado
/// falle la carga en vez de ignorarse en silencio; la validación posterior
/// refuerza que un `rango` traiga `min`/`max` (y no `op`/`esperado`) y un
/// `comparacion` traiga `op`/`esperado` (y no `min`/`max`).
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LimiteYaml {
    /// `"range"` o `"comparison"`. `kind` por la palabra reservada de Rust;
    /// en el YAML la clave es `type`.
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    expected: Option<f64>,
}

impl LimiteYaml {
    /// Traduce a `modelo::Limite`, validando que los campos cuadren con el
    /// `tipo` declarado. `nombre_paso` solo para mensajes de error.
    fn a_limite(&self, nombre_paso: &str) -> Result<Limite, ErrorCarga> {
        match self.kind.as_str() {
            "range" => {
                let Some(min) = self.min else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite 'range' sin 'min'"
                    )));
                };
                let Some(max) = self.max else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite 'range' sin 'max'"
                    )));
                };
                if min > max {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite 'range' con min ({min}) > max ({max})"
                    )));
                }
                if self.op.is_some() || self.expected.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite 'range' con campos 'op'/'expected' (no aplican a un rango)"
                    )));
                }
                Ok(Limite::Rango { min, max })
            }
            "comparison" => {
                let Some(op_texto) = &self.op else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite 'comparison' sin 'op'"
                    )));
                };
                let Some(op) = Operador::de_texto(op_texto) else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite 'comparison' con 'op' inválido '{op_texto}' (eq/ne/lt/le/gt/ge)"
                    )));
                };
                let Some(esperado) = self.expected else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite 'comparison' sin 'expected'"
                    )));
                };
                if self.min.is_some() || self.max.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite 'comparison' con campos 'min'/'max' (no aplican a una comparación)"
                    )));
                }
                Ok(Limite::Comparacion { op, esperado })
            }
            otro => Err(ErrorCarga::Validacion(format!(
                "el paso '{nombre_paso}' tiene un límite con 'type' '{otro}' desconocido (range|comparison)"
            ))),
        }
    }
}

/// Nombre de ejecutor reservado del motor (clave interna de la conexión al
/// ejecutor embebido). No declarable en el YAML: el cargador lo rechaza.
pub const NOMBRE_EMBEDIDO_RESERVADO: &str = modelo::EJECUTOR_EMBEBIDO;

impl EjecutorYaml {
    /// Traduce a `modelo::DefinicionEjecutor`, validando la coherencia entre
    /// `tipo` y sus campos (fail-fast). `dir_yaml` es el directorio del
    /// archivo que declara el ejecutor: los paths `wasm` se resuelven
    /// relativo a él.
    fn a_definicion(self, dir_yaml: &Path) -> Result<DefinicionEjecutor, ErrorCarga> {
        if self.name == NOMBRE_EMBEDIDO_RESERVADO {
            return Err(ErrorCarga::Validacion(format!(
                "el ejecutor '{NOMBRE_EMBEDIDO_RESERVADO}' está reservado; elige otro nombre"
            )));
        }
        let tipo = match self.kind.as_str() {
            "embedded" => {
                if self.path.is_some() || self.host.is_some() || self.port.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'embedded' pero trae 'path'/'host'/'port' (no aplican)",
                        self.name
                    )));
                }
                TipoEjecutor::Embebido
            }
            "wasm" => {
                if self.host.is_some() || self.port.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'wasm' pero trae 'host'/'puerto' (sólo aplican a 'grpc')",
                        self.name
                    )));
                }
                let Some(path) = self.path else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'wasm' pero no trae 'path'",
                        self.name
                    )));
                };
                // El cargador corre dentro del sandbox WASM del motor, que
                // solo tiene preabierto el directorio del YAML (DEF-4): un
                // path absoluto es invisible para `exists()` exista o no en
                // el host, así que se distingue antes de comprobar.
                if Path::new(&path).is_absolute() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'wasm' con 'path' absoluto '{}': el \
                         cargador corre en un sandbox que solo ve el directorio \
                         del YAML; usa un path relativo",
                        self.name, path
                    )));
                }
                // El path debe existir (relativo al directorio del YAML),
                // como las subsecuencias externas (fail-fast al cargar).
                let ruta = normalizar_path(dir_yaml, Path::new(&path));
                if !ruta.exists() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'wasm' y su 'path' '{}' no existe",
                        self.name, path
                    )));
                }
                TipoEjecutor::Wasm { path }
            }
            "grpc" => {
                if self.path.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'grpc' pero trae 'path' (sólo aplica a 'wasm')",
                        self.name
                    )));
                }
                let (Some(host), Some(puerto)) = (self.host, self.port) else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'grpc' pero no trae 'host' y 'port'",
                        self.name
                    )));
                };
                TipoEjecutor::Grpc { host, puerto }
            }
            otro => {
                return Err(ErrorCarga::Validacion(format!(
                    "el ejecutor '{}' tiene 'type' '{otro}' desconocido (embedded|wasm|grpc)",
                    self.name
                )))
            }
        };
        Ok(DefinicionEjecutor {
            nombre: self.name,
            tipo,
        })
    }
}

/// Override de ejecutores por CLI (RF-36.3, patrón `--limits`): reescribe la
/// tabla de `programa.ejecutores` desde la lista de overrides `nombre=host:puerto`
/// (p. ej. `python=192.168.1.50:9101`).
///
/// - El `nombre` debe existir en `ejecutores` (fail-fast): si no, error.
/// - Un ejecutor `grpc` se re-apunta a `host:puerto`.
/// - Un ejecutor `embebido` o `wasm` se **convierte** a `grpc` (el override
///   explícito fuerza remoto, igual que el host re-escribe los `.wasm`).
///
/// Devuelve cuántos ejecutores se sobreescribieron (para el log del CLI).
pub fn aplicar_override_ejecutores(
    programa: &mut Programa,
    overrides: &[String],
) -> Result<usize, ErrorCarga> {
    let mut aplicados = 0;
    for override_ in overrides {
        let (nombre, resto) = override_.split_once('=').ok_or_else(|| {
            ErrorCarga::Validacion(format!(
                "override de ejecutor inválido '{override_}' (esperado 'nombre=host:puerto')"
            ))
        })?;
        let (host, puerto) = resto.split_once(':').ok_or_else(|| {
            ErrorCarga::Validacion(format!(
                "override de ejecutor '{override_}' inválido (esperado 'nombre=host:puerto')"
            ))
        })?;
        let puerto: u16 = puerto.parse().map_err(|_| {
            ErrorCarga::Validacion(format!(
                "override de ejecutor '{override_}': el puerto '{puerto}' no es un número"
            ))
        })?;
        let ejecutor = programa.ejecutores.get_mut(nombre).ok_or_else(|| {
            ErrorCarga::Validacion(format!(
                "override de ejecutor: '{nombre}' no está declarado en 'ejecutores:'"
            ))
        })?;
        ejecutor.tipo = TipoEjecutor::Grpc {
            host: host.to_string(),
            puerto,
        };
        aplicados += 1;
    }
    Ok(aplicados)
}

/// Qué salió mal al cargar una secuencia. Sigue el mismo patrón de errores
/// manuales que `motor::Error` (`crates/motor/src/lib.rs`): `Display` +
/// `error::Error` + `From`, sin `thiserror`.
#[derive(Debug)]
pub enum ErrorCarga {
    /// No se pudo leer el fichero del disco.
    Lectura(std::io::Error),
    /// El YAML no parsea o no encaja en el schema (campo desconocido, tipo
    /// equivocado, campo obligatorio ausente).
    Sintaxis(noyalib::Error),
    /// El YAML parsea, pero viola una regla de negocio (nombre vacío,
    /// `reintentos` 0, `main` vacío).
    Validacion(String),
    /// Un error de esquema ya **redactado por completo**: el mensaje trae su
    /// propio contexto y `Display` no le antepone nada. Es lo que produce el
    /// diagnóstico de DIAG-5 cuando sabe qué quiso escribir el usuario y el
    /// prefijo genérico («YAML inválido», «secuencia inválida») estorbaría.
    Diagnostico(String),
}

impl std::fmt::Display for ErrorCarga {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorCarga::Lectura(e) => write!(f, "no se pudo leer el fichero: {e}"),
            ErrorCarga::Sintaxis(e) => write!(f, "YAML inválido: {e}"),
            ErrorCarga::Validacion(m) => write!(f, "secuencia inválida: {m}"),
            ErrorCarga::Diagnostico(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ErrorCarga {}

impl From<std::io::Error> for ErrorCarga {
    fn from(e: std::io::Error) -> Self {
        ErrorCarga::Lectura(e)
    }
}

impl From<noyalib::Error> for ErrorCarga {
    fn from(e: noyalib::Error) -> Self {
        ErrorCarga::Sintaxis(e)
    }
}

/// Todos los campos del schema, para poder sugerir el correcto ante una
/// errata. Es una ayuda de diagnóstico, no una fuente de verdad: el schema lo
/// imponen los `struct` con `deny_unknown_fields`, y si esta lista se queda
/// corta lo único que se pierde es una sugerencia.
const CAMPOS_DEL_SCHEMA: [&str; 28] = [
    // SecuenciaYaml
    "name",
    "setup",
    "main",
    "cleanup",
    "locals",
    "parameters",
    "file_globals",
    "subsequences",
    "executors",
    // PasoYaml
    "retries",
    "limit",
    "disable",
    "pause_on_fail",
    "precondition",
    "assign",
    "type",
    "statement",
    "condition",
    "sequence",
    "inputs",
    "args",
    "executor",
    // EjecutorYaml
    "path",
    "host",
    "port",
    // LimiteYaml
    "min",
    "max",
    "op",
];

/// Lo que la gente escribe de verdad cuando se equivoca, y a qué campo se le
/// redirige.
///
/// **La mitad de esta tabla acaba de desaparecer, y conviene saber por qué.**
/// Salió de los ficheros de la beta, y lo que registraba era que el
/// betatester escribía `name`, `retries`, `assign`, `precondition`,
/// `executor` y `sequence` por instinto contra un schema que estaba en
/// español. Eran ocho entradas para traducir de vuelta lo que la gente ya
/// escribía en inglés. Ahora el schema **es** ese inglés, así que sobran: es
/// la evidencia de campo de que la traducción iba en la dirección correcta.
///
/// Lo que queda son dos grupos: los nombres que la gente trae de otras
/// herramientas (`steps`, `variables`), y **el castellano del schema
/// anterior**, para que quien tenga una secuencia vieja reciba «querías decir
/// `name`» en vez de «campo desconocido». Eso es diagnóstico, no
/// compatibilidad: el campo viejo **no se acepta**, sólo se reconoce para
/// explicar el error.
const ALIAS_DE_CAMPO: [(&str, &str); 17] = [
    // De otras herramientas.
    ("steps", "main"),
    ("pasos", "main"),
    ("variables", "locals"),
    ("limits", "limit"),
    // El schema anterior, en castellano.
    ("nombre", "name"),
    ("reintentos", "retries"),
    ("limite", "limit"),
    ("limites", "limit"),
    ("precondicion", "precondition"),
    ("asigna", "assign"),
    ("tipo", "type"),
    ("condicion", "condition"),
    ("secuencia", "sequence"),
    ("subsecuencias", "subsequences"),
    ("ejecutor", "executor"),
    ("ejecutores", "executors"),
    ("puerto", "port"),
];

/// Distancia de edición (Levenshtein) en caracteres, no en bytes: el schema
/// lleva acentos (`precondicion` no, pero `límites` sí en los alias).
fn distancia_edicion(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut fila: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut anterior = fila[0];
        fila[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let sustituir = anterior + usize::from(ca != cb);
            anterior = fila[j + 1];
            fila[j + 1] = sustituir.min(fila[j] + 1).min(fila[j + 1] + 1);
        }
    }
    fila[b.len()]
}

/// Qué quiso escribir el usuario: primero los alias conocidos, y si no, el
/// campo del schema más parecido. El umbral es estrecho a propósito — una
/// sugerencia equivocada desorienta más que ninguna.
fn sugerencia_de_campo(campo: &str) -> Option<&'static str> {
    if let Some((_, bueno)) = ALIAS_DE_CAMPO.iter().find(|(malo, _)| *malo == campo) {
        return Some(bueno);
    }
    let umbral = if campo.chars().count() <= 4 { 1 } else { 2 };
    CAMPOS_DEL_SCHEMA
        .iter()
        .map(|c| (*c, distancia_edicion(campo, c)))
        .filter(|(_, d)| *d <= umbral)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c)
}

/// Dónde aparece `clave` dentro del YAML, en notación de ruta
/// (`subsecuencias.init`, `main[2].limite`). Es lo que le falta al error de
/// serde, que solo da el nombre del campo: en un fichero con subsecuencias, un
/// `unknown field: steps` no dice en cuál de ellas está.
fn rutas_de_clave(valor: &noyalib::Value, clave: &str, prefijo: &str, out: &mut Vec<String>) {
    match valor {
        noyalib::Value::Mapping(m) => {
            for (k, v) in m {
                if k == clave {
                    out.push(if prefijo.is_empty() {
                        "la raíz".to_string()
                    } else {
                        prefijo.to_string()
                    });
                }
                let sub = if prefijo.is_empty() {
                    k.to_string()
                } else {
                    format!("{prefijo}.{k}")
                };
                rutas_de_clave(v, clave, &sub, out);
            }
        }
        noyalib::Value::Sequence(s) => {
            for (i, v) in s.iter().enumerate() {
                rutas_de_clave(v, clave, &format!("{prefijo}[{i}]"), out);
            }
        }
        _ => {}
    }
}

/// Convierte el `unknown field: steps` de serde en algo accionable: dónde está
/// y qué se quiso escribir (DIAG-5). Solo corre en el camino de error, y si no
/// consigue ubicar el campo devuelve el error original intacto.
fn diagnostica_campo_desconocido(texto: &str, original: noyalib::Error) -> ErrorCarga {
    let noyalib::Error::UnknownField(campo) = &original else {
        return ErrorCarga::Sintaxis(original);
    };
    let Ok(raiz) = noyalib::from_str::<noyalib::Value>(texto) else {
        return ErrorCarga::Sintaxis(original);
    };
    let mut rutas = Vec::new();
    rutas_de_clave(&raiz, campo, "", &mut rutas);
    if rutas.is_empty() {
        return ErrorCarga::Sintaxis(original);
    }
    let mut msg = format!("campo desconocido '{campo}' en {}", rutas.join(", "));
    if let Some(bueno) = sugerencia_de_campo(campo) {
        msg.push_str(&format!(" (¿querías '{bueno}'?)"));
    }
    ErrorCarga::Diagnostico(msg)
}

/// Carga una secuencia desde texto YAML. Es el punto testeable sin tocar
/// el disco; `cargar_de_archivo` lo envuelve. No resuelve sequence calls (ni
/// valida lvalues contra la secuencia padre): para eso, usar
/// [`cargar_programa_de_archivo`].
pub fn cargar_de_texto(texto: &str) -> Result<DefinicionSecuencia, ErrorCarga> {
    let yaml: SecuenciaYaml = match noyalib::from_str(texto) {
        Ok(y) => y,
        Err(e) => return Err(diagnostica_campo_desconocido(texto, e)),
    };
    secuencia_yaml_a_definicion(yaml, None)
}

/// Como [`cargar_de_texto`], pero para un archivo que se carga como
/// **subsecuencia externa** de un `sequence_call`.
///
/// Issue #21: `leer_ejecutores` sólo se llama sobre la raíz (y, con
/// `--process-model`, sobre el PM y la secuencia del usuario). Los archivos
/// que entran por la cola de subsecuencias se leían con `cargar_de_texto`, que
/// ni mira la sección `ejecutores:` — así que una subsecuencia que declaraba
/// la suya la veía **descartada sin una palabra**. En el caso peor la raíz
/// declaraba otro ejecutor con el mismo nombre, `--validate` decía «válida», y
/// los pasos de la subsecuencia acababan en un ejecutor que no era el que su
/// autor había escrito.
///
/// Este es el único punto del pipeline donde se tiene el YAML crudo de la
/// subsecuencia, así que es donde se detecta.
fn cargar_subsecuencia_externa(
    texto: &str,
    origen: &str,
) -> Result<DefinicionSecuencia, ErrorCarga> {
    let yaml: SecuenciaYaml = match noyalib::from_str(texto) {
        Ok(y) => y,
        Err(e) => return Err(diagnostica_campo_desconocido(texto, e)),
    };
    if !yaml.executors.is_empty() {
        return Err(ErrorCarga::Validacion(format!(
            "la subsecuencia externa '{origen}' declara una sección 'executors:', y \
             Anvil no la lee: la tabla de ejecutores se declara **una sola vez**, en la \
             secuencia raíz (la que se pasa a anvil; con --process-model, también en el \
             process model). Las subsecuencias no declaran los suyos, los referencian \
             por nombre con 'ejecutor:'. Mueve esa declaración a la raíz"
        )));
    }
    secuencia_yaml_a_definicion(yaml, None)
}

/// Traduce una `SecuenciaYaml` (parseada) a `DefinicionSecuencia`,
/// validándola (reglas de negocio + límites + expresiones) y traduciendo
/// recursivamente sus `subsecuencias` inline. `fallback` es la **clave** del
/// mapa `subsecuencias` cuando se traduce una inline: si el `nombre` del
/// YAML está vacío, se toma esa clave (DRY: una inline se nombra por su
/// entrada en el mapa). La raíz no tiene fallback y debe declarar `nombre`.
fn secuencia_yaml_a_definicion(
    mut y: SecuenciaYaml,
    fallback: Option<&str>,
) -> Result<DefinicionSecuencia, ErrorCarga> {
    if y.name.trim().is_empty() {
        match fallback {
            Some(k) => y.name = k.to_string(),
            None => {
                return Err(ErrorCarga::Validacion(
                    "el 'name' de la secuencia no puede estar vacío".into(),
                ))
            }
        }
    }
    validar(&y)?;

    let traduce_pasos = |pasos: Vec<PasoYaml>| -> Result<Vec<DefinicionPaso>, ErrorCarga> {
        pasos.into_iter().map(PasoYaml::a_definicion).collect()
    };

    // Las subsecuencias inline se traducen recursivamente (cada una es una
    // SecuenciaYaml completa, con su propia validación; el nombre cae al
    // fallback de la clave).
    let mut subsecuencias = HashMap::new();
    let nombre_secuencia = y.name.clone();
    for (k, sub) in y.subsequences {
        // Issue #21, la mitad inline: `SecuenciaYaml` es recursiva, así que una
        // inline puede escribir `ejecutores:` — y se descartaba igual de
        // callado que en una externa. La tabla es del `Programa`, no de la
        // secuencia, y sólo se lee de la raíz del archivo.
        if !sub.executors.is_empty() {
            return Err(ErrorCarga::Validacion(format!(
                "la subsecuencia inline '{k}' de la secuencia '{nombre_secuencia}' \
                 declara una sección 'executors:', y Anvil no la lee: los ejecutores \
                 se declaran **una sola vez**, en el 'executors:' de la secuencia raíz \
                 del archivo. Muévela ahí; aquí basta con referenciarlos por nombre \
                 con 'ejecutor:'"
            )));
        }
        subsecuencias.insert(k.clone(), secuencia_yaml_a_definicion(sub, Some(&k))?);
    }

    let def = DefinicionSecuencia {
        nombre: y.name,
        pasos_setup: traduce_pasos(y.setup)?,
        pasos_main: traduce_pasos(y.main)?,
        pasos_cleanup: traduce_pasos(y.cleanup)?,
        locals: declaraciones(y.locals, "locals", &nombre_secuencia)?,
        parameters: declaraciones(y.parameters, "parameters", &nombre_secuencia)?,
        file_globals: declaraciones(y.file_globals, "file_globals", &nombre_secuencia)?,
        subsecuencias,
    };
    validar_lvalues(&def)?;
    validar_alcance_resultado(&def)?;
    validar_campos_de_resultado(&def)?;
    // La última, a propósito: ver el docstring de `validar_variables_leidas`.
    validar_variables_leidas(&def)?;
    Ok(def)
}

/// §5 del informe de beta: `resultado.*` **sólo** está ligado durante el
/// `asigna` del propio paso — el motor hace `set_resultado` justo antes y
/// `limpia_resultado` después (`motor/src/entorno.rs`). Fuera de esa ventana
/// no fallaba: valía `nothing`.
///
/// Eso convertía un error de definición en tres capas de silencio
/// encadenadas. `precondition: 'result.measured_value != nothing'` es un
/// `false` constante → el paso se salta → y como `saltado` es neutral en el
/// agregado, **la secuencia termina en verde**. En la campaña ese patrón se
/// propagó a 19 secuencias y 51 precondiciones, y produjo dos «bugs críticos»
/// que no lo eran.
///
/// Regla, fail-fast al cargar: `resultado.*` en `precondicion`, en la
/// `condicion` de un `pass_fail` o en un `statement` → error. En `asigna`,
/// que es su sitio, se permite.
fn validar_alcance_resultado(def: &DefinicionSecuencia) -> Result<(), ErrorCarga> {
    for p in def
        .pasos_setup
        .iter()
        .chain(&def.pasos_main)
        .chain(&def.pasos_cleanup)
    {
        // (nombre del campo YAML, campo con la expresión) — el mensaje cita
        // el campo tal y como el usuario lo escribió.
        let mut donde: Vec<(&str, &expr::Expresion)> = Vec::new();
        if let Some(pre) = &p.precondicion {
            donde.push(("precondition", pre));
        }
        if let Some(cond) = &p.condicion {
            donde.push(("condition", cond));
        }
        for (campo_yaml, e) in donde {
            if let Some(campo) = primer_uso_de_resultado(e) {
                return Err(error_resultado_fuera_de_asigna(
                    &p.nombre, campo_yaml, &campo,
                ));
            }
        }
        if let Some(stmts) = &p.statement {
            for s in stmts {
                let expr::Sentencia::Assign {
                    scope,
                    campo,
                    valor,
                } = s;
                // Como lvalue (`resultado.x = …`) y como lectura en el lado
                // derecho: los dos son el mismo malentendido.
                if *scope == expr::Scope::Resultado {
                    return Err(error_resultado_fuera_de_asigna(
                        &p.nombre,
                        "statement",
                        campo,
                    ));
                }
                if let Some(campo) = primer_uso_de_resultado(valor) {
                    return Err(error_resultado_fuera_de_asigna(
                        &p.nombre,
                        "statement",
                        &campo,
                    ));
                }
            }
        }
    }
    Ok(())
}

/// El primer `resultado.X` que aparece en la expresión, o `None`. Recorre el
/// AST entero: el caso de la campaña era una conjunción
/// (`locals.v > 4.9 && resultado.valor_medido != nothing`), así que mirar
/// sólo la raíz no habría bastado.
fn primer_uso_de_resultado(e: &expr::Expresion) -> Option<String> {
    primer_uso_de_resultado_si(e, &|_| true)
}

/// Como `primer_uso_de_resultado`, pero sólo cuenta los `resultado.X` cuyo
/// campo cumple `pred`. Con `|_| true` es «cualquier uso» (dónde se puede
/// escribir `resultado.*`); con «campo desconocido» es el typo del issue #27.
fn primer_uso_de_resultado_si(e: &expr::Expresion, pred: &impl Fn(&str) -> bool) -> Option<String> {
    match e {
        expr::Expresion::Var {
            scope: expr::Scope::Resultado,
            campo,
        } if pred(campo) => Some(campo.clone()),
        expr::Expresion::Var { .. } | expr::Expresion::Lit(_) => None,
        expr::Expresion::BinOp { izq, der, .. } => {
            primer_uso_de_resultado_si(izq, pred).or_else(|| primer_uso_de_resultado_si(der, pred))
        }
        expr::Expresion::UnOp { operando, .. } => primer_uso_de_resultado_si(operando, pred),
    }
}

/// ADR-0019, regla de detección (issue #27): los campos de `resultado` son
/// tres y conocidos (`modelo::CAMPOS_RESULTADO`), así que un
/// `result.measured_valu` es **comprobable sin ejecutar**. Y hay que
/// comprobarlo ahí: en runtime valía `nothing`, ese `nothing` se volcaba a la
/// local que iba a decidir el veredicto, y la secuencia salía en `paso` con la
/// variable destruida. Un typo no puede costar una campaña entera.
///
/// Se mira sólo `asigna` porque es el único sitio donde `resultado.*` es
/// legal; en `precondicion`, `condicion` y `statement` lo rechaza entero
/// `validar_alcance_resultado`, que corre justo antes.
///
/// **`resultado.salidas.<nombre>` es la excepción, y es deliberada**
/// (ADR-0020 §3). El cargador no sabe qué salidas devuelve un paso —eso sólo
/// se ve corriendo— así que aquí sólo se comprueba la *forma*: que haya un
/// nombre detrás del punto. Que ese nombre exista lo caza el motor en
/// ejecución, como `error`. Es la excepción a la regla de detección de
/// ADR-0019 que el propio ADR-0020 asume, y lo que devolvería este terreno a
/// `--validate` es la introspección de firma del issue #45.
fn validar_campos_de_resultado(def: &DefinicionSecuencia) -> Result<(), ErrorCarga> {
    let desconocido = |campo: &str| {
        match campo.strip_prefix(expr::CAMPO_SALIDAS) {
            // `resultado.salidas.<algo>`: la forma es válida.
            Some(resto) if resto.starts_with('.') && resto.len() > 1 => false,
            // `resultado.salidas` a secas, o `resultado.salidas.` — no nombra
            // ninguna salida, así que no puede valer nada.
            Some(_) => true,
            None => !modelo::CAMPOS_RESULTADO.contains(&campo),
        }
    };
    for p in def
        .pasos_setup
        .iter()
        .chain(&def.pasos_main)
        .chain(&def.pasos_cleanup)
    {
        let Some(asignaciones) = &p.asigna else {
            continue;
        };
        for a in asignaciones {
            if let Some(campo) = primer_uso_de_resultado_si(&a.expr, &desconocido) {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' asigna a '{}' desde 'result.{campo}', que no existe: \
                     los campos de 'result' son {}, y 'outputs.<name>' para \
                     lo que devuelva el paso",
                    p.nombre,
                    a.var,
                    modelo::CAMPOS_RESULTADO
                        .map(|c| format!("'{c}'"))
                        .join(", ")
                )));
            }
        }
    }
    Ok(())
}

/// El mensaje dice **dónde** está el uso indebido y **dónde sí** vale, que es
/// lo que faltó en la campaña: el diagnóstico llegaba como un `false` mudo.
fn error_resultado_fuera_de_asigna(paso: &str, campo_yaml: &str, campo: &str) -> ErrorCarga {
    ErrorCarga::Validacion(format!(
        "el paso '{paso}' usa 'result.{campo}' en '{campo_yaml}', donde no \
         está disponible: 'result.*' sólo es visible dentro del 'assign' del \
         propio paso, porque es el resultado que ese paso acaba de devolver. \
         Fuera de ahí valdría siempre 'nothing'. Vuelca lo que necesites a un \
         local con 'assign' y léelo desde '{campo_yaml}'"
    ))
}

/// DEF-3 del informe de beta: `asigna` escribe siempre en Locals (ADR-0009).
/// Si su destino coincide con el nombre de un `parameter` declarado, el
/// motor no avisa: crea un local homónimo, el `parameter` conserva su valor
/// inicial y el retorno by-reference de un sequence call devuelve **ese**
/// valor inicial al padre — un verde falso, sin ningún indicio en el YAML ni
/// en la carga. La misma clase de fallo silencioso la produce un **typo** en
/// el destino: se crea el local mal escrito, el declarado se queda con su
/// valor inicial, y quien lo lea después decide con el dato equivocado.
///
/// Regla, fail-fast al cargar (simétrica con `validar_call` para los
/// argumentos de un sequence call, más abajo):
/// - destino de `asigna` que coincide con un `parameter` declarado → error.
/// - destino de `asigna` no declarado en `locals` → error.
/// - lvalue de `statement` (`locals.X`/`parameters.P`) no declarado en su
///   scope → error.
///
/// Recorre `def` ya traducida (AST, no texto). Se invoca una vez por cada
/// nivel de `secuencia_yaml_a_definicion` (raíz, y cada inline por su propia
/// llamada recursiva), así que **no** hace falta bajar aquí a
/// `def.subsecuencias`; cada archivo externo tiene su propia llamada a
/// `cargar_de_texto`.
fn validar_lvalues(def: &DefinicionSecuencia) -> Result<(), ErrorCarga> {
    for p in def
        .pasos_setup
        .iter()
        .chain(&def.pasos_main)
        .chain(&def.pasos_cleanup)
    {
        if let Some(asignaciones) = &p.asigna {
            for a in asignaciones {
                if def.parameters.contains_key(&a.var) {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{}' asigna a '{}', declarado en 'parameters' de \
                         la secuencia '{}'. 'asigna' escribe siempre en locals y \
                         crearía un local homónimo que lo ensombrece: usa un paso \
                         'tipo: statement' con 'parameters.{} = …' si quieres \
                         devolver el valor al llamador",
                        p.nombre, a.var, def.nombre, a.var
                    )));
                }
                if !def.locals.contains_key(&a.var) {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{}' asigna a '{}', no declarado en 'locals' de \
                         la secuencia '{}'. Decláralo en 'locals:' con su valor \
                         inicial",
                        p.nombre, a.var, def.nombre
                    )));
                }
            }
        }
        if let Some(stmts) = &p.statement {
            for s in stmts {
                let expr::Sentencia::Assign { scope, campo, .. } = s;
                // `file_globals` son constantes del fichero: el motor rechaza
                // escribirlas siempre, en cualquier secuencia
                // (`EntornoMotor::escribe`, «sólo locals»). Que sea decidible
                // sin ejecutar y aun así muriera a mitad de la corrida es el
                // issue #17.
                if *scope == expr::Scope::FileGlobals {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{}' tiene un statement que escribe en \
                         'file_globals.{campo}', y 'file_globals' es de sólo lectura: \
                         son las constantes del fichero, y ninguna secuencia las muta. \
                         Si necesitas un valor que cambie, decláralo en 'locals:' de la \
                         secuencia '{}'",
                        p.nombre, def.nombre
                    )));
                }
                let declarado = match scope {
                    expr::Scope::Locals => def.locals.contains_key(campo),
                    expr::Scope::Parameters => def.parameters.contains_key(campo),
                    // FileGlobals ya salió arriba; Resultado lo rechaza entero
                    // `validar_alcance_resultado`, que corre justo después.
                    _ => true,
                };
                if !declarado {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{}' tiene un statement que escribe en \
                         '{}.{campo}', no declarado en '{}' de la secuencia '{}'. \
                         Decláralo con su valor inicial",
                        p.nombre,
                        scope.nombre(),
                        scope.nombre(),
                        def.nombre
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Issue #19: `validar_lvalues` mira **dónde se escribe**, y nadie miraba
/// dónde se lee. `--validate` aprobaba `precondicion: 'locals.no_existe > 0.0'`
/// y la corrida moría a mitad con «no existe 'locals.no_existe'», con la mitad
/// del test ya ejecutada sobre la unidad. El manual promete que los tres scopes
/// son estrictos y que leer algo no declarado es «un error de carga»: esto lo
/// hace verdad.
///
/// Se valida contra las declaraciones de **la propia** secuencia, que es
/// exactamente la tabla que verá el runtime: `EntornoMotor::desde_definicion*`
/// materializa `locals`/`file_globals` de su `DefinicionSecuencia`, y los
/// `parameters` de una subsecuencia los fija `validar_call` exigiendo igualdad
/// de claves. No hay herencia de scopes, así que no hay falso positivo posible.
///
/// **Fuera de alcance a propósito**: el chequeo de tipos (`bool * número`).
/// El nombre inexistente es decidible al cargar; el tipo no lo es sin evaluar,
/// y el evaluador ya lo trata como `error` de ejecución (ADR-0019, Regla 2).
///
/// `parametros` de un `sequence_call` no se toca aquí: `validar_call` ya
/// comprueba que cada `origen` esté en los `locals` del padre, y duplicarlo
/// cambiaría qué error se dispara primero.
///
/// **El orden importa**: esta validación va la **última** de las cuatro. Varias
/// secuencias mal escritas caen en más de una regla a la vez (leer un local no
/// declarado *y* usar `resultado.*` donde no vale), y el diagnóstico útil es el
/// de `resultado.*`, que explica un malentendido; el nombre no declarado es un
/// typo. Adelantarla tapa el mensaje bueno.
fn validar_variables_leidas(def: &DefinicionSecuencia) -> Result<(), ErrorCarga> {
    for p in def
        .pasos_setup
        .iter()
        .chain(&def.pasos_main)
        .chain(&def.pasos_cleanup)
    {
        // (nombre del campo YAML, expresión) — el mensaje cita el campo tal y
        // como el usuario lo escribió, igual que `validar_alcance_resultado`.
        let mut donde: Vec<(&str, &expr::Expresion)> = Vec::new();
        if let Some(pre) = &p.precondicion {
            donde.push(("precondition", pre));
        }
        if let Some(cond) = &p.condicion {
            donde.push(("condition", cond));
        }
        // Sólo el lado derecho: los destinos los valida `validar_lvalues`.
        if let Some(stmts) = &p.statement {
            for expr::Sentencia::Assign { valor, .. } in stmts {
                donde.push(("statement", valor));
            }
        }
        if let Some(asignaciones) = &p.asigna {
            for a in asignaciones {
                donde.push(("assign", &a.expr));
            }
        }
        for (campo_yaml, e) in donde {
            if let Some((scope, campo)) = primera_var_no_declarada(e, def) {
                let s = scope.nombre();
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' lee '{s}.{campo}' en '{campo_yaml}', y '{campo}' no \
                     está declarado en '{s}:' de la secuencia '{}'. En ejecución esto \
                     no es una variable nueva: la corrida muere a mitad con «no existe \
                     '{s}.{campo}'». Decláralo en '{s}:' con su valor inicial, o \
                     corrige el nombre",
                    p.nombre, def.nombre
                )));
            }
        }
    }
    Ok(())
}

/// La primera `scope.campo` de la expresión que no esté declarada en `def`, o
/// `None`. Mismo recorrido recursivo que `primer_uso_de_resultado_si`: mirar
/// sólo la raíz no basta, el caso real de la campaña era una conjunción.
///
/// `Scope::Resultado` se **salta**: dónde vale lo decide
/// `validar_alcance_resultado` y qué campos tiene, `validar_campos_de_resultado`.
fn primera_var_no_declarada<'a>(
    e: &'a expr::Expresion,
    def: &DefinicionSecuencia,
) -> Option<(expr::Scope, &'a str)> {
    match e {
        expr::Expresion::Var {
            scope: expr::Scope::Resultado,
            ..
        } => None,
        expr::Expresion::Var { scope, campo } => {
            let declarado = match scope {
                expr::Scope::Locals => def.locals.contains_key(campo),
                expr::Scope::Parameters => def.parameters.contains_key(campo),
                expr::Scope::FileGlobals => def.file_globals.contains_key(campo),
                expr::Scope::Resultado => true,
            };
            if declarado {
                None
            } else {
                Some((*scope, campo))
            }
        }
        expr::Expresion::Lit(_) => None,
        expr::Expresion::BinOp { izq, der, .. } => {
            primera_var_no_declarada(izq, def).or_else(|| primera_var_no_declarada(der, def))
        }
        expr::Expresion::UnOp { operando, .. } => primera_var_no_declarada(operando, def),
    }
}

/// La otra mitad del issue #17. `parameters` es entrada/salida **by-reference
/// de una llamada**: el motor sólo los deja mutar cuando la secuencia se está
/// ejecutando como subsecuencia, con los argumentos del padre enlazados
/// (`EntornoMotor::desde_definicion_con_args`, `parameters_mutables`). La
/// secuencia raíz del programa no la llama nadie, así que sus `parameters` no
/// tienen a quién devolver nada: escribirlos muere en runtime con «sólo
/// locals».
///
/// Escribirlos **desde una subsecuencia sí es legítimo** — es el modo
/// documentado de devolver un valor al llamador (ADR-0010) — así que esto se
/// comprueba únicamente sobre la raíz del `Programa`, y no baja a sus
/// `subsecuencias` inline: una inline también se invoca por `sequence_call`.
fn validar_parameters_de_la_raiz(raiz: &DefinicionSecuencia) -> Result<(), ErrorCarga> {
    for p in raiz
        .pasos_setup
        .iter()
        .chain(&raiz.pasos_main)
        .chain(&raiz.pasos_cleanup)
    {
        let Some(stmts) = &p.statement else { continue };
        for expr::Sentencia::Assign { scope, campo, .. } in stmts {
            if *scope == expr::Scope::Parameters {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' tiene un statement que escribe en \
                     'parameters.{campo}', y '{}' es la secuencia raíz: sus \
                     'parameters' no los enlaza ningún llamador, así que escribirlos \
                     no devuelve nada a nadie y en ejecución falla con «sólo locals». \
                     Escribir 'parameters' sólo vale desde una subsecuencia, para \
                     devolver el valor al padre; en la raíz, usa 'locals:'",
                    p.nombre, raiz.nombre
                )));
            }
        }
    }
    Ok(())
}

/// Carga una secuencia desde un fichero YAML en disco.
pub fn cargar_de_archivo(ruta: &str) -> Result<DefinicionSecuencia, ErrorCarga> {
    let texto = std::fs::read_to_string(ruta)?;
    cargar_de_texto(&texto)
}

/// ¿Es `destino` un path (archivo externo) y no un nombre (inline)?
/// Convención de M4b: si contiene `/` o `\`, o termina en `.yaml`/`.yml` →
/// path relativo al directorio del archivo que lo referencia; si no, es un
/// nombre de subsecuencia inline del mismo archivo.
pub fn es_path(destino: &str) -> bool {
    let t = destino.trim();
    t.contains('/') || t.contains('\\') || t.ends_with(".yaml") || t.ends_with(".yml")
}

/// Directorio que contiene a `ruta` (su `parent`), o "" si no tiene.
fn dir_de(ruta: &str) -> PathBuf {
    Path::new(ruta)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf()
}

/// Normaliza un path relativo a `base` resolviendo `.` y `..` de forma
/// lógica (sin IO, sin resolver symlinks): la clave canónica estable para
/// `programa.archivos` y para detectar ciclos.
pub fn normalizar_path(base: &Path, rel: &Path) -> PathBuf {
    let mut out = if rel.is_absolute() {
        PathBuf::new()
    } else {
        base.to_path_buf()
    };
    for comp in rel.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::Normal(p) => out.push(p),
            std::path::Component::RootDir => out = PathBuf::from("/"),
            std::path::Component::Prefix(p) => out = PathBuf::from(p.as_os_str()),
        }
    }
    out
}

/// Lee la sección `ejecutores:` de un YAML y la acumula en `acc` (M5-ext.1,
/// RF-36.3). Se re-lee el fichero porque la `DefinicionSecuencia` no lleva esa
/// sección: es dato del `Programa`. Un nombre repetido —dentro del archivo o
/// contra lo ya acumulado (PM + secuencia del usuario)— es error (fail-fast).
fn leer_ejecutores(
    ruta: &str,
    dir_base: &Path,
    acc: &mut HashMap<String, DefinicionEjecutor>,
) -> Result<(), ErrorCarga> {
    let texto = std::fs::read_to_string(ruta)?;
    let yaml: SecuenciaYaml = match noyalib::from_str(&texto) {
        Ok(y) => y,
        Err(e) => return Err(diagnostica_campo_desconocido(&texto, e)),
    };
    for y in yaml.executors {
        let def = y.a_definicion(dir_base)?;
        if acc.contains_key(&def.nombre) {
            return Err(ErrorCarga::Validacion(format!(
                "el ejecutor '{}' está declarado más de una vez en 'ejecutores:'",
                def.nombre
            )));
        }
        acc.insert(def.nombre.clone(), def);
    }
    Ok(())
}

/// Carga un **programa** desde un fichero YAML en disco (M4b, RF-27): la
/// secuencia raíz más todas las subsecuencias de **archivos externos**
/// referenciadas por path, ya resueltas y validadas.
///
/// Hace tres cosas (fail-fast, antes de ejecutar nada):
///
/// 1. **Carga** la raíz y, recursivamente, los archivos externos a los que
///    apuntan los `sequence_call` por path. Los paths se **reescriben** a su
///    clave canónica ([`normalizar_path`]) en cada `DefinicionPaso.secuencia`,
///    así el motor los resuelve con un mero `programa.archivos[clave]` (sin
///    conocer el sistema de ficheros, ADR-0005).
/// 2. **Valida** cada `sequence_call`: que el destino exista (inline por
///    nombre o archivo por path), que cada argumento `locals.X` esté
///    declarado en `locals` de la secuencia contenedora, y que la **firma**
///    encaje (claves de `parametros` == `parameters` de la subsecuencia).
/// 3. **Detecta ciclos** en el grafo de llamadas (por nombre inline o por
///    path): `A → B → A` es error.
pub fn cargar_programa_de_archivo(ruta: &str) -> Result<Programa, ErrorCarga> {
    let raiz = cargar_de_archivo(ruta)?;
    let dir_base = dir_de(ruta);

    // M5-ext.1 (RF-36.3): la tabla de ejecutores declarada en `ejecutores:`
    // del YAML de la **raíz**. Se re-lee el fichero para esa sección (la
    // `DefinicionSecuencia` no la lleva: es dato del `Programa`). Nombres
    // duplicados → error (fail-fast).
    let mut ejecutores = HashMap::new();
    leer_ejecutores(ruta, &dir_base, &mut ejecutores)?;

    let mut programa = Programa {
        raiz,
        archivos: HashMap::new(),
        ejecutores,
    };

    // Cola de (clave_canónica, dir_contenedor) de archivos externos a cargar.
    let mut cola: Vec<(String, PathBuf)> = Vec::new();
    let mut cargados: HashSet<String> = HashSet::new();

    // Fase A: reescribir paths de la raíz a claves canónicas y encolar archivos.
    procesar_secuencia(&mut programa.raiz, &dir_base, &mut cola)?;
    while let Some((clave, _dir_cont)) = cola.pop() {
        if cargados.contains(&clave) {
            continue;
        }
        cargados.insert(clave.clone());
        let path = PathBuf::from(&clave);
        let texto = std::fs::read_to_string(&path)?;
        let mut sub = cargar_subsecuencia_externa(&texto, &clave)?;
        let dir_sub = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        // Reescribir paths de la subsecuencia externa y encolar los suyos.
        procesar_secuencia(&mut sub, &dir_sub, &mut cola)?;
        programa.archivos.insert(clave, sub);
    }

    // Fase B: validar lvalues, firmas y nombres; detectar ciclos.
    let id_raiz = normalizar_path(&dir_base, Path::new(ruta))
        .to_string_lossy()
        .into_owned();
    let mut camino: Vec<String> = Vec::new();
    validar_parameters_de_la_raiz(&programa.raiz)?;
    visitar(&programa, &id_raiz, &programa.raiz, &mut camino)?;
    validar_referencias(&programa)?;

    Ok(programa)
}

/// Where a step is dispatched, resolved from what the YAML declares.
///
/// It lives here and not in the engine because **two** things need the answer
/// and they must not disagree: the engine, to open the right connection, and
/// this loader, to refuse a reference handed to a step of another executor
/// before anything is energised (ADR-0022 §3). Two copies of the routing rule
/// is how the check and the dispatch drift apart, and the drift would show up
/// as a sequence that validates and then measures against the wrong bench.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint<'a> {
    /// The built-in WASM executor. **Every** `type: embedded` collapses here:
    /// they are all the same process, whatever the sequence calls them.
    Embebido,
    /// A gRPC executor, keyed by the name the YAML gave it.
    Grpc(&'a str),
    /// A `type: wasm` executor. The engine never runs one — the host
    /// translates it to `grpc` first (ADR-0014).
    Wasm(&'a str),
    /// A name no `executors:` entry declares.
    NoDeclarado(&'a str),
}

impl<'a> Endpoint<'a> {
    /// The routing key: what the engine keys its connections on, and what two
    /// steps must share to be talking to the same process.
    pub fn clave(&self) -> &'a str {
        match self {
            Endpoint::Embebido => NOMBRE_EMBEDIDO_RESERVADO,
            Endpoint::Grpc(n) | Endpoint::Wasm(n) | Endpoint::NoDeclarado(n) => n,
        }
    }
}

/// Resolves a step's `executor:` (or its absence) against the program's table.
pub fn resolver_endpoint<'a>(
    ejecutor: Option<&'a str>,
    ejecutores: &HashMap<String, DefinicionEjecutor>,
) -> Endpoint<'a> {
    let Some(nombre) = ejecutor else {
        return Endpoint::Embebido;
    };
    match ejecutores.get(nombre).map(|e| &e.tipo) {
        Some(TipoEjecutor::Embebido) => Endpoint::Embebido,
        Some(TipoEjecutor::Grpc { .. }) => Endpoint::Grpc(nombre),
        Some(TipoEjecutor::Wasm { .. }) => Endpoint::Wasm(nombre),
        None => Endpoint::NoDeclarado(nombre),
    }
}

/// How an endpoint is named to a human. `__anvil_embebido__` is an internal
/// key nobody can declare, so printing it would be printing plumbing.
pub fn nombre_visible_de_endpoint(clave: &str) -> &str {
    modelo::nombre_visible_de_ejecutor(clave)
}

/// Everything about object references that can be decided by reading the files
/// (ADR-0022 §3, and its §Consequences on `--validate`).
///
/// It runs over the **whole program** because that is the smallest unit that
/// has both halves of the question: a `locals:` declaration lives in one
/// sequence, and the `executors:` table it names lives in the root of the
/// file. None of this needs an executor to be up, which is the point — the
/// mode a person would name by default (`--validate`, no `--with-executors`)
/// has no catalog, and these checks do not want one.
///
/// What it refuses, and why each one is worth a refusal:
///
/// 1. **A declaration naming an executor that does not exist.** A typo in the
///    executor's name would otherwise make the cross-executor check below
///    vacuous, and vacuously passing is worse than not checking.
/// 2. **A reference from a `wasm` executor.** `anvil:step@0.3.0` is
///    `run(name, attempt, inputs)` — a function, with no resources and no
///    state between calls — so the component has nowhere to keep the map
///    (ADR-0022 §8). Refusing here, at load, is the explicit error the ADR
///    asks for; the bridge refuses again at run time, for the case that gets
///    there another way.
/// 3. **A handle handed to a step of another executor.** The one problem
///    TestStand cannot even pose, because everything there is one process. It
///    is checked on the two ends a handle has: the `inputs:` that spend it and
///    the `assign` that fills it.
/// 4. **A reference variable written by anything but an `assign` on a `grpc`
///    step**, and from anything but `result.outputs.<name>`. Nothing else can
///    produce a handle: a `statement` computes, and `result.measured_value` is
///    a number. Allowing either would let a number end up in a variable the
///    file says is a handle, and the type would be a label rather than a fact.
fn validar_referencias(programa: &Programa) -> Result<(), ErrorCarga> {
    validar_referencias_de(&programa.raiz, programa)?;
    for sec in programa.archivos.values() {
        validar_referencias_de(sec, programa)?;
    }
    Ok(())
}

fn validar_referencias_de(
    def: &DefinicionSecuencia,
    programa: &Programa,
) -> Result<(), ErrorCarga> {
    // (1) and (2): the declarations themselves.
    for (nombre, decl) in &def.locals {
        let Some(ejecutor) = decl.ejecutor_de_referencia() else {
            continue;
        };
        match resolver_endpoint(Some(ejecutor), &programa.ejecutores) {
            Endpoint::NoDeclarado(_) => {
                return Err(ErrorCarga::Validacion(format!(
                    "'locals.{nombre}' de la secuencia '{}' declara una referencia del \
                     ejecutor '{ejecutor}', que no está en 'executors:' de la secuencia raíz",
                    def.nombre
                )))
            }
            Endpoint::Wasm(_) => {
                return Err(ErrorCarga::Validacion(format!(
                    "'locals.{nombre}' de la secuencia '{}' declara una referencia del \
                     ejecutor '{ejecutor}', que es 'type: wasm'. Un componente WASM no puede \
                     sostener una referencia: su interfaz es una función sin estado entre \
                     llamadas (anvil:step, ADR-0020 §4d), así que no tiene dónde guardar el \
                     objeto. Sírvelo como ejecutor 'grpc' (ADR-0022 §8)",
                    def.nombre
                )))
            }
            _ => {}
        }
    }

    let refs = |campo: &str| -> Option<&str> {
        def.locals
            .get(campo)
            .and_then(|d| d.ejecutor_de_referencia())
    };

    for p in def
        .pasos_setup
        .iter()
        .chain(&def.pasos_main)
        .chain(&def.pasos_cleanup)
    {
        let destino = resolver_endpoint(p.ejecutor.as_deref(), &programa.ejecutores);

        // (3a) `inputs: { rack: '${locals.rack}' }` — the handle being spent.
        for (entrada, valor) in p.entradas.iter().flatten() {
            let EntradaPaso::Expresion(expr::Expresion::Var {
                scope: expr::Scope::Locals,
                campo,
            }) = valor
            else {
                continue;
            };
            let Some(duenio) = refs(campo) else { continue };
            let origen = resolver_endpoint(Some(duenio), &programa.ejecutores);
            if origen.clave() != destino.clave() {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' de la secuencia '{}' se despacha al ejecutor '{}' y le \
                     pasa en '{entrada}' la referencia 'locals.{campo}', que es del ejecutor \
                     '{duenio}'. Una referencia sólo significa algo dentro del ejecutor que \
                     la acuñó: en cualquier otro no es más que una cadena que no casa \
                     (ADR-0022 §3)",
                    p.nombre,
                    def.nombre,
                    nombre_visible_de_endpoint(destino.clave()),
                )));
            }
        }

        // (3b) and (4): the `assign` that fills a reference variable.
        for a in p.asigna.iter().flatten() {
            let Some(duenio) = refs(&a.var) else { continue };
            if p.tipo != TipoPaso::Grpc {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' de la secuencia '{}' asigna a 'locals.{}', que se declara \
                     como referencia, y no es un paso de ejecutor. Sólo un paso servido por \
                     un ejecutor puede acuñar una referencia (ADR-0022 §4)",
                    p.nombre, def.nombre, a.var
                )));
            }
            let origen = resolver_endpoint(Some(duenio), &programa.ejecutores);
            if origen.clave() != destino.clave() {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' de la secuencia '{}' se despacha al ejecutor '{}' y su \
                     'assign' escribe en 'locals.{}', declarada como referencia del ejecutor \
                     '{duenio}'. La referencia que acuñe este paso sería del ejecutor \
                     equivocado (ADR-0022 §3)",
                    p.nombre,
                    def.nombre,
                    nombre_visible_de_endpoint(destino.clave()),
                    a.var
                )));
            }
            let es_salida = matches!(
                &a.expr,
                expr::Expresion::Var {
                    scope: expr::Scope::Resultado,
                    campo,
                } if campo.strip_prefix(expr::CAMPO_SALIDAS).and_then(|r| r.strip_prefix('.')).is_some_and(|n| !n.is_empty())
            );
            if !es_salida {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' de la secuencia '{}' escribe en 'locals.{}', declarada \
                     como referencia, algo que no es una salida del paso. Una referencia \
                     sólo la acuña el ejecutor y sólo llega por 'result.outputs.<nombre>': \
                     cualquier otro campo de 'result' es un número o un texto, y acabaría en \
                     una variable que el fichero dice que es una referencia (ADR-0022 §1)",
                    p.nombre, def.nombre, a.var
                )));
            }
        }

        // (4, the other half): a `statement` cannot mint a handle either.
        for s in p.statement.iter().flatten() {
            let expr::Sentencia::Assign { scope, campo, .. } = s;
            if *scope == expr::Scope::Locals && refs(campo).is_some() {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' de la secuencia '{}' tiene un statement que escribe en \
                     'locals.{campo}', declarada como referencia. Un statement calcula, y \
                     una referencia no se calcula: sólo la acuña un ejecutor y sólo llega \
                     por el 'assign' de un paso suyo (ADR-0022 §1)",
                    p.nombre, def.nombre
                )));
            }
        }
    }

    for sub in def.subsecuencias.values() {
        validar_referencias_de(sub, programa)?;
    }
    Ok(())
}

/// Nombre reservado en la **raíz** de un process model para el
/// `sequence_call` que invoca a la secuencia del usuario (M5, RF-38). El PM
/// es genérico y no sabe qué secuencia va a correr; la ruta la da el CLI.
/// El PM autora `secuencia: secuencia_usuario` (un **nombre**: `es_path()`
/// es falso, así [`procesar_secuencia`] lo deja intacto) y
/// [`cargar_programa_con_pm`] lo **reescribe** al path canónico de la
/// secuencia inyectada. El motor **nunca** ve el placeholder: tras la
/// reescritura es un path normal que [`ejecuta_sequence_call`] resuelve en
/// `programa.archivos` como cualquier subsecuencia externa (ADR-0005/0010).
pub const SECUENCIA_USUARIO: &str = "secuencia_usuario";

/// Carga un **programa con process model** (M5, RF-38): el PM (`ruta_pm`) es
/// la raíz del `Programa`; su `main` lleva un `sequence_call` al nombre
/// reservado [`SECUENCIA_USUARIO`], que este cargador reescribe al path
/// canónico de la secuencia del usuario (`ruta_usuario`) y registra en
/// `programa.archivos`.
///
/// Reusa el pipeline de M4b ([`procesar_secuencia`] + [`visitar`]) sobre el
/// programa entero (PM + usuario + subsecuencias externas de ambos). El
/// motor **no cambia**: tras la reescritura, `ejecuta_sequence_call`
/// resuelve el call por path en `programa.archivos` como hoy. La misma
/// secuencia del usuario puede correrse con distintos PMs (R&D vs fábrica)
/// cambiando sólo `--process-model`.
///
/// Reglas (fail-fast al cargar):
/// 1. La raíz del PM tiene **exactamente un** `sequence_call` con
///    `secuencia: secuencia_usuario` en `main`; cero o más de uno → error.
/// 2. `secuencia_usuario` no aparece en `subsecuencias` de la raíz del PM
///    (nombre reservado). El usuario puede usar ese nombre en sus propias
///    inline sin conflicto (alcance distinto).
/// 3. La secuencia del usuario se carga y registra en `programa.archivos`
///    bajo `normalizar(dir_de(ruta_usuario), ruta_usuario)` (la misma clave
///    que usaría [`cargar_programa_de_archivo`] para ella).
/// 4. El `secuencia` del placeholder se reescribe a esa clave **después**
///    de `procesar_secuencia` (si se hiciera antes, `procesar_secuencia`
///    renormalizaría la clave relativa al directorio del PM y la rompería).
/// 5. `visitar` sobre el programa entero: ciclos, firma (`validar_call`) y
///    lvalues. El PM canónico declara sin `parametros`, así exige que la
///    raíz del usuario no declare `parameters`; un PM custom puede
///    emparejar `parametros` ↔ `parameters`.
pub fn cargar_programa_con_pm(ruta_pm: &str, ruta_usuario: &str) -> Result<Programa, ErrorCarga> {
    let raiz_pm = cargar_de_archivo(ruta_pm)?;
    let dir_pm = dir_de(ruta_pm);

    // (1+2) Hallar el call placeholder y rechazar el nombre reservado en
    // subsecuencias inline de la raíz del PM.
    let call_idx = indice_call_secuencia_usuario(&raiz_pm)?;
    if raiz_pm.subsecuencias.contains_key(SECUENCIA_USUARIO) {
        return Err(ErrorCarga::Validacion(format!(
            "el nombre '{SECUENCIA_USUARIO}' está reservado para la secuencia \
             del usuario y no puede declararse como subsecuencia inline del PM"
        )));
    }

    // (3) Cargar la secuencia del usuario y registrarla en archivos. La
    // clave canónica es la ruta del usuario tal cual la da el CLI (relativa
    // al cwd del proceso), normalizada (resolviendo `.`/`..`) **sin
    // anteponer su directorio**: `ruta_usuario` ya va expresada relativa al
    // cwd, así que `normalizar(dir_de(ruta_usuario), ruta_usuario)` la
    // duplicaría (`ejemplos/` + `ejemplos/basica.yaml`). Las subsecuencias
    // externas del usuario sí se normalizan relativas a `dir_de(ruta_usuario)`
    // (paso 5a), que es como se leen del disco.
    let usuario = cargar_de_archivo(ruta_usuario)?;
    let dir_usuario = dir_de(ruta_usuario);
    let clave_usuario = normalizar_path(Path::new(""), Path::new(ruta_usuario))
        .to_string_lossy()
        .into_owned();

    // Ejecutores (M5-ext.1): los del PM y los de la secuencia del usuario se
    // unen en una sola tabla; un nombre en ambos es error (leer_ejecutores).
    let mut ejecutores = HashMap::new();
    leer_ejecutores(ruta_pm, &dir_pm, &mut ejecutores)?;
    leer_ejecutores(ruta_usuario, &dir_usuario, &mut ejecutores)?;

    let mut programa = Programa {
        raiz: raiz_pm,
        archivos: HashMap::new(),
        ejecutores,
    };
    programa.archivos.insert(clave_usuario.clone(), usuario);

    // (5a) Procesar paths externos del PM (relativos a dir_pm) y del usuario
    // (relativos a dir_usuario). El placeholder `secuencia_usuario` no es
    // path → queda intacto en este paso.
    let mut cola: Vec<(String, PathBuf)> = Vec::new();
    let mut cargados: HashSet<String> = HashSet::new();
    procesar_secuencia(&mut programa.raiz, &dir_pm, &mut cola)?;
    let usuario_def = programa
        .archivos
        .get_mut(&clave_usuario)
        .expect("acabamos de insertar la secuencia del usuario");
    procesar_secuencia(usuario_def, &dir_usuario, &mut cola)?;
    while let Some((clave, _dir_cont)) = cola.pop() {
        if cargados.contains(&clave) {
            continue;
        }
        cargados.insert(clave.clone());
        let path = PathBuf::from(&clave);
        let texto = std::fs::read_to_string(&path)?;
        let mut sub = cargar_subsecuencia_externa(&texto, &clave)?;
        let dir_sub = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        procesar_secuencia(&mut sub, &dir_sub, &mut cola)?;
        programa.archivos.insert(clave, sub);
    }

    // (4) Reescribir el placeholder a la clave canónica del usuario, ya
    // **después** de procesar_secuencia (ver regla 4 del docstring).
    programa.raiz.pasos_main[call_idx].secuencia = Some(clave_usuario.clone());

    // (5b) Visitar el programa entero: ciclos + firma + lvalues. El call al
    // usuario ya es un path normal y se valida como cualquier externa.
    let id_pm = normalizar_path(&dir_pm, Path::new(ruta_pm))
        .to_string_lossy()
        .into_owned();
    let mut camino: Vec<String> = Vec::new();
    // Bajo un PM la raíz es el PM: la secuencia del usuario sí se invoca por
    // `sequence_call`, así que sus `parameters` son legítimamente mutables.
    validar_parameters_de_la_raiz(&programa.raiz)?;
    visitar(&programa, &id_pm, &programa.raiz, &mut camino)?;
    validar_referencias(&programa)?;

    Ok(programa)
}

/// Halla el índice del `sequence_call` con `secuencia == secuencia_usuario`
/// en `main` de la raíz del PM. Exactamente uno; si no hay o hay más, error.
fn indice_call_secuencia_usuario(pm: &DefinicionSecuencia) -> Result<usize, ErrorCarga> {
    let mut found: Option<usize> = None;
    for (i, p) in pm.pasos_main.iter().enumerate() {
        if p.tipo == TipoPaso::SequenceCall && p.secuencia.as_deref() == Some(SECUENCIA_USUARIO) {
            if found.is_some() {
                return Err(ErrorCarga::Validacion(format!(
                    "el process model declara más de un sequence_call a '{SECUENCIA_USUARIO}'"
                )));
            }
            found = Some(i);
        }
    }
    found.ok_or_else(|| {
        ErrorCarga::Validacion(format!(
            "el process model no declara un sequence_call a '{SECUENCIA_USUARIO}' en su main"
        ))
    })
}
/// cada `sequence_call` por path, reescribe su `secuencia` a la clave
/// canónica y encola el archivo para cargarlo. Las inline (por nombre) se
/// dejan tal cual: el motor las resuelve en `def.subsecuencias`.
fn procesar_secuencia(
    def: &mut DefinicionSecuencia,
    dir: &Path,
    cola: &mut Vec<(String, PathBuf)>,
) -> Result<(), ErrorCarga> {
    for paso in def
        .pasos_setup
        .iter_mut()
        .chain(&mut def.pasos_main)
        .chain(&mut def.pasos_cleanup)
    {
        if paso.tipo == TipoPaso::SequenceCall {
            if let Some(sec) = paso.secuencia.as_ref() {
                if es_path(sec) {
                    let path_dest = normalizar_path(dir, Path::new(sec));
                    let clave = path_dest.to_string_lossy().into_owned();
                    cola.push((
                        clave.clone(),
                        path_dest
                            .parent()
                            .unwrap_or_else(|| Path::new(""))
                            .to_path_buf(),
                    ));
                    *paso.secuencia.as_mut().unwrap() = clave;
                }
            }
        }
    }
    for sub in def.subsecuencias.values_mut() {
        procesar_secuencia(sub, dir, cola)?;
    }
    Ok(())
}

/// DFS de validación sobre el grafo de llamadas. `id` identifica al nodo
/// (path canónico de un archivo, o `{id_archivo}::{nombre_inline}`).
/// `camino` lleva los ids en curso para detectar `A → B → A`.
fn visitar(
    programa: &Programa,
    id: &str,
    def: &DefinicionSecuencia,
    camino: &mut Vec<String>,
) -> Result<(), ErrorCarga> {
    if camino.iter().any(|c| c == id) {
        let mut trail = camino.join(" → ");
        trail.push_str(" → ");
        trail.push_str(id);
        return Err(ErrorCarga::Validacion(format!(
            "ciclo de subsecuencias: {trail}"
        )));
    }
    camino.push(id.to_string());
    for paso in def
        .pasos_setup
        .iter()
        .chain(&def.pasos_main)
        .chain(&def.pasos_cleanup)
    {
        // M5-ext.1 (RF-36.3): un paso con `ejecutor:` debe referenciar un
        // nombre declarado en `ejecutores:` del YAML de la raíz (fail-fast).
        // `a_definicion` ya garantizó que sólo un paso `grpc` lo trae.
        if let Some(nombre) = paso.ejecutor.as_deref() {
            if !programa.ejecutores.contains_key(nombre) {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' de la secuencia '{}' referencia el ejecutor \
                     '{nombre}' que no está en 'ejecutores:'. La tabla se declara \
                     **una sola vez**, en la secuencia raíz (la que se pasa a anvil; \
                     con --process-model, también en el process model): declararlo en \
                     la subsecuencia no vale, porque Anvil no lee esa sección fuera \
                     de la raíz",
                    paso.nombre, def.nombre
                )));
            }
        }
        if paso.tipo != TipoPaso::SequenceCall {
            continue;
        }
        let secuencia = paso.secuencia.as_ref().expect("validado en a_definicion");
        if es_path(secuencia) {
            let sub = programa.archivos.get(secuencia).ok_or_else(|| {
                ErrorCarga::Validacion(format!(
                    "el sequence call '{}' referencia '{secuencia}' que no se resolvió al cargar",
                    paso.nombre
                ))
            })?;
            validar_call(paso, def, sub, secuencia)?;
            visitar(programa, secuencia, sub, camino)?;
        } else {
            let sub = def.subsecuencias.get(secuencia).ok_or_else(|| {
                ErrorCarga::Validacion(format!(
                    "el sequence call '{}' referencia la subsecuencia inline '{secuencia}' que no existe",
                    paso.nombre
                ))
            })?;
            validar_call(paso, def, sub, secuencia)?;
            let id_sub = format!("{id}::{secuencia}");
            visitar(programa, &id_sub, sub, camino)?;
        }
    }
    camino.pop();
    Ok(())
}

/// Valida un `sequence_call` contra la firma de su subsecuencia: que cada
/// argumento `locals.X` esté declarado en `locals` de la secuencia
/// contenedora (`padre`) y que las claves de `parametros` coincidan con las
/// de `parameters` de la subsecuencia (`sub`).
fn validar_call(
    paso: &DefinicionPaso,
    padre: &DefinicionSecuencia,
    sub: &DefinicionSecuencia,
    destino: &str,
) -> Result<(), ErrorCarga> {
    let args: Vec<&Argumento> = paso
        .parametros
        .as_ref()
        .map(|v| v.iter().collect())
        .unwrap_or_default();
    // Lvalues: la forma `Var{Locals, campo}` ya se validó en `a_definicion`;
    // aquí validamos que `campo` esté declarado en `locals` del padre.
    for a in &args {
        if let expr::Expresion::Var {
            scope: expr::Scope::Locals,
            campo,
        } = &a.origen
        {
            if !padre.locals.contains_key(campo) {
                return Err(ErrorCarga::Validacion(format!(
                    "el argumento '{}' del sequence call '{}' usa 'locals.{campo}', \
                     no declarado en locals de su secuencia",
                    a.param, paso.nombre
                )));
            }
        }
    }
    // Firma: claves de los argumentos == claves de `parameters` de la subsec.
    let claves_args: HashSet<&String> = args.iter().map(|a| &a.param).collect();
    let claves_sub: HashSet<&String> = sub.parameters.keys().collect();
    if claves_args != claves_sub {
        let faltan: Vec<&String> = claves_sub.difference(&claves_args).copied().collect();
        let sobran: Vec<&String> = claves_args.difference(&claves_sub).copied().collect();
        let mut detalles = Vec::new();
        if !faltan.is_empty() {
            detalles.push(format!("falta(n) {faltan:?}"));
        }
        if !sobran.is_empty() {
            detalles.push(format!("sobran {sobran:?}"));
        }
        return Err(ErrorCarga::Validacion(format!(
            "el sequence call '{}' no encaja con la firma de '{}' \
             (parameters: {:?}): {}",
            paso.nombre,
            destino,
            sub.parameters.keys().collect::<Vec<_>>(),
            detalles.join("; ")
        )));
    }
    Ok(())
}

/// Carga un **sidecar de límites** (RF-30): un YAML que asocia cada nombre de
/// paso a su `Limite`. Formato:
///
/// ```yaml
/// medir_voltaje:
///   tipo: rango
///   min: 4.5
///   max: 5.5
/// verificar_frecuencia:
///   tipo: comparacion
///   op: ge
///   esperado: 1000.0
/// ```
///
/// Cada entrada se valida con `LimiteYaml::a_limite` (mismas reglas que un
/// límite embebido). Lo inyecta [`aplicar_limites`].
pub fn cargar_limites_de_archivo(ruta: &str) -> Result<HashMap<String, Limite>, ErrorCarga> {
    let texto = std::fs::read_to_string(ruta)?;
    let mapa: HashMap<String, LimiteYaml> = match noyalib::from_str(&texto) {
        Ok(m) => m,
        // El sidecar es la entrada más fácil de escribir mal, y su error
        // genérico es el que más despistó en la beta (DIAG-5): antes de
        // rendirse, mirar si el fallo tiene una explicación concreta.
        Err(e) => return Err(diagnostica_sidecar(&texto, e)),
    };
    mapa.into_iter()
        .map(|(nombre, l)| Ok((nombre.clone(), l.a_limite(&nombre)?)))
        .collect()
}

/// Claves con las que un usuario envuelve el sidecar por costumbre de otros
/// formatos. Ninguna es válida: el sidecar es un mapa plano paso→límite.
const ENVOLTORIOS_DE_SIDECAR: [&str; 5] = ["limites", "límites", "limite", "limits", "pasos"];

/// Explica el fallo de un sidecar cuando se puede, en vez de dejar salir el
/// `unknown field: <nombre_del_paso>` de serde, que acusa al nombre del paso
/// —que está bien— en lugar de al envoltorio que lo esconde.
///
/// Solo corre en el camino de error, así que el segundo parseo (permisivo, a
/// `Value`) no cuesta nada en el camino feliz. Si no encuentra una explicación
/// concreta, devuelve el error original intacto.
///
/// El caso que motiva esto es real: en la beta, un sidecar envuelto en
/// `limites:` produjo el diagnóstico «el sidecar no funciona con process
/// model», que era falso, y de ahí salió un bug fantasma que costó días.
fn diagnostica_sidecar(texto: &str, original: noyalib::Error) -> ErrorCarga {
    let raiz: HashMap<String, noyalib::Value> = match noyalib::from_str(texto) {
        Ok(v) => v,
        Err(_) => return ErrorCarga::Sintaxis(original),
    };
    // Una sola clave de nivel superior, que es una palabra de envoltorio y
    // contiene un mapa: el usuario indentó todo el sidecar bajo ella.
    if raiz.len() == 1 {
        if let Some((clave, valor)) = raiz.iter().next() {
            if ENVOLTORIOS_DE_SIDECAR.contains(&clave.as_str()) && valor.is_mapping() {
                return ErrorCarga::Diagnostico(format!(
                    "el sidecar de límites es un mapa plano paso→límite; '{clave}:' en la raíz \
                     es un envoltorio que Anvil no admite (quita esa línea y desindenta el resto)"
                ));
            }
        }
    }
    ErrorCarga::Sintaxis(original)
}

/// Inyecta los límites del sidecar en la secuencia, buscando cada paso por
/// `nombre`. El sidecar **manda** sobre el límite embebido en la secuencia:
/// es el mecanismo de variabilidad por lote/variante (RF-30) — cambiar
/// umbrales en producción sin re-deploy de la secuencia.
///
/// Devuelve cuántos pasos recibieron un límite del sidecar (para que el CLI
/// lo informe). Un nombre del sidecar que no existe en la secuencia se
/// ignora (no es error: el sidecar puede cubrir más pasos de los que una
/// secuencia concreta usa).
pub fn aplicar_limites(
    secuencia: &mut DefinicionSecuencia,
    limites: &HashMap<String, Limite>,
) -> usize {
    let mut aplicados = 0;
    for paso in secuencia
        .pasos_setup
        .iter_mut()
        .chain(&mut secuencia.pasos_main)
        .chain(&mut secuencia.pasos_cleanup)
    {
        if let Some(lim) = limites.get(&paso.nombre) {
            paso.limite = Some(lim.clone());
            aplicados += 1;
        }
    }
    aplicados
}

/// Los nombres del sidecar que **no corresponden a ningún paso** de la
/// secuencia (RF-30, DIAG-1 del informe de beta). Un sidecar que no afecta a
/// nada es casi siempre un error del usuario —un nombre mal escrito, o el
/// sidecar apuntando a la secuencia equivocada— y sin esto falla en silencio:
/// los límites embebidos siguen en pie y la secuencia da un veredicto que no
/// es el que se pidió. Ordenados, para que el aviso sea estable.
pub fn limites_sin_aplicar(
    secuencia: &DefinicionSecuencia,
    limites: &HashMap<String, Limite>,
) -> Vec<String> {
    let nombres: HashSet<&str> = secuencia
        .pasos_setup
        .iter()
        .chain(&secuencia.pasos_main)
        .chain(&secuencia.pasos_cleanup)
        .map(|p| p.nombre.as_str())
        .collect();
    let mut sobran: Vec<String> = limites
        .keys()
        .filter(|n| !nombres.contains(n.as_str()))
        .cloned()
        .collect();
    sobran.sort();
    sobran
}

/// Inyecta los límites del sidecar en **todo el programa**: la raíz, las
/// subsecuencias de archivos externos y las inline de cada una. Devuelve
/// cuántos pasos recibieron un límite.
///
/// Aplicarlo sólo a la raíz era DEF-1 del informe de beta: con
/// `--process-model` la raíz es el **process model**
/// ([`cargar_programa_con_pm`]) y la secuencia del operador queda en
/// `archivos`, así que el sidecar no afectaba a nada —en silencio— justo en
/// el modo de producción, que es para el que existe (variar umbrales por
/// lote sin re-deploy, RF-30). El criterio es uniforme: **el sidecar casa
/// por nombre de paso en cualquier secuencia del programa**.
pub fn aplicar_limites_programa(
    programa: &mut Programa,
    limites: &HashMap<String, Limite>,
) -> usize {
    let mut aplicados = aplicar_limites_recursivo(&mut programa.raiz, limites);
    for sec in programa.archivos.values_mut() {
        aplicados += aplicar_limites_recursivo(sec, limites);
    }
    aplicados
}

/// [`aplicar_limites`] sobre una secuencia y sus subsecuencias **inline**
/// (las externas las recorre [`aplicar_limites_programa`] por `archivos`).
fn aplicar_limites_recursivo(
    secuencia: &mut DefinicionSecuencia,
    limites: &HashMap<String, Limite>,
) -> usize {
    let mut aplicados = aplicar_limites(secuencia, limites);
    for sub in secuencia.subsecuencias.values_mut() {
        aplicados += aplicar_limites_recursivo(sub, limites);
    }
    aplicados
}

/// Los nombres del sidecar que no corresponden a ningún paso de **ningún**
/// lugar del programa (DIAG-1 sobre el alcance de
/// [`aplicar_limites_programa`]). Ordenados, para que el aviso sea estable.
pub fn limites_sin_aplicar_programa(
    programa: &Programa,
    limites: &HashMap<String, Limite>,
) -> Vec<String> {
    let mut nombres: HashSet<&str> = HashSet::new();
    recoge_nombres_de_paso(&programa.raiz, &mut nombres);
    for sec in programa.archivos.values() {
        recoge_nombres_de_paso(sec, &mut nombres);
    }
    let mut sobran: Vec<String> = limites
        .keys()
        .filter(|n| !nombres.contains(n.as_str()))
        .cloned()
        .collect();
    sobran.sort();
    sobran
}

/// Nombres de todos los pasos de una secuencia y de sus inline.
fn recoge_nombres_de_paso<'a>(secuencia: &'a DefinicionSecuencia, nombres: &mut HashSet<&'a str>) {
    nombres.extend(
        secuencia
            .pasos_setup
            .iter()
            .chain(&secuencia.pasos_main)
            .chain(&secuencia.pasos_cleanup)
            .map(|p| p.nombre.as_str()),
    );
    for sub in secuencia.subsecuencias.values() {
        recoge_nombres_de_paso(sub, nombres);
    }
}

/// Reglas de negocio que el schema por sí solo no expresa. No revisa el
/// `nombre` (eso lo hace [`secuencia_yaml_a_definicion`] con su fallback) ni
/// las `subsecuencias` (las traduce/recorre la propia función llamadora).
fn validar(y: &SecuenciaYaml) -> Result<(), ErrorCarga> {
    if y.main.is_empty() {
        return Err(ErrorCarga::Validacion(
            "la sección 'main' es obligatoria y no puede estar vacía".into(),
        ));
    }
    for p in y.setup.iter().chain(&y.main).chain(&y.cleanup) {
        if p.retries == 0 {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' tiene 'retries' 0; el mínimo es 1",
                p.name
            )));
        }
        if p.name.trim().is_empty() {
            return Err(ErrorCarga::Validacion(
                "un paso tiene el 'name' vacío".into(),
            ));
        }
    }
    Ok(())
}

impl PasoYaml {
    // `mut self` porque `parametros` se consume por una vía o por la otra
    // según el `tipo` (ADR-0020): by-value en un `grpc`, by-reference en un
    // `sequence_call`. El `take()` deja claro cuál se ha llevado el mapa.
    fn a_definicion(mut self) -> Result<DefinicionPaso, ErrorCarga> {
        let limite = match self.limit {
            Some(l) => Some(l.a_limite(&self.name)?),
            None => None,
        };

        // RF-33: la precondición se parsea a AST aquí (fail-fast). Un error de
        // sintaxis se reporta con el nombre del paso.
        let precondicion = match self.precondition.as_deref() {
            Some(texto) => Some(expr::parse_expresion(extraer_expr(texto)).map_err(|e| {
                ErrorCarga::Validacion(format!(
                    "precondición del paso '{}' inválida: {e}",
                    self.name
                ))
            })?),
            None => None,
        };

        // RF-31: cada `asigna` es `nombre_local -> expr`. La expr se evalúa
        // sobre `resultado`/scopes y el motor la vuelca a Locals.
        let asigna = match self.assign {
            Some(mapa) => Some(
                mapa.into_iter()
                    .map(|(var, texto)| {
                        let expr = expr::parse_expresion(extraer_expr(&texto)).map_err(|e| {
                            ErrorCarga::Validacion(format!(
                                "asigna '{}' del paso '{}': {e}",
                                var, self.name
                            ))
                        })?;
                        Ok(Asignacion { var, expr })
                    })
                    .collect::<Result<Vec<_>, ErrorCarga>>()?,
            ),
            None => None,
        };

        // RF-27: tipo de paso. `grpc` (default), `statement`, `sequence_call`
        // (M4b) o `pass_fail` (RF-25, ADR-0018).
        let tipo = match self.kind.as_str() {
            "grpc" => TipoPaso::Grpc,
            "statement" => TipoPaso::Statement,
            "sequence_call" => TipoPaso::SequenceCall,
            "pass_fail" => TipoPaso::PassFail,
            otro => {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' tiene tipo '{otro}' inválido \
                     (grpc|statement|sequence_call|pass_fail)",
                    self.name
                )))
            }
        };

        // RF-27: el statement se parsea a una lista de sentencias.
        let statement = match self.statement.as_deref() {
            Some(texto) => Some(expr::parse_sentencias(texto).map_err(|e| {
                ErrorCarga::Validacion(format!("statement del paso '{}' inválido: {e}", self.name))
            })?),
            None => None,
        };

        // RF-25 (ADR-0018): la condición del veredicto se parsea a AST aquí
        // (fail-fast), igual que la precondición. Bool estricto al evaluar:
        // que sea booleana no se sabe hasta el runtime (tipos dinámicos), así
        // que un no-Bool es `error` de ejecución, no de carga.
        let condicion = match self.condition.as_deref() {
            Some(texto) => Some(expr::parse_expresion(extraer_expr(texto)).map_err(|e| {
                ErrorCarga::Validacion(format!("condición del paso '{}' inválida: {e}", self.name))
            })?),
            None => None,
        };

        // M4b (RF-27): argumentos by-reference del sequence call. Cada valor
        // es "locals.X" y se parsea a AST; se valida que sea un lvalue local
        // puro (`Expresion::Var { scope: Locals, .. }`). Que el `campo`
        // exista en `locals` de la secuencia contenedora se valida al
        // resolver el programa (ver `cargar_programa_de_archivo`).
        // `inputs` es de un paso `grpc` (by-value, ADR-0020) y `args` de un
        // `sequence_call` (by-reference, ADR-0010). Cada uno en el sitio que
        // no le toca es un error de definición, no algo que ignorar: un paso
        // que declara valores que nadie va a leer está mal escrito.
        if !matches!(tipo, TipoPaso::Grpc) && self.inputs.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' pero trae 'inputs' (son los parámetros by-value de un \
                 paso 'grpc'; para pasar variables a una subsecuencia es 'args')",
                self.name, self.kind
            )));
        }
        if !matches!(tipo, TipoPaso::SequenceCall) && self.args.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' pero trae 'args' (son los argumentos by-reference de un \
                 'sequence_call'; para pasar valores a un paso es 'inputs')",
                self.name, self.kind
            )));
        }

        // ADR-0020: los parámetros by-value que viajan en la petición.
        let entradas = entradas_de_paso(&self.name, self.inputs.take())?;

        let parametros = match self.args.take() {
            Some(mapa) if !mapa.is_empty() => Some(
                mapa.into_iter()
                    .map(|(param, valor)| {
                        // By-reference: el valor tiene que ser el texto
                        // "locals.X". Un escalar no-texto (`p: 2`) no puede
                        // ser un lvalue, y decirlo aquí evita el mensaje
                        // interno de serde del issue #20.
                        let texto = match valor {
                            ValorYaml::Texto(t) => t,
                            otro => {
                                return Err(ErrorCarga::Validacion(format!(
                                    "el argumento '{param}' del sequence call '{}' es {}, y \
                                     by-reference sólo admite una variable local (locals.X)",
                                    self.name,
                                    describe_valor(&otro)
                                )))
                            }
                        };
                        let origen = expr::parse_expresion(extraer_expr(&texto)).map_err(|e| {
                            ErrorCarga::Validacion(format!(
                                "parámetro '{param}' del sequence call '{}': {e}",
                                self.name
                            ))
                        })?;
                        match &origen {
                            expr::Expresion::Var {
                                scope: expr::Scope::Locals,
                                ..
                            } => {}
                            _ => {
                                return Err(ErrorCarga::Validacion(format!(
                                    "el argumento '{param}' del sequence call '{}' debe ser una \
                                     variable local (locals.X); by-reference no admite expresiones",
                                    self.name
                                )))
                            }
                        }
                        Ok(Argumento { param, origen })
                    })
                    .collect::<Result<Vec<_>, ErrorCarga>>()?,
            ),
            _ => None,
        };

        // Coherencia tipo ↔ campos (fail-fast).
        if matches!(tipo, TipoPaso::Statement) && statement.is_none() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'statement' pero no trae 'statement'",
                self.name
            )));
        }
        if !matches!(tipo, TipoPaso::Statement) && statement.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' pero trae 'statement' (reservado para 'statement')",
                self.name, self.kind
            )));
        }
        // RF-25 (ADR-0018): un `pass_fail` es su condición; sin ella no hay
        // veredicto que dar.
        if matches!(tipo, TipoPaso::PassFail) && condicion.is_none() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'pass_fail' pero no trae 'condition'",
                self.name
            )));
        }
        if !matches!(tipo, TipoPaso::PassFail) && condicion.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' pero trae 'condition' (reservado para 'pass_fail')",
                self.name, self.kind
            )));
        }
        if matches!(tipo, TipoPaso::SequenceCall) && self.sequence.is_none() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'sequence_call' pero no trae 'sequence'",
                self.name
            )));
        }
        // Ni un sequence call ni un pass_fail miden: el primero agrega los
        // resultados de sus pasos, el segundo evalúa variables ya pobladas.
        if matches!(tipo, TipoPaso::SequenceCall | TipoPaso::PassFail) && limite.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' y trae 'limit': no mide",
                self.name, self.kind
            )));
        }
        if matches!(tipo, TipoPaso::SequenceCall) && self.retries > 1 {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'sequence_call' con reintentos={}: no admite reintentos \
                 (sus pasos internos declaran los suyos)",
                self.name, self.retries
            )));
        }
        // Un `pass_fail` es puro y determinista (el motor evalúa una
        // expresión, sin red): reintentarlo daría el mismo veredicto. Se
        // rechaza en vez de aceptarlo e ignorarlo en silencio.
        if matches!(tipo, TipoPaso::PassFail) && self.retries > 1 {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'pass_fail' con reintentos={}: no admite reintentos \
                 (evalúa una expresión, el resultado no cambia entre intentos)",
                self.name, self.retries
            )));
        }
        // Un `pass_fail` no produce `resultado.*`, así que su `asigna` no
        // volcaría nada. Rechazarlo en vez de ignorarlo: un `asigna` que no se
        // aplica es la clase de fallo silencioso de DEF-3.
        if matches!(tipo, TipoPaso::PassFail) && asigna.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'pass_fail' y trae 'assign': un pass_fail no produce \
                 'result.*' que volcar (usa un paso 'statement' aparte)",
                self.name
            )));
        }
        // Lo mismo para un `statement`, por el mismo motivo (ADR-0019, regla de
        // detección, issue #27): tampoco produce `resultado.*`, así que su
        // `asigna` era un no-op que `--validate` aprobaba. El caso hermano
        // (`pass_fail`) llevaba resuelto desde ADR-0018; éste se quedó fuera.
        if matches!(tipo, TipoPaso::Statement) && asigna.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'statement' y trae 'assign': un statement no produce \
                 'result.*' que volcar (asigna dentro del propio 'statement': \
                 'locals.x = …')",
                self.name
            )));
        }
        // Un `sequence_call` sí produce `resultado.*` — pero sólo dos tercios
        // de él. `ejecuta_sequence_call` (motor/src/lib.rs) rellena `estado`
        // (el agregado de la subsecuencia) y `mensaje`, y **nunca**
        // `valor_medido`: una subsecuencia no mide, agrega. Así que
        // `assign: {v: '${result.measured_value}'}` volcaba `nothing` encima
        // del destino y lo destruía, sin una palabra — el mismo fallo
        // silencioso que ADR-0019 arregló para los campos inexistentes
        // (issue #27), sólo que aquí el campo existe y lo que falta es el
        // valor. Anvil-Test lo cazó con una variable que valía 42.0 antes del
        // sequence call y `nothing` después.
        if matches!(tipo, TipoPaso::SequenceCall) {
            if let Some(asignaciones) = &asigna {
                for a in asignaciones {
                    if primer_uso_de_resultado_si(&a.expr, &|c| c == "measured_value").is_some() {
                        return Err(ErrorCarga::Validacion(format!(
                            "el paso '{}' es 'sequence_call' y asigna a '{}' desde \
                             'result.measured_value': una subsecuencia no mide, agrega \
                             el veredicto de sus pasos, así que ese campo vale siempre \
                             'nothing' y borraría '{}' sin avisar. De 'result' de un \
                             sequence call hay 'status' y 'message'; para devolver un \
                             valor medido, pásalo por 'args' (by-reference)",
                            self.name, a.var, a.var
                        )));
                    }
                }
            }
        }
        // `secuencia` sigue siendo sólo de `sequence_call`. `parametros` ya
        // no: en un paso `grpc` son los parámetros by-value del ADR-0020, y
        // se han recogido antes en `entradas`. En `statement` y `pass_fail`
        // no significa nada, y se sigue rechazando.
        if !matches!(tipo, TipoPaso::SequenceCall) && self.sequence.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' pero trae 'sequence' (reservado para 'sequence_call')",
                self.name, self.kind
            )));
        }
        // M5-ext.1 (RF-36.3): `ejecutor` sólo aplica a un paso `Grpc` (los
        // `statement`/`sequence_call` son motor-side y no van por gRPC).
        if !matches!(tipo, TipoPaso::Grpc) && self.executor.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' pero trae 'executor' (reservado para 'grpc')",
                self.name, self.kind
            )));
        }

        Ok(DefinicionPaso {
            nombre: self.name,
            reintentos: self.retries,
            limite,
            disable: self.disable,
            pause_on_fail: self.pause_on_fail,
            precondicion,
            asigna,
            tipo,
            statement,
            condicion,
            secuencia: self.sequence,
            parametros,
            entradas,
            ejecutor: self.executor,
        })
    }
}

/// Cómo se nombra un escalar YAML en un mensaje de error. Existe para que el
/// cargador diga «es un número» y no `TypeMismatch { expected: "string",
/// found: "non-string scalar" }`, que es el defecto de diagnóstico del #20.
fn describe_valor(v: &ValorYaml) -> String {
    match v {
        ValorYaml::Bool(b) => format!("el booleano `{b}`"),
        ValorYaml::Numero(n) => format!("el número `{n}`"),
        ValorYaml::Texto(t) => format!("el texto `{t}`"),
        ValorYaml::Declaracion(d) => format!("una declaración `type: {}`", d.kind),
    }
}

/// Los scopes que el motor conoce. Si un parámetro by-value empieza por uno de
/// éstos y **no** va entre `${...}`, casi seguro que quien lo escribió quería
/// una expresión y va a mandar el nombre de la variable como texto.
const SCOPES: [&str; 3] = ["locals.", "file_globals.", "parameters."];

/// ADR-0020 §2: los parámetros **by-value** de un paso `grpc`.
///
/// Cada valor es un literal —y su tipo es el del escalar YAML— o una
/// expresión `${...}` que evalúa el motor antes de llamar.
///
/// **La red de seguridad.** `{ channel: locals.channel }` viajaría como el
/// texto literal `"locals.channel"`, y el paso mediría con una cadena en vez
/// de con el número. Es un error silencioso —el YAML carga, el paso corre, la
/// medida es otra— así que no se traga: es error de carga y el mensaje dice
/// cuál es la forma correcta.
///
/// Antes esto tapaba además una trampa peor: `parametros` se llamaba igual en
/// un `sequence_call`, donde sí es by-reference, y copiar un bloque cambiaba
/// el significado. Eso lo arregla ahora el schema —`inputs` y `args` son
/// campos distintos y `deny_unknown_fields` rechaza el cruce— pero la
/// comprobación se queda: escribir el nombre de una variable sin `${}` sigue
/// siendo un error plausible por sí solo.
///
/// Se devuelve ordenado por nombre: el orden del cable tiene que ser
/// determinista para que dos corridas iguales produzcan bytes iguales.
fn entradas_de_paso(
    paso: &str,
    mapa: Option<HashMap<String, ValorYaml>>,
) -> Result<Option<Vec<(String, EntradaPaso)>>, ErrorCarga> {
    let mapa = match mapa {
        Some(m) if !m.is_empty() => m,
        _ => return Ok(None),
    };
    let mut entradas: Vec<(String, EntradaPaso)> = Vec::with_capacity(mapa.len());
    for (nombre, valor) in mapa {
        let entrada = match &valor {
            ValorYaml::Bool(b) => EntradaPaso::Literal(ValorDefinicion::Bool(*b)),
            ValorYaml::Numero(n) => EntradaPaso::Literal(ValorDefinicion::Numero(*n)),
            ValorYaml::Texto(t) => {
                let interior = extraer_expr(t);
                if interior != t {
                    // Venía como `${...}`: es una expresión.
                    EntradaPaso::Expresion(expr::parse_expresion(interior).map_err(|e| {
                        ErrorCarga::Validacion(format!(
                            "parámetro '{nombre}' del paso '{paso}': {e}"
                        ))
                    })?)
                } else if SCOPES.iter().any(|s| t.starts_with(s)) {
                    return Err(ErrorCarga::Validacion(format!(
                        "el parámetro '{nombre}' del paso '{paso}' vale '{t}', que viajaría \
                         como el texto literal \"{t}\" y no como el valor de esa variable. \
                         Si querías la variable, escríbela como '${{{t}}}'."
                    )));
                } else {
                    EntradaPaso::Literal(ValorDefinicion::Texto(t.clone()))
                }
            }
            // ADR-0022 §1: a reference has no literal form, and this is where
            // that refusal is spent. A handle reaches a step by being read out
            // of a variable —`'${locals.rack}'`, an expression— and never by
            // being written into the sequence, which is what makes "the
            // sequence can be read without running it" true of it.
            ValorYaml::Declaracion(_) => {
                return Err(ErrorCarga::Validacion(format!(
                    "el parámetro '{nombre}' del paso '{paso}' se escribe como una \
                     declaración, y un parámetro es un valor. Una referencia no se puede \
                     escribir a mano: declárala en 'locals:' con \
                     '{{ type: {TIPO_REFERENCIA}, executor: <nombre> }}', recógela con \
                     'assign' del paso que la abre, y pásala como '${{locals.<nombre>}}'"
                )));
            }
        };
        entradas.push((nombre, entrada));
    }
    entradas.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Some(entradas))
}

/// Si `texto` es de la forma `${expr}` (toda la cadena), devuelve `expr`;
/// si no, devuelve `texto` tal cual. Así `asigna` admite las dos formas
/// `x: result.measured_value` y `x: "${result.measured_value}"`. La
/// interpolación parcial (`"V=${...}"`) se rechaza: post-MVP.
fn extraer_expr(texto: &str) -> &str {
    let s = texto.trim();
    if s.starts_with("${") && s.ends_with('}') {
        // Sólo si toda la cadena es `${...}`. Una interpolación parcial
        // (p. ej. `"V=${x}"`) no encaja aquí y se pasa tal cual al parser, que
        // la rechazará con un error de sintaxis claro.
        &s[2..s.len() - 1]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basica_yaml() -> &'static str {
        "\
name: basica
setup:
  - name: conectar_equipo
    retries: 3
main:
  - name: medir_voltaje
    retries: 1
    limit:
      type: range
      min: 4.5
      max: 5.5
  - name: verificar_led
    retries: 1
cleanup:
  - name: desconectar_equipo
    retries: 1
"
    }

    #[test]
    fn carga_la_secuencia_basica() {
        let s = cargar_de_texto(basica_yaml()).unwrap();
        assert_eq!(s.nombre, "basica");
        assert_eq!(s.pasos_setup.len(), 1);
        assert_eq!(s.pasos_main.len(), 2);
        assert_eq!(s.pasos_cleanup.len(), 1);
    }

    #[test]
    fn la_traduccion_coincide_con_el_ejemplo_en_codigo() {
        // El mismo contenido que crates/motor/src/bin/basica_datos.rs. Los
        // scopes de M4 quedan vacíos (no los usa `basica.yaml`).
        let s = cargar_de_texto(basica_yaml()).unwrap();
        let esperada = modelo::DefinicionSecuencia {
            nombre: "basica".into(),
            pasos_setup: vec![DefinicionPaso::nuevo("conectar_equipo", 3)],
            pasos_main: vec![
                DefinicionPaso::con_limite(
                    "medir_voltaje",
                    1,
                    Limite::Rango { min: 4.5, max: 5.5 },
                ),
                DefinicionPaso::nuevo("verificar_led", 1),
            ],
            pasos_cleanup: vec![DefinicionPaso::nuevo("desconectar_equipo", 1)],
            ..Default::default()
        };
        assert_eq!(s, esperada);
    }

    #[test]
    fn reintentos_omitido_es_1() {
        let yaml = "\
name: s
main:
  - name: un_paso
";
        let s = cargar_de_texto(yaml).unwrap();
        assert_eq!(s.pasos_main[0].reintentos, 1);
    }

    #[test]
    fn setup_y_cleanup_son_opcionales() {
        let yaml = "\
name: s
main:
  - name: un_paso
";
        let s = cargar_de_texto(yaml).unwrap();
        assert!(s.pasos_setup.is_empty());
        assert!(s.pasos_cleanup.is_empty());
    }

    #[test]
    fn main_ausente_es_error() {
        let yaml = "name: s\n";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Sintaxis(_)),
            "main ausente debe ser error de schema, no de validación: {err}"
        );
    }

    #[test]
    fn main_vacio_es_error_de_validacion() {
        let yaml = "name: s\nmain: []\n";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("main")));
    }

    #[test]
    fn nombre_vacio_es_error() {
        let yaml = "\
name: ''
main:
  - name: un_paso
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("name")));
    }

    #[test]
    fn reintentos_cero_es_error() {
        let yaml = "\
name: s
main:
  - name: un_paso
    retries: 0
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("retries")));
    }

    #[test]
    fn campo_desconocido_es_error() {
        // Desde M4, `disable`/`pause_on_fail`/`precondicion`/`asigna`/`tipo`/
        // `statement` son campos conocidos; desde M4b también `secuencia`/
        // `parametros` (paso) y `subsecuencias` (secuencia). Usamos uno
        // realmente desconocido (`foo`) para seguir probando
        // `deny_unknown_fields` (fail-fast).
        let yaml = "\
name: s
main:
  - name: un_paso
    foo: bar
";
        let err = cargar_de_texto(yaml).unwrap_err();
        // DIAG-5: sigue siendo fail-fast, y además ubica el campo.
        assert!(
            matches!(&err, ErrorCarga::Diagnostico(m) if m.contains("'foo'") && m.contains("main[0]")),
            "campo desconocido debe ser error de schema, ubicado: {err}"
        );
    }

    // --- M4b: sequence call, subsecuencias inline y por path ---

    /// Una subsecuencia inline se carga en `subsecuencias` y un `sequence_call`
    /// por nombre la referencia. El programa resuelve sin archivos externos.
    #[test]
    fn sequence_call_inline_se_carga_y_resuelve() {
        let yaml = "\
name: padre
locals: { ok: false }
subsequences:
  init:
    parameters: { canal: 0.0, listo: false }
    main:
      - name: comprobar
        type: statement
        statement: 'parameters.listo = (parameters.canal >= 0.0)'
main:
  - name: preparar
    type: sequence_call
    sequence: init
    args: { canal: locals.ok, listo: locals.ok }
";
        let s = cargar_de_texto(yaml).unwrap();
        assert_eq!(s.subsecuencias.len(), 1);
        assert_eq!(s.pasos_main[0].tipo, modelo::TipoPaso::SequenceCall);
        assert_eq!(s.pasos_main[0].secuencia.as_deref(), Some("init"));
        let args = s.pasos_main[0].parametros.as_ref().unwrap();
        assert_eq!(args.len(), 2);
    }

    /// `cargar_de_texto` no resuelve ni valida lvalues/firma (sólo forma);
    /// `cargar_programa_de_archivo` sí. Aquí probamos la validación de la
    /// **forma** del lvalue en `a_definicion`.
    #[test]
    fn argumento_que_no_es_locals_x_es_error() {
        let yaml = "\
name: s
main:
  - name: c
    type: sequence_call
    sequence: ./h.yaml
    args: { p: file_globals.g }
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("locals.X")));
    }

    #[test]
    fn argumento_expresion_no_es_lvalue_es_error() {
        let yaml = "\
name: s
main:
  - name: c
    type: sequence_call
    sequence: ./h.yaml
    args: { p: 'locals.x + 1' }
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("by-reference")));
    }

    /// Sequence call sin `secuencia` → error; con `limite` → error;
    /// con `reintentos > 1` → error; con `statement` → error.
    #[test]
    fn sequence_call_mal_usado_es_error() {
        let casos = [
            ("name: s\nmain:\n  - name: c\n    type: sequence_call\n", "no trae 'sequence'"),
            ("name: s\nmain:\n  - name: c\n    type: sequence_call\n    sequence: x\n    limit: { type: range, min: 1, max: 2 }\n", "no mide"),
            ("name: s\nmain:\n  - name: c\n    type: sequence_call\n    sequence: x\n    retries: 2\n", "no admite reintentos"),
            ("name: s\nmain:\n  - name: c\n    type: sequence_call\n    sequence: x\n    statement: 'locals.y = 1'\n", "reservado para 'statement'"),
        ];
        for (yaml, frag) in casos {
            let err = cargar_de_texto(yaml).unwrap_err();
            assert!(
                matches!(&err, ErrorCarga::Validacion(m) if m.contains(frag)),
                "esperaba '{frag}' en {err}"
            );
        }
    }

    /// RF-25 (ADR-0018): `pass_fail` parsea su `condicion` a AST al cargar.
    #[test]
    fn pass_fail_se_parsea_a_condicion() {
        let yaml = "\
name: s
locals:
  v: 0.0
main:
  - name: verificar_dut
    type: pass_fail
    condition: 'locals.v > 4.9 && locals.v < 5.1'
";
        let s = cargar_de_texto(yaml).unwrap();
        let p = &s.pasos_main[0];
        assert_eq!(p.tipo, TipoPaso::PassFail);
        assert!(p.condicion.is_some(), "la condición llega como AST");
        assert!(p.statement.is_none());
    }

    /// La condición admite las dos formas de expresión, como `asigna`.
    #[test]
    fn pass_fail_admite_la_forma_interpolada() {
        let yaml = "\
name: s
locals:
  v: 0.0
main:
  - name: verificar_dut
    type: pass_fail
    condition: '${locals.v > 4.9}'
";
        assert!(cargar_de_texto(yaml).is_ok());
    }

    /// `pass_fail` sin `condicion` → error; `condicion` fuera de un
    /// `pass_fail` → error; con `limite`/`reintentos`/`asigna` → error.
    #[test]
    fn pass_fail_mal_usado_es_error() {
        let casos = [
            (
                "name: s\nmain:\n  - name: v\n    type: pass_fail\n",
                "no trae 'condition'",
            ),
            (
                "name: s\nmain:\n  - name: v\n    condition: 'true'\n",
                "reservado para 'pass_fail'",
            ),
            (
                "name: s\nmain:\n  - name: v\n    type: statement\n    statement: 'locals.x = 1'\n    condition: 'true'\n",
                "reservado para 'pass_fail'",
            ),
            (
                "name: s\nmain:\n  - name: v\n    type: pass_fail\n    condition: 'true'\n    limit: { type: range, min: 1, max: 2 }\n",
                "no mide",
            ),
            (
                "name: s\nmain:\n  - name: v\n    type: pass_fail\n    condition: 'true'\n    retries: 2\n",
                "no admite reintentos",
            ),
            (
                "name: s\nlocals:\n  x: 0.0\nmain:\n  - name: v\n    type: pass_fail\n    condition: 'true'\n    assign: { x: '1.0' }\n",
                "no produce 'result.*'",
            ),
            (
                "name: s\nmain:\n  - name: v\n    type: pass_fail\n    condition: 'locals.v >'\n",
                "condición del paso",
            ),
        ];
        for (yaml, frag) in casos {
            let err = cargar_de_texto(yaml).unwrap_err();
            assert!(
                matches!(&err, ErrorCarga::Validacion(m) if m.contains(frag)),
                "esperaba '{frag}' en {err}"
            );
        }
    }

    /// `grpc`/`statement` no pueden traer `secuencia`/`parametros`.
    #[test]
    fn grpc_no_admite_secuencia_ni_parametros() {
        let yaml = "\
name: s
main:
  - name: c
    sequence: ./h.yaml
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("reservado para 'sequence_call'"))
        );
    }

    /// Una subsecuencia inline es una secuencia completa: `main` es
    /// obligatorio (como en la raíz). Ausente → error de schema (Sintaxis).
    #[test]
    fn inline_sin_main_es_error() {
        let yaml = "\
name: s
subsequences:
  init:
    name: init
main:
  - name: p
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Sintaxis(_)),
            "inline sin main: error de schema: {err}"
        );
    }

    /// Una inline puede omitir `nombre`: toma el de su clave en el mapa.
    #[test]
    fn inline_hereda_nombre_de_la_clave() {
        let yaml = "\
name: s
subsequences:
  init:
    main:
      - name: p
main:
  - name: m
";
        let s = cargar_de_texto(yaml).unwrap();
        assert_eq!(s.subsecuencias.get("init").unwrap().nombre, "init");
    }

    /// `deny_unknown_fields` también aplica dentro de `subsecuencias`: una
    /// inline con un campo raro falla.
    #[test]
    fn inline_con_campo_desconocido_es_error() {
        let yaml = "\
name: s
subsequences:
  init:
    name: init
    main:
      - name: p
    foo: bar
main:
  - name: p
";
        let err = cargar_de_texto(yaml).unwrap_err();
        // DIAG-5: en un fichero con varias inline, saber en cuál está es la
        // diferencia entre arreglarlo y buscarlo a ojo.
        assert!(
            matches!(&err, ErrorCarga::Diagnostico(m) if m.contains("subsequences.init")),
            "{err}"
        );
    }

    /// `cargar_programa_de_archivo` resuelve un archivo externo por path,
    /// lo registra en `archivos` (clave canónica) y reescribe `secuencia`.
    #[test]
    fn programa_resuelve_subsecuencia_externa() {
        // Escribimos dos archivos en un dir temporal.
        let dir = std::env::temp_dir().join(format!("anvil_m4b_{}", "ext_ok"));
        std::fs::create_dir_all(&dir).unwrap();
        let hija = dir.join("hija.yaml");
        std::fs::write(
            &hija,
            "name: hija\nparameters: { canal: 0.0 }\nmain:\n  - name: m\n    type: grpc\n",
        )
        .unwrap();
        let padre = dir.join("padre.yaml");
        std::fs::write(
            &padre,
            "name: padre\nlocals: { canal: 1.0 }\nmain:\n  - name: c\n    type: sequence_call\n    sequence: ./hija.yaml\n    args: { canal: locals.canal }\n",
        )
        .unwrap();

        let prog = cargar_programa_de_archivo(padre.to_str().unwrap()).unwrap();
        assert_eq!(prog.raiz.nombre, "padre");
        assert_eq!(prog.archivos.len(), 1, "una subsecuencia externa cargada");
        let clave = prog.raiz.pasos_main[0].secuencia.as_deref().unwrap();
        assert!(es_path(clave), "reescribe a path canónico: {clave}");
        assert_eq!(
            prog.archivos
                .get(clave)
                .map(|d| d.nombre.as_str())
                .unwrap_or(""),
            "hija"
        );
    }

    /// Ciclo por path (A → B → A) se detecta al cargar el programa.
    #[test]
    fn programa_detecta_ciclo_por_path() {
        let dir = std::env::temp_dir().join(format!("anvil_m4b_{}", "ciclo"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.yaml"),
            "name: a\nmain:\n  - name: c\n    type: sequence_call\n    sequence: ./b.yaml\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("b.yaml"),
            "name: b\nmain:\n  - name: c\n    type: sequence_call\n    sequence: ./a.yaml\n",
        )
        .unwrap();
        let err = cargar_programa_de_archivo(dir.join("a.yaml").to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("ciclo")));
    }

    /// Firma que no encaja (falta un parámetro) → error al cargar el programa.
    #[test]
    fn programa_firma_no_encaja_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m4b_{}", "firma"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("h.yaml"),
            "name: h\nparameters: { canal: 0.0, extra: 0.0 }\nmain:\n  - name: m\n",
        )
        .unwrap();
        std::fs::write(dir.join("p.yaml"), "name: p\nlocals: { canal: 1.0 }\nmain:\n  - name: c\n    type: sequence_call\n    sequence: ./h.yaml\n    args: { canal: locals.canal }\n").unwrap();
        let err = cargar_programa_de_archivo(dir.join("p.yaml").to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("firma")));
    }

    /// Argumento `locals.X` no declarado en el padre → error al cargar.
    #[test]
    fn programa_lvalue_no_declarado_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m4b_{}", "lvalue"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("h.yaml"),
            "name: h\nparameters: { canal: 0.0 }\nmain:\n  - name: m\n",
        )
        .unwrap();
        std::fs::write(dir.join("p.yaml"), "name: p\nmain:\n  - name: c\n    type: sequence_call\n    sequence: ./h.yaml\n    args: { canal: locals.inventado }\n").unwrap();
        let err = cargar_programa_de_archivo(dir.join("p.yaml").to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("locals.inventado")));
    }

    /// Path no encontrado → error de lectura al cargar el programa.
    #[test]
    fn programa_path_no_encontrado_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m4b_{}", "nofile"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.yaml"), "name: p\nmain:\n  - name: c\n    type: sequence_call\n    sequence: ./no_existe.yaml\n").unwrap();
        let err = cargar_programa_de_archivo(dir.join("p.yaml").to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Lectura(_)));
    }

    /// El ejemplo `ejemplos/subsecuencia.yaml` carga como programa: la
    /// subsecuencia externa `./medir_fuentes.yaml` se resuelve, la inline
    /// `init_comun` se enlaza por nombre y la firma/lvalues validan.
    #[test]
    fn ejemplo_subsecuencia_carga_como_programa() {
        let ruta = format!(
            "{}/../../ejemplos/subsecuencia.yaml",
            env!("CARGO_MANIFEST_DIR")
        );
        let prog = cargar_programa_de_archivo(&ruta)
            .unwrap_or_else(|e| panic!("no carga el programa {ruta}: {e}"));
        assert_eq!(prog.raiz.nombre, "basica");
        assert_eq!(prog.raiz.subsecuencias.len(), 1, "una inline: init_comun");
        assert_eq!(prog.archivos.len(), 1, "una externa: medir_fuentes.yaml");
        // El call externo reescribe su `secuencia` a la clave canónica (path).
        let call_ext = &prog.raiz.pasos_main[1];
        assert_eq!(call_ext.tipo, modelo::TipoPaso::SequenceCall);
        assert!(
            es_path(call_ext.secuencia.as_deref().unwrap()),
            "path reescrito"
        );
    }

    #[test]
    fn limite_rango_embebido_se_carga() {
        let yaml = "\
name: s
main:
  - name: medir_voltaje
    limit:
      type: range
      min: 4.5
      max: 5.5
";
        let s = cargar_de_texto(yaml).unwrap();
        assert_eq!(
            s.pasos_main[0].limite,
            Some(Limite::Rango { min: 4.5, max: 5.5 })
        );
    }

    #[test]
    fn limite_comparacion_embebido_se_carga() {
        let yaml = "\
name: s
main:
  - name: verificar_frecuencia
    limit:
      type: comparison
      op: ge
      expected: 1000.0
";
        let s = cargar_de_texto(yaml).unwrap();
        assert_eq!(
            s.pasos_main[0].limite,
            Some(Limite::Comparacion {
                op: Operador::Ge,
                esperado: 1000.0
            })
        );
    }

    #[test]
    fn limite_rango_sin_min_es_error_de_validacion() {
        let yaml = "\
name: s
main:
  - name: m
    limit:
      type: range
      max: 5.5
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("min")),
            "{err}"
        );
    }

    #[test]
    fn limite_rango_min_mayor_que_max_es_error() {
        let yaml = "\
name: s
main:
  - name: m
    limit:
      type: range
      min: 6.0
      max: 5.5
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains(">")),
            "{err}"
        );
    }

    #[test]
    fn limite_comparacion_op_invalido_es_error() {
        let yaml = "\
name: s
main:
  - name: m
    limit:
      type: comparison
      op: mayor_que
      expected: 1000.0
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("op")),
            "{err}"
        );
    }

    #[test]
    fn limite_rango_con_campos_de_comparacion_es_error() {
        // fail-fast: un rango no admite op/esperado.
        let yaml = "\
name: s
main:
  - name: m
    limit:
      type: range
      min: 4.5
      max: 5.5
      op: ge
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("op")),
            "{err}"
        );
    }

    #[test]
    fn limite_tipo_desconocido_es_error() {
        let yaml = "\
name: s
main:
  - name: m
    limit:
      type: ventana
      min: 4.5
      max: 5.5
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("type")),
            "{err}"
        );
    }

    #[test]
    fn limite_campo_desconocido_dentro_del_limite_es_error() {
        // deny_unknown_fields en LimiteYaml: un campo raro dentro del límite.
        let yaml = "\
name: s
main:
  - name: m
    limit:
      type: range
      min: 4.5
      max: 5.5
      tolerancia: 0.1
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Diagnostico(m) if m.contains("main[0].limit")),
            "{err}"
        );
    }

    #[test]
    fn property_loader_aplica_limites_por_nombre() {
        let mut s = cargar_de_texto(basica_yaml()).unwrap();
        let mut lim = HashMap::new();
        lim.insert(
            "medir_voltaje".to_string(),
            Limite::Rango { min: 4.5, max: 5.5 },
        );
        let n = aplicar_limites(&mut s, &lim);
        assert_eq!(n, 1, "solo medir_voltaje recibió límite");
        assert_eq!(
            s.pasos_main[0].limite,
            Some(Limite::Rango { min: 4.5, max: 5.5 })
        );
        // Los demás pasos siguen sin límite.
        assert_eq!(
            s.pasos_main[1].limite, None,
            "verificar_led no estaba en el sidecar"
        );
    }

    #[test]
    fn property_loader_sobreescribe_el_limite_embebido() {
        // El sidecar manda sobre el límite embebido (variabilidad por lote).
        let yaml = "\
name: s
main:
  - name: medir_voltaje
    limit:
      type: range
      min: 4.5
      max: 5.5
";
        let mut s = cargar_de_texto(yaml).unwrap();
        let mut lim = HashMap::new();
        lim.insert(
            "medir_voltaje".to_string(),
            Limite::Rango { min: 4.0, max: 6.0 },
        );
        aplicar_limites(&mut s, &lim);
        assert_eq!(
            s.pasos_main[0].limite,
            Some(Limite::Rango { min: 4.0, max: 6.0 }),
            "el sidecar overridea el embebido"
        );
    }

    #[test]
    fn property_loader_ignora_nombres_que_no_estan_en_la_secuencia() {
        let mut s = cargar_de_texto(basica_yaml()).unwrap();
        let mut lim = HashMap::new();
        lim.insert(
            "paso_que_no_existe".to_string(),
            Limite::Rango { min: 0.0, max: 1.0 },
        );
        assert_eq!(aplicar_limites(&mut s, &lim), 0, "ningún paso coincide");
        assert_eq!(
            limites_sin_aplicar(&s, &lim),
            vec!["paso_que_no_existe"],
            "y se puede decir cuál no casó (DIAG-1)"
        );
    }

    #[test]
    fn limites_sin_aplicar_lista_solo_los_huerfanos_y_ordenados() {
        let s = cargar_de_texto(basica_yaml()).unwrap();
        let rango = Limite::Rango { min: 0.0, max: 1.0 };
        let lim = HashMap::from([
            ("medir_voltaje".to_string(), rango.clone()), // sí existe en basica
            ("zeta_inventado".to_string(), rango.clone()),
            ("alfa_inventado".to_string(), rango),
        ]);
        assert_eq!(
            limites_sin_aplicar(&s, &lim),
            vec!["alfa_inventado", "zeta_inventado"],
            "sólo los que no casan, en orden estable"
        );
    }

    #[test]
    fn limites_sin_aplicar_vacio_cuando_todo_casa() {
        let s = cargar_de_texto(basica_yaml()).unwrap();
        let lim = HashMap::from([(
            "medir_voltaje".to_string(),
            Limite::Rango { min: 0.0, max: 1.0 },
        )]);
        assert!(limites_sin_aplicar(&s, &lim).is_empty());
    }

    /// DEF-1 del informe de beta: con `--process-model` la raíz es el PM y la
    /// secuencia del operador vive en `archivos`. El sidecar tiene que llegar
    /// igual, o el mecanismo de variabilidad por lote no funciona justo en
    /// producción.
    #[test]
    fn property_loader_alcanza_la_secuencia_del_operador_bajo_un_pm() {
        let dir = std::env::temp_dir().join("anvil_def1_sidecar_pm");
        std::fs::create_dir_all(&dir).unwrap();
        let pm = dir.join("pm.yaml");
        std::fs::write(
            &pm,
            "name: pm\nmain:\n  - name: test_uut\n    type: sequence_call\n    sequence: secuencia_usuario\n",
        )
        .unwrap();
        let usuario = dir.join("usuario.yaml");
        std::fs::write(
            &usuario,
            "name: usuario\nmain:\n  - name: medir_voltaje\n    type: grpc\n",
        )
        .unwrap();

        let mut prog =
            cargar_programa_con_pm(pm.to_str().unwrap(), usuario.to_str().unwrap()).unwrap();
        let lim = HashMap::from([(
            "medir_voltaje".to_string(),
            Limite::Rango { min: 4.0, max: 6.0 },
        )]);

        // La primitiva por secuencia sólo ve el PM: ahí no hay nada que casar.
        assert_eq!(
            aplicar_limites(&mut prog.raiz, &lim),
            0,
            "el PM no tiene un paso 'medir_voltaje'"
        );

        assert!(
            limites_sin_aplicar_programa(&prog, &lim).is_empty(),
            "el nombre sí existe en el programa, no es huérfano"
        );
        assert_eq!(
            aplicar_limites_programa(&mut prog, &lim),
            1,
            "el sidecar llega a la secuencia del operador"
        );
        let clave = prog.raiz.pasos_main[0].secuencia.as_deref().unwrap();
        assert_eq!(
            prog.archivos[clave].pasos_main[0].limite,
            Some(Limite::Rango { min: 4.0, max: 6.0 })
        );
    }

    /// El criterio es uniforme: el sidecar casa por nombre en cualquier
    /// secuencia del programa, también en las **inline**.
    #[test]
    fn property_loader_programa_alcanza_raiz_externas_e_inline() {
        let dir = std::env::temp_dir().join("anvil_def1_sidecar_alcance");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("hija.yaml"),
            "name: hija\nmain:\n  - name: medir_voltaje\n    type: grpc\n",
        )
        .unwrap();
        let padre = dir.join("padre.yaml");
        std::fs::write(
            &padre,
            "name: padre\nsubsequences:\n  inline:\n    name: inline\n    main:\n      - name: medir_voltaje\n        type: grpc\nmain:\n  - name: medir_voltaje\n    type: grpc\n  - name: c1\n    type: sequence_call\n    sequence: ./hija.yaml\n  - name: c2\n    type: sequence_call\n    sequence: inline\n",
        )
        .unwrap();

        let mut prog = cargar_programa_de_archivo(padre.to_str().unwrap()).unwrap();
        let lim = HashMap::from([(
            "medir_voltaje".to_string(),
            Limite::Rango { min: 4.0, max: 6.0 },
        )]);
        assert_eq!(
            aplicar_limites_programa(&mut prog, &lim),
            3,
            "raíz + externa + inline"
        );
    }

    #[test]
    fn limites_sin_aplicar_programa_lista_los_huerfanos_de_todo_el_programa() {
        let mut prog = Programa {
            raiz: cargar_de_texto(basica_yaml()).unwrap(),
            ..Default::default()
        };
        prog.archivos.insert(
            "hija.yaml".to_string(),
            cargar_de_texto("name: hija\nmain:\n  - name: solo_en_la_hija\n    type: grpc\n")
                .unwrap(),
        );
        let rango = Limite::Rango { min: 0.0, max: 1.0 };
        let lim = HashMap::from([
            ("medir_voltaje".to_string(), rango.clone()), // en la raíz
            ("solo_en_la_hija".to_string(), rango.clone()), // en la externa
            ("zeta_inventado".to_string(), rango.clone()),
            ("alfa_inventado".to_string(), rango),
        ]);
        assert_eq!(
            limites_sin_aplicar_programa(&prog, &lim),
            vec!["alfa_inventado", "zeta_inventado"],
            "sólo los que no casan en ninguna secuencia, en orden estable"
        );
    }

    /// DIAG-5: `steps:` es el error más común de quien viene de otra
    /// herramienta, y el campo correcto está a una palabra de distancia.
    #[test]
    fn steps_sugiere_main() {
        let err = cargar_de_texto("name: s\nsteps:\n  - name: p\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("campo desconocido 'steps'"), "{msg}");
        assert!(msg.contains("la raíz"), "{msg}");
        assert!(msg.contains("¿querías 'main'?"), "{msg}");
    }

    #[test]
    fn steps_en_una_inline_ubica_la_inline_y_sugiere() {
        let yaml = "\
name: s
subsequences:
  interna:
    steps:
      - name: p
main:
  - name: p
";
        let msg = cargar_de_texto(yaml).unwrap_err().to_string();
        assert!(msg.contains("subsequences.interna"), "{msg}");
        assert!(msg.contains("¿querías 'main'?"), "{msg}");
    }

    /// Una errata sin alias se resuelve por parecido.
    #[test]
    fn errata_sugiere_el_campo_parecido() {
        let msg = cargar_de_texto("name: s\nmain:\n  - name: p\n    retrie: 2\n")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("¿querías 'retries'?"), "{msg}");
    }

    /// Y un campo que no se parece a nada no recibe una sugerencia inventada:
    /// desorienta más que callarse.
    #[test]
    fn campo_sin_parecido_no_sugiere_nada() {
        let msg = cargar_de_texto("name: s\nmain:\n  - name: p\n    zumbido: 2\n")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("campo desconocido 'zumbido'"), "{msg}");
        assert!(!msg.contains("¿querías"), "{msg}");
    }

    /// El mismo campo mal escrito en dos sitios se reporta con ambos.
    #[test]
    fn campo_repetido_lista_todas_las_ubicaciones() {
        let yaml = "\
name: s
subsequences:
  a:
    steps: []
  b:
    steps: []
main:
  - name: p
";
        let msg = cargar_de_texto(yaml).unwrap_err().to_string();
        assert!(msg.contains("subsequences.a"), "{msg}");
        assert!(msg.contains("subsequences.b"), "{msg}");
    }

    #[test]
    fn distancia_de_edicion_cuenta_caracteres_no_bytes() {
        assert_eq!(distancia_edicion("limite", "límite"), 1);
        assert_eq!(distancia_edicion("main", "main"), 0);
        assert_eq!(distancia_edicion("mian", "main"), 2);
    }

    /// DIAG-5: el sidecar envuelto en `limites:` acusaba al nombre del paso
    /// (`unknown field: medir_voltaje`), que está bien. Ahora señala el
    /// envoltorio, que es lo que sobra.
    #[test]
    fn sidecar_con_envoltorio_señala_el_envoltorio() {
        let dir = std::env::temp_dir().join("anvil_diag5_envoltorio");
        std::fs::create_dir_all(&dir).unwrap();
        let ruta = dir.join("envoltorio.limits.yaml");
        std::fs::write(
            &ruta,
            "limites:\n  medir_voltaje:\n    tipo: rango\n    min: 4.0\n    max: 5.0\n",
        )
        .unwrap();
        let err = cargar_limites_de_archivo(ruta.to_str().unwrap()).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(&err, ErrorCarga::Diagnostico(_)), "{msg}");
        assert!(msg.contains("mapa plano"), "{msg}");
        assert!(msg.contains("'limites:'"), "{msg}");
        // Y no acusa al paso, que no tiene culpa.
        assert!(!msg.contains("unknown field"), "{msg}");
    }

    /// Las demás palabras con las que se envuelve por costumbre.
    #[test]
    fn sidecar_reconoce_los_envoltorios_habituales() {
        for clave in ["límites", "limite", "limits", "pasos"] {
            let texto = format!("{clave}:\n  medir_voltaje:\n    tipo: comparacion\n    op: ge\n    esperado: 4.0\n");
            let original = noyalib::from_str::<HashMap<String, LimiteYaml>>(&texto).unwrap_err();
            let err = diagnostica_sidecar(&texto, original);
            assert!(
                matches!(&err, ErrorCarga::Diagnostico(m) if m.contains(clave)),
                "{clave}: {err}"
            );
        }
    }

    /// Un sidecar roto por otro motivo conserva el error de serde: el
    /// diagnóstico no debe inventarse explicaciones que no tiene.
    #[test]
    fn sidecar_roto_por_otra_causa_conserva_el_error_original() {
        let texto = "medir_voltaje:\n  tipo: rango\n  min: 4.5\n  tolerancia: 0.1\n";
        let original = noyalib::from_str::<HashMap<String, LimiteYaml>>(texto).unwrap_err();
        let err = diagnostica_sidecar(texto, original);
        assert!(matches!(&err, ErrorCarga::Sintaxis(_)), "{err}");
    }

    /// Un envoltorio con más claves al lado no es el caso del envoltorio: ahí
    /// no sabemos qué quiso escribir el usuario.
    #[test]
    fn sidecar_con_varias_claves_raiz_no_se_diagnostica_como_envoltorio() {
        let texto =
            "limites:\n  a:\n    tipo: rango\nmedir:\n  tipo: rango\n  min: 1.0\n  max: 2.0\n";
        let original = noyalib::from_str::<HashMap<String, LimiteYaml>>(texto).unwrap_err();
        let err = diagnostica_sidecar(texto, original);
        assert!(matches!(&err, ErrorCarga::Sintaxis(_)), "{err}");
    }

    #[test]
    fn cargar_limites_de_texto_valida_entradas() {
        // Versión sin disco de cargar_limites_de_archivo para testear directo.
        let texto = "\
medir_voltaje:
  type: range
  min: 4.5
  max: 5.5
verificar_frecuencia:
  type: comparison
  op: ge
  expected: 1000.0
";
        let mapa: HashMap<String, LimiteYaml> = noyalib::from_str(texto).unwrap();
        let lim: HashMap<String, Limite> = mapa
            .into_iter()
            .map(|(n, l)| Ok((n.clone(), l.a_limite(&n)?)))
            .collect::<Result<_, ErrorCarga>>()
            .unwrap();
        assert_eq!(
            lim.get("medir_voltaje"),
            Some(&Limite::Rango { min: 4.5, max: 5.5 })
        );
        assert_eq!(
            lim.get("verificar_frecuencia"),
            Some(&Limite::Comparacion {
                op: Operador::Ge,
                esperado: 1000.0
            })
        );
    }

    #[test]
    fn yaml_mal_formado_es_error_de_sintaxis() {
        let err = cargar_de_texto("nombre: [sin cerrar").unwrap_err();
        assert!(matches!(&err, ErrorCarga::Sintaxis(_)));
    }

    // --- M4: variables, precondición, asigna, statement, control de flujo ---

    #[test]
    fn locals_file_globals_y_parameters_se_cargan_con_tipos_inferidos() {
        let yaml = "\
name: s
file_globals:
  lote: \"A-2026-08\"
  umbral: 4.5
locals:
  voltaje: 0.0
  ok: false
parameters: {}
main:
  - name: un_paso
";
        let s = cargar_de_texto(yaml).unwrap();
        assert_eq!(
            s.file_globals.get("lote"),
            Some(&ValorDefinicion::Texto("A-2026-08".into()))
        );
        assert_eq!(
            s.file_globals.get("umbral"),
            Some(&ValorDefinicion::Numero(4.5))
        );
        assert_eq!(s.locals.get("voltaje"), Some(&ValorDefinicion::Numero(0.0)));
        assert_eq!(s.locals.get("ok"), Some(&ValorDefinicion::Bool(false)));
        assert!(s.parameters.is_empty());
    }

    #[test]
    fn disable_y_pause_on_fail_se_aceptan_y_por_defecto_son_false() {
        let yaml = "\
name: s
main:
  - name: un_paso
    disable: true
    pause_on_fail: true
";
        let s = cargar_de_texto(yaml).unwrap();
        assert!(s.pasos_main[0].disable);
        assert!(s.pasos_main[0].pause_on_fail);
        // Sin los campos: defaults false (compat con M3).
        let s2 = cargar_de_texto("name: s\nmain:\n  - name: otro\n").unwrap();
        assert!(!s2.pasos_main[0].disable);
        assert!(!s2.pasos_main[0].pause_on_fail);
    }

    #[test]
    fn precondicion_se_parsea_a_ast() {
        // Este test llevaba de ejemplo `... && resultado.valor_medido !=
        // nothing`, que es justo el patrón que la campaña propagó a 51
        // precondiciones y que hoy es error de carga (§5 del informe).
        let yaml = "\
name: s
locals:
  contador: 0
  listo: false
main:
  - name: medir
    precondition: 'locals.contador > 0 && locals.listo'
";
        let s = cargar_de_texto(yaml).unwrap();
        assert!(
            s.pasos_main[0].precondicion.is_some(),
            "la precondición debe parsearse a AST"
        );
    }

    // --- §5 del informe de beta: `resultado.*` fuera de `asigna` ---

    /// El mensaje tiene que decir dónde está el uso y dónde sí vale: el
    /// diagnóstico que faltó en la campaña.
    fn error_de(yaml: &str) -> String {
        match cargar_de_texto(yaml) {
            Err(ErrorCarga::Validacion(m)) => m,
            otro => panic!("se esperaba error de validación, no {otro:?}"),
        }
    }

    #[test]
    fn resultado_en_precondicion_es_error_de_carga() {
        // El caso literal de la campaña: una conjunción cuyo segundo término
        // es un `false` constante. Antes cargaba, saltaba el paso y salía verde.
        let m = error_de(
            "\
name: s
locals:
  v_real: 0.0
main:
  - name: medir
    precondition: 'locals.v_real > 4.9 && result.measured_value != nothing'
",
        );
        assert!(m.contains("medir"), "nombra el paso: {m}");
        assert!(m.contains("result.measured_value"), "nombra el campo: {m}");
        assert!(m.contains("precondition"), "ubica el campo YAML: {m}");
        assert!(m.contains("assign"), "dice dónde sí vale: {m}");
    }

    #[test]
    fn resultado_en_condicion_de_pass_fail_es_error_de_carga() {
        let m = error_de(
            "\
name: s
main:
  - name: veredicto
    type: pass_fail
    condition: 'result.measured_value > 4.5'
",
        );
        assert!(m.contains("veredicto") && m.contains("condition"), "{m}");
    }

    #[test]
    fn resultado_en_statement_es_error_de_carga_leyendo_y_escribiendo() {
        // Como lectura en el lado derecho...
        let m = error_de(
            "\
name: s
locals:
  v: 0.0
main:
  - name: copiar
    type: statement
    statement: 'locals.v = result.measured_value'
",
        );
        assert!(m.contains("copiar") && m.contains("statement"), "{m}");

        // ...y como lvalue, que es el mismo malentendido al revés.
        let m = error_de(
            "\
name: s
main:
  - name: escribir
    type: statement
    statement: 'result.measured_value = 1.0'
",
        );
        assert!(m.contains("result.measured_value"), "{m}");
    }

    // --- ADR-0019, regla de detección: lo comprobable sin ejecutar, al cargar ---

    /// Issue #27, caso 1: el typo que destruía una variable y dejaba la
    /// secuencia en verde. Los campos de `resultado` son tres y conocidos, así
    /// que esto se ve en el escritorio, no con la unidad en el banco.
    #[test]
    fn asigna_desde_un_campo_inexistente_de_resultado_es_error_de_carga() {
        let m = error_de(
            "\
name: r27c
locals:
  val: 0.0
main:
  - name: medir_voltaje
    assign:
      val: '${result.measured_valu}'
",
        );
        assert!(m.contains("medir_voltaje"), "nombra el paso: {m}");
        assert!(m.contains("measured_valu"), "nombra el campo escrito: {m}");
        for campo in modelo::CAMPOS_RESULTADO {
            assert!(m.contains(&format!("'{campo}'")), "enumera '{campo}': {m}");
        }
    }

    /// El typo dentro de una expresión compuesta también se caza: el recorrido
    /// es del AST entero, no de la raíz.
    #[test]
    fn el_campo_inexistente_se_caza_dentro_de_una_expresion() {
        let m = error_de(
            "\
name: s
locals:
  ok: false
main:
  - name: medir
    assign:
      ok: '${result.measured_value > 1.0 && result.stauts == \"pass\"}'
",
        );
        assert!(m.contains("stauts"), "{m}");
    }

    /// Y los tres campos buenos siguen cargando, claro.
    #[test]
    fn asigna_desde_los_campos_buenos_carga() {
        let s = cargar_de_texto(
            "\
name: s
locals:
  v: 0.0
  e: ''
  msg: ''
main:
  - name: medir
    assign:
      v: '${result.measured_value}'
      e: '${result.status}'
      msg: '${result.message}'
",
        )
        .expect("los tres campos son válidos");
        assert_eq!(s.pasos_main[0].asigna.as_ref().unwrap().len(), 3);
    }

    /// Issue #27, caso 3: `asigna` sobre un `statement` era un no-op que
    /// `--validate` aprobaba. El caso hermano (`pass_fail`) ya se rechazaba;
    /// el mensaje sigue su tono, y dice qué hacer en su lugar.
    #[test]
    fn asigna_sobre_un_statement_es_error_de_carga() {
        let m = error_de(
            "\
name: r27e
locals:
  val: 0.0
main:
  - name: stmt
    type: statement
    statement: 'locals.val = 99.0'
    assign:
      val: '${result.measured_value}'
",
        );
        assert!(m.contains("stmt"), "nombra el paso: {m}");
        assert!(m.contains("no produce 'result.*'"), "el motivo: {m}");
        assert!(m.contains("locals.x = …"), "qué hacer en su lugar: {m}");
    }

    #[test]
    fn resultado_en_asigna_sigue_siendo_valido() {
        // `asigna` es su sitio: ahí el motor sí lo tiene ligado.
        let yaml = "\
name: s
locals:
  v: 0.0
main:
  - name: medir
    assign:
      v: 'result.measured_value'
";
        let s = cargar_de_texto(yaml).unwrap();
        assert!(s.pasos_main[0].asigna.is_some());
    }

    #[test]
    fn resultado_anidado_en_la_expresion_tambien_se_detecta() {
        // Mirar sólo la raíz del AST no habría bastado: el caso real estaba
        // dentro de un `&&`, y aquí va aún más hondo.
        let m = error_de(
            "\
name: s
locals:
  v: 0.0
main:
  - name: medir
    precondition: '!(locals.v > 1.0 && (result.measured_value < 2.0 || false))'
",
        );
        assert!(m.contains("result.measured_value"), "{m}");
    }

    #[test]
    fn precondicion_mal_formada_es_error_de_validacion_con_nombre() {
        let yaml = "\
name: s
main:
  - name: medir
    precondition: 'locals.contador >'
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("medir") && m.contains("precondición")),
            "el error debe mencionar el paso y la sección: {err}"
        );
    }

    #[test]
    fn asigna_se_parsea_y_acepta_las_dos_formas() {
        let yaml = "\
name: s
locals:
  voltaje: 0.0
  ok: false
main:
  - name: medir
    assign:
      voltaje: result.measured_value
      ok: '${result.status == \"paso\"}'
";
        let s = cargar_de_texto(yaml).unwrap();
        let asigna = s.pasos_main[0].asigna.as_ref().unwrap();
        assert_eq!(asigna.len(), 2);
        // Ambas formas llegan a un AST (no a texto).
        assert!(asigna.iter().any(|a| a.var == "voltaje"));
        assert!(asigna.iter().any(|a| a.var == "ok"));
    }

    #[test]
    fn asigna_mal_formada_es_error_con_var_y_paso() {
        let yaml = "\
name: s
main:
  - name: medir
    assign:
      x: 'result.measured_value +'
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("'x'") && m.contains("medir")),
            "el error debe mencionar la var y el paso: {err}"
        );
    }

    #[test]
    fn tipo_statement_sin_statement_es_error() {
        let yaml = "\
name: s
main:
  - name: init
    type: statement
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("statement")),
            "{err}"
        );
    }

    #[test]
    fn tipo_grpc_con_statement_es_error() {
        let yaml = "\
name: s
main:
  - name: init
    type: grpc
    statement: 'locals.x = 1'
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("statement")),
            "{err}"
        );
    }

    #[test]
    fn statement_se_parsea_a_sentencias() {
        let yaml = "\
name: s
locals:
  ok: false
  contador: 0
main:
  - name: init
    type: statement
    statement: 'locals.ok = false; locals.contador = 0'
";
        let s = cargar_de_texto(yaml).unwrap();
        let stmts = s.pasos_main[0].statement.as_ref().unwrap();
        assert_eq!(stmts.len(), 2, "dos sentencias separadas por ';'");
    }

    // --- DEF-3: `asigna`/`statement` sobre un destino no declarado ---

    #[test]
    fn asigna_sobre_un_parameter_declarado_es_error() {
        // DEF-3 del informe de beta: `asigna` escribe siempre en locals; que
        // el destino coincida con un `parameter` era un local homónimo
        // silencioso, sin avisar ni al cargar ni al ejecutar.
        let yaml = "\
name: s
parameters:
  p: 0.0
main:
  - name: medir_voltaje
    assign: { p: '${result.measured_value}' }
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m)
                if m.contains("medir_voltaje") && m.contains("'p'") && m.contains("parameters")),
            "el error debe nombrar el paso, la variable y 'parameters': {err}"
        );
    }

    #[test]
    fn asigna_a_un_local_no_declarado_es_error() {
        // Mismo footgun por la otra vía: un typo en el destino crea un local
        // nuevo en vez de fallar, y quien lea el nombre bien escrito no ve
        // nunca el valor.
        let yaml = "\
name: s
locals:
  voltaje: 0.0
main:
  - name: medir_voltaje
    assign: { voltage: '${result.measured_value}' }
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m)
                if m.contains("medir_voltaje") && m.contains("'voltage'") && m.contains("locals")),
            "el error debe nombrar el paso, la variable y 'locals': {err}"
        );
    }

    #[test]
    fn statement_a_un_local_no_declarado_es_error() {
        let yaml = "\
name: s
main:
  - name: init
    type: statement
    statement: 'locals.x = 1'
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m)
                if m.contains("init") && m.contains("locals.x")),
            "{err}"
        );
    }

    #[test]
    fn statement_a_un_parameter_no_declarado_es_error() {
        let yaml = "\
name: s
main:
  - name: init
    type: statement
    statement: 'parameters.p = 1'
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m)
                if m.contains("init") && m.contains("parameters.p")),
            "{err}"
        );
    }

    #[test]
    fn statement_a_parameter_declarado_es_valido() {
        // No debe romper el canal de retorno by-reference de una subsecuencia
        // (patrón de ejemplos/medir_fuentes.yaml).
        let yaml = "\
name: s
parameters:
  canal: 0.0
main:
  - name: ajustar_canal
    type: statement
    statement: 'parameters.canal = parameters.canal + 1.0'
";
        assert!(cargar_de_texto(yaml).is_ok());
    }

    #[test]
    fn asigna_sobre_parameter_declarado_en_subsecuencia_inline_es_error() {
        // La validación baja también a las inline, con sus propios scopes.
        let yaml = "\
name: s
subsequences:
  init:
    parameters:
      p: 0.0
    main:
      - name: medir
        assign: { p: '${result.measured_value}' }
main:
  - name: c
    type: sequence_call
    sequence: init
    args: {}
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m)
                if m.contains("medir") && m.contains("'p'") && m.contains("parameters")),
            "{err}"
        );
    }

    #[test]
    fn tipo_desconocido_es_error() {
        let yaml = "\
name: s
main:
  - name: init
    type: magia
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("magia")),
            "{err}"
        );
    }

    #[test]
    fn tipo_omitido_es_grpc_por_defecto() {
        let s = cargar_de_texto("name: s\nmain:\n  - name: un_paso\n").unwrap();
        assert_eq!(s.pasos_main[0].tipo, modelo::TipoPaso::Grpc);
    }

    /// End-to-end: el ejemplo `ejemplos/variables.yaml` carga con todos los
    /// campos de M4 (scopes, precondición, asigna, statement, disable,
    /// pause_on_fail). Valida que el schema admite el ejemplo de referencia.
    #[test]
    fn ejemplo_variables_yaml_carga() {
        let ruta = format!(
            "{}/../../ejemplos/variables.yaml",
            env!("CARGO_MANIFEST_DIR")
        );
        let s = cargar_de_archivo(&ruta).unwrap_or_else(|e| panic!("no carga {ruta}: {e}"));
        assert_eq!(s.nombre, "variables_demo");
        assert_eq!(s.file_globals.len(), 2, "lote + umbral_min");
        assert_eq!(s.locals.len(), 2, "voltaje_leido + ok");
        // init_log es statement con sentencia.
        let init = &s.pasos_main[0];
        assert_eq!(init.tipo, modelo::TipoPaso::Statement);
        assert!(init.statement.is_some());
        // medir_voltaje: precondición + límite + asigna (2).
        let medir = &s.pasos_main[1];
        assert!(medir.precondicion.is_some());
        assert!(medir.limite.is_some());
        assert_eq!(medir.asigna.as_ref().unwrap().len(), 2);
        // paso_obsoleto: disable.
        assert!(s.pasos_main[2].disable);
        // verificar_frecuencia: pause_on_fail.
        assert!(s.pasos_main[3].pause_on_fail);
    }

    // ---- M5-ext.1: ejecutores ----

    /// `ejecutores:` con un embebido y un grpc; pasos que los referencian
    /// por nombre → `Programa.ejecutores` correcto.
    #[test]
    fn ejecutores_yaml_se_parsean() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "parse"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(
            &y,
            "\
name: demo
executors:
  - { name: embebido, type: embedded }
  - { name: python, type: grpc, host: 127.0.0.1, port: 9101 }
main:
  - name: a
  - name: b
    executor: python
",
        )
        .unwrap();
        let prog = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap();
        assert_eq!(prog.ejecutores.len(), 2);
        assert_eq!(prog.ejecutores["embebido"].tipo, TipoEjecutor::Embebido);
        assert_eq!(
            prog.ejecutores["python"].tipo,
            TipoEjecutor::Grpc {
                host: "127.0.0.1".into(),
                puerto: 9101
            }
        );
        // Pasos: el primero sin ejecutor (embebido por defecto), el segundo python.
        assert_eq!(prog.raiz.pasos_main[0].ejecutor, None);
        assert_eq!(prog.raiz.pasos_main[1].ejecutor.as_deref(), Some("python"));
    }

    /// Sin `ejecutores:` → tabla vacía y todos los pasos al embebido (compat M4b).
    #[test]
    fn sin_ejecutores_tabla_vacia() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "vacio"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "name: s\nmain:\n  - name: a\n").unwrap();
        let prog = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap();
        assert!(prog.ejecutores.is_empty());
        assert_eq!(prog.raiz.pasos_main[0].ejecutor, None);
    }

    /// Un paso con `ejecutor: X` donde X no está declarado → error al cargar.
    #[test]
    fn ejecutor_no_declarado_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "indef"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "name: s\nmain:\n  - name: a\n    executor: inventado\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("inventado")),
            "{err}"
        );
    }

    /// `tipo: wasm` con `path` absoluto → error que explica el sandbox
    /// (DEF-4), no que el fichero "no existe" (el fichero sí existe en
    /// disco; el cargador solo no puede verlo desde su sandbox).
    #[test]
    fn wasm_con_path_absoluto_es_error_explicativo() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "wasm_absoluto"));
        std::fs::create_dir_all(&dir).unwrap();
        let wasm = dir.join("p.wasm");
        std::fs::write(&wasm, b"\0asm").unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(
            &y,
            format!(
                "name: s\nexecutors:\n  - {{ name: p, type: wasm, path: {} }}\nmain:\n  - name: a\n",
                wasm.display()
            ),
        )
        .unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("absoluto") && !m.contains("no existe")),
            "{err}"
        );
    }

    /// `tipo: wasm` sin `path` → error; con `path` inexistente → error.
    #[test]
    fn wasm_sin_path_o_inexistente_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "wasm"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(
            &y,
            "name: s\nexecutors:\n  - { name: p, type: wasm }\nmain:\n  - name: a\n",
        )
        .unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("'path'")),
            "{err}"
        );

        std::fs::write(&y, "name: s\nexecutors:\n  - { name: p, type: wasm, path: ./no_existe.wasm }\nmain:\n  - name: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("no existe")),
            "{err}"
        );
    }

    /// `tipo: wasm` con un path que sí existe (se crea en el dir) → carga OK.
    #[test]
    fn wasm_con_path_existente_carga() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "wasm_ok"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.wasm"), b"\0asm").unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "name: s\nexecutors:\n  - { name: p, type: wasm, path: ./p.wasm }\nmain:\n  - name: a\n    executor: p\n").unwrap();
        let prog = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap();
        assert_eq!(
            prog.ejecutores["p"].tipo,
            TipoEjecutor::Wasm {
                path: "./p.wasm".into()
            }
        );
    }

    /// `grpc` sin `host`/`puerto` → error; `grpc` con `path` → error.
    #[test]
    fn grpc_incompleto_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "grpc"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "name: s\nexecutors:\n  - { name: p, type: grpc, host: 127.0.0.1 }\nmain:\n  - name: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("'host' y 'port'")),
            "{err}"
        );

        std::fs::write(&y, "name: s\nexecutors:\n  - { name: p, type: grpc, host: 127.0.0.1, port: 9101, path: ./p.wasm }\nmain:\n  - name: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("'path'")),
            "{err}"
        );
    }

    /// `embebido` con campos de más → error; tipo desconocido → error.
    #[test]
    fn embebido_con_campos_y_tipo_desconocido_son_errores() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "emb"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "name: s\nexecutors:\n  - { name: e, type: embedded, path: ./p.wasm }\nmain:\n  - name: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("no aplican")),
            "{err}"
        );

        std::fs::write(
            &y,
            "name: s\nexecutors:\n  - { name: e, type: raro }\nmain:\n  - name: a\n",
        )
        .unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("raro")),
            "{err}"
        );
    }

    /// Dos ejecutores con el mismo nombre → error. Nombre reservado → error.
    #[test]
    fn nombres_duplicados_y_reservados_son_errores() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "dups"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "name: s\nexecutors:\n  - { name: a, type: embedded }\n  - { name: a, type: embedded }\nmain:\n  - name: p\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("más de una vez")),
            "{err}"
        );

        std::fs::write(&y, format!("name: s\nexecutors:\n  - {{ name: {NOMBRE_EMBEDIDO_RESERVADO}, type: embedded }}\nmain:\n  - name: p\n")).unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("reservado")),
            "{err}"
        );
    }

    /// `ejecutor` en un paso `statement`/`sequence_call` → error (es gRPC-only).
    #[test]
    fn ejecutor_en_paso_no_grpc_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "tipo"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "name: s\nexecutors:\n  - { name: e, type: embedded }\nmain:\n  - name: a\n    type: statement\n    statement: 'locals.x = 1'\n    executor: e\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("reservado para 'grpc'")),
            "{err}"
        );
    }

    /// `deny_unknown_fields` sigue rechazando campos raros en un ejecutor.
    #[test]
    fn ejecutor_con_campo_desconocido_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "deny"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(
            &y,
            "name: s\nexecutors:\n  - { name: e, type: embedded, foo: bar }\nmain:\n  - name: a\n",
        )
        .unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Diagnostico(m) if m.contains("executors[0]")),
            "{err}"
        );
    }

    /// M5-ext.1: el ejemplo `ejemplos/demo_ejecutores.yaml` carga como
    /// programa: la tabla `ejecutores:` se traduce y los pasos que la
    /// referencian se enlazan.
    #[test]
    fn ejemplo_demo_ejecutores_carga_como_programa() {
        let ruta = format!(
            "{}/../../ejemplos/demo_ejecutores.yaml",
            env!("CARGO_MANIFEST_DIR")
        );
        let prog = cargar_programa_de_archivo(&ruta)
            .unwrap_or_else(|e| panic!("no carga el programa {ruta}: {e}"));
        assert_eq!(prog.raiz.nombre, "demo_ejecutores");
        assert_eq!(prog.ejecutores.len(), 2, "embebido + python");
        assert_eq!(
            prog.ejecutores["python"].tipo,
            TipoEjecutor::Grpc {
                host: "127.0.0.1".into(),
                puerto: 9101
            }
        );
        assert_eq!(
            prog.raiz.pasos_main[0].ejecutor, None,
            "verificar_led → embebido"
        );
        assert_eq!(
            prog.raiz.pasos_main[1].ejecutor.as_deref(),
            Some("python"),
            "medir_simulador → python"
        );
        assert_eq!(
            prog.raiz.pasos_main[2].ejecutor.as_deref(),
            Some("python"),
            "conectar_equipo → python"
        );
    }

    /// Override `--executor nombre=host:puerto`: re-apunta un grpc, convierte
    /// un embebido, y falla si el nombre no está declarado.
    #[test]
    fn override_de_ejecutores() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "override"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "name: s\nexecutors:\n  - { name: e, type: embedded }\n  - { name: py, type: grpc, host: 127.0.0.1, port: 9101 }\nmain:\n  - name: a\n").unwrap();
        let mut prog = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap();

        // Re-apuntar un grpc a remoto.
        let n =
            aplicar_override_ejecutores(&mut prog, &["py=192.168.1.50:9200".to_string()]).unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            prog.ejecutores["py"].tipo,
            TipoEjecutor::Grpc {
                host: "192.168.1.50".into(),
                puerto: 9200
            }
        );

        // Convertir un embebido en grpc (el usuario fuerza remoto).
        let n =
            aplicar_override_ejecutores(&mut prog, &["e=192.168.1.60:9300".to_string()]).unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            prog.ejecutores["e"].tipo,
            TipoEjecutor::Grpc {
                host: "192.168.1.60".into(),
                puerto: 9300
            }
        );

        // Formato inválido → error; nombre no declarado → error.
        let err = aplicar_override_ejecutores(&mut prog, &["mal_formado".to_string()]).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("esperado")),
            "{err}"
        );
        let err =
            aplicar_override_ejecutores(&mut prog, &["zzz=1.2.3.4:1".to_string()]).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("no está declarado")),
            "{err}"
        );
    }

    // ---- M5: process model Sequential (RF-38) ----

    fn pm_yaml() -> &'static str {
        "\
name: sequential
locals: { uut_id: \"\", estado_usuario: \"\" }
setup:
  - name: identificar_uut
    retries: 1
    assign: { uut_id: \"${result.message}\" }
main:
  - name: correr_secuencia_usuario
    type: sequence_call
    sequence: secuencia_usuario
    assign: { estado_usuario: \"${result.status}\" }
cleanup:
  - name: notificar_resultado
    retries: 1
"
    }

    /// PM canónico + usuario `basica.yaml` (sin parameters): el cargador
    /// reescribe el placeholder al path canónico del usuario y lo registra.
    fn dir_pm(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("anvil_m5_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn pm_canonico_resuelve_y_reescribe_el_placeholder() {
        let dir = dir_pm("ok");
        std::fs::write(dir.join("pm.yaml"), pm_yaml()).unwrap();
        std::fs::write(dir.join("usuario.yaml"), basica_yaml()).unwrap();
        let prog = cargar_programa_con_pm(
            dir.join("pm.yaml").to_str().unwrap(),
            dir.join("usuario.yaml").to_str().unwrap(),
        )
        .unwrap();
        let call = &prog.raiz.pasos_main[0];
        let clave = call.secuencia.as_deref().unwrap();
        assert_ne!(clave, SECUENCIA_USUARIO, "el placeholder se reescribió");
        assert!(es_path(clave), "ahora es un path canónico: {clave}");
        assert_eq!(
            prog.archivos.get(clave).map(|d| d.nombre.as_str()),
            Some("basica"),
            "el usuario quedó registrado bajo su clave canónica"
        );
    }

    #[test]
    fn pm_sin_call_a_secuencia_usuario_es_error() {
        let dir = dir_pm("sin_call");
        std::fs::write(dir.join("pm.yaml"), "name: pm\nmain:\n  - name: x\n").unwrap();
        std::fs::write(dir.join("usuario.yaml"), basica_yaml()).unwrap();
        let err = cargar_programa_con_pm(
            dir.join("pm.yaml").to_str().unwrap(),
            dir.join("usuario.yaml").to_str().unwrap(),
        )
        .unwrap_err();
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("secuencia_usuario")));
    }

    #[test]
    fn pm_con_dos_calls_a_secuencia_usuario_es_error() {
        let dir = dir_pm("dos_calls");
        std::fs::write(
            dir.join("pm.yaml"),
            "name: pm\nmain:\n  - name: a\n    type: sequence_call\n    sequence: secuencia_usuario\n  - name: b\n    type: sequence_call\n    sequence: secuencia_usuario\n",
        )
        .unwrap();
        std::fs::write(dir.join("usuario.yaml"), basica_yaml()).unwrap();
        let err = cargar_programa_con_pm(
            dir.join("pm.yaml").to_str().unwrap(),
            dir.join("usuario.yaml").to_str().unwrap(),
        )
        .unwrap_err();
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("más de un")));
    }

    #[test]
    fn pm_con_secuencia_usuario_en_subsecuencias_es_error() {
        let dir = dir_pm("reservado");
        std::fs::write(
            dir.join("pm.yaml"),
            "name: pm\nsubsequences:\n  secuencia_usuario:\n    name: secuencia_usuario\n    main:\n      - name: x\nmain:\n  - name: a\n    type: sequence_call\n    sequence: secuencia_usuario\n",
        )
        .unwrap();
        std::fs::write(dir.join("usuario.yaml"), basica_yaml()).unwrap();
        let err = cargar_programa_con_pm(
            dir.join("pm.yaml").to_str().unwrap(),
            dir.join("usuario.yaml").to_str().unwrap(),
        )
        .unwrap_err();
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("reservado")));
    }

    #[test]
    fn pm_usuario_con_subsecuencias_externas_las_resuelve() {
        // El usuario es `ejemplos/subsecuencia.yaml`, que referencia a
        // `./medir_fuentes.yaml` y a la inline `init_comun`. El PM debe
        // resolver tanto al usuario como a sus dependencias externas.
        let dir = dir_pm("subs");
        std::fs::write(dir.join("pm.yaml"), pm_yaml()).unwrap();
        std::fs::copy(
            format!(
                "{}/../../ejemplos/subsecuencia.yaml",
                env!("CARGO_MANIFEST_DIR")
            ),
            dir.join("subsecuencia.yaml"),
        )
        .unwrap();
        std::fs::copy(
            format!(
                "{}/../../ejemplos/medir_fuentes.yaml",
                env!("CARGO_MANIFEST_DIR")
            ),
            dir.join("medir_fuentes.yaml"),
        )
        .unwrap();
        let prog = cargar_programa_con_pm(
            dir.join("pm.yaml").to_str().unwrap(),
            dir.join("subsecuencia.yaml").to_str().unwrap(),
        )
        .unwrap();
        // archivos contiene al usuario + a medir_fuentes.yaml (su externa).
        assert!(
            prog.archivos.len() >= 2,
            "usuario + subsecuencia externa del usuario"
        );
        let call = &prog.raiz.pasos_main[0];
        assert_ne!(call.secuencia.as_deref().unwrap(), SECUENCIA_USUARIO);
    }

    #[test]
    fn pm_canonico_sin_parametros_exige_usuario_sin_parameters() {
        // El PM canónico declara sin `parametros`; un usuario con
        // `parameters` no encaja la firma (vacía != {p}).
        let dir = dir_pm("firma");
        std::fs::write(dir.join("pm.yaml"), pm_yaml()).unwrap();
        std::fs::write(
            dir.join("usuario.yaml"),
            "name: u\nparameters: { p: 0.0 }\nmain:\n  - name: m\n",
        )
        .unwrap();
        let err = cargar_programa_con_pm(
            dir.join("pm.yaml").to_str().unwrap(),
            dir.join("usuario.yaml").to_str().unwrap(),
        )
        .unwrap_err();
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("firma")));
    }

    #[test]
    fn pm_ciclo_pm_usuario_a_pm_es_error() {
        // El usuario llama de vuelta al PM (por path) → ciclo al cargar.
        let dir = dir_pm("ciclo");
        std::fs::write(dir.join("pm.yaml"), pm_yaml()).unwrap();
        std::fs::write(
            dir.join("usuario.yaml"),
            "name: u\nmain:\n  - name: vuelta\n    type: sequence_call\n    sequence: ./pm.yaml\n",
        )
        .unwrap();
        let err = cargar_programa_con_pm(
            dir.join("pm.yaml").to_str().unwrap(),
            dir.join("usuario.yaml").to_str().unwrap(),
        )
        .unwrap_err();
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("ciclo")));
    }

    #[test]
    fn pm_usuario_sin_parameters_pasa() {
        let dir = dir_pm("pasa");
        std::fs::write(dir.join("pm.yaml"), pm_yaml()).unwrap();
        std::fs::write(dir.join("usuario.yaml"), basica_yaml()).unwrap();
        let prog = cargar_programa_con_pm(
            dir.join("pm.yaml").to_str().unwrap(),
            dir.join("usuario.yaml").to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(prog.raiz.nombre, "sequential");
    }

    #[test]
    fn ejemplo_sequential_carga_como_programa() {
        // El PM canónico de `process_models/sequential.yaml` envuelve a
        // `ejemplos/basica.yaml`. Smoke de integración de la convención.
        let pm = format!(
            "{}/../../process_models/sequential.yaml",
            env!("CARGO_MANIFEST_DIR")
        );
        let usuario = format!("{}/../../ejemplos/basica.yaml", env!("CARGO_MANIFEST_DIR"));
        let prog = cargar_programa_con_pm(&pm, &usuario)
            .unwrap_or_else(|e| panic!("no carga el PM {pm} con {usuario}: {e}"));
        assert_eq!(prog.raiz.nombre, "sequential");
        let call = &prog.raiz.pasos_main[0];
        assert_ne!(call.secuencia.as_deref().unwrap(), SECUENCIA_USUARIO);
        assert_eq!(
            prog.archivos
                .get(call.secuencia.as_deref().unwrap())
                .map(|d| d.nombre.as_str()),
            Some("basica")
        );
    }

    // --- Issue #19: leer una variable no declarada es error de carga ---
    //
    // `validar_lvalues` miraba dónde se escribe y nadie miraba dónde se lee:
    // `--validate` aprobaba la secuencia y la corrida moría a mitad, con la
    // unidad medio probada. Los scopes son estrictos según el manual; estos
    // tests son lo que lo hace verdad.

    #[test]
    fn precondicion_con_un_local_no_declarado_es_error_de_carga() {
        let m = error_de(
            "\
name: s
locals:
  v: 0.0
main:
  - name: medir
    precondition: 'locals.no_existe > 0.0'
",
        );
        assert!(m.contains("locals.no_existe"), "{m}");
        assert!(m.contains("precondition"), "dice en qué campo: {m}");
        assert!(m.contains("medir"), "dice en qué paso: {m}");
    }

    #[test]
    fn condicion_de_pass_fail_con_una_variable_no_declarada_es_error() {
        let m = error_de(
            "\
name: s
locals:
  v: 0.0
main:
  - name: verdict
    type: pass_fail
    condition: 'file_globals.umbral > 1.0'
",
        );
        assert!(m.contains("file_globals.umbral"), "{m}");
        assert!(m.contains("condition"), "{m}");
    }

    /// El que se ve fallar al revertir: el **lvalue** sí está declarado, así
    /// que `validar_lvalues` lo aprueba; lo no declarado está a la derecha.
    #[test]
    fn el_lado_derecho_de_un_statement_valida_las_lecturas() {
        let m = error_de(
            "\
name: s
locals:
  x: 0.0
main:
  - name: calcula
    type: statement
    statement: 'locals.x = locals.y + 1.0'
",
        );
        assert!(m.contains("locals.y"), "{m}");
        assert!(m.contains("statement"), "{m}");
    }

    #[test]
    fn el_lado_derecho_de_un_asigna_valida_las_lecturas() {
        let m = error_de(
            "\
name: s
locals:
  x: 0.0
main:
  - name: medir
    assign:
      x: '${result.measured_value + locals.offset}'
",
        );
        assert!(m.contains("locals.offset"), "{m}");
        assert!(m.contains("assign"), "{m}");
    }

    /// El caso de la campaña era una conjunción: mirar sólo la raíz del AST
    /// no habría bastado. Mismo motivo que `primer_uso_de_resultado_si`.
    #[test]
    fn una_variable_no_declarada_anidada_en_la_expresion_tambien_se_detecta() {
        let m = error_de(
            "\
name: s
locals:
  a: true
main:
  - name: medir
    precondition: '!(locals.a && (parameters.p || true))'
",
        );
        assert!(m.contains("parameters.p"), "{m}");
    }

    /// Guarda contra el falso positivo, y contra que esto pise a `resultado.*`
    /// (que tiene sus propias dos validaciones y no se toca aquí).
    #[test]
    fn leer_variables_declaradas_en_los_tres_scopes_es_valido() {
        let s = cargar_de_texto(
            "\
name: s
file_globals:
  umbral: 4.0
parameters:
  canal: 0.0
locals:
  v: 0.0
  ok: false
main:
  - name: medir
    precondition: 'parameters.canal >= 0.0'
    assign:
      v: '${result.measured_value}'
  - name: calcula
    type: statement
    statement: 'locals.ok = locals.v > file_globals.umbral'
  - name: verdict
    type: pass_fail
    condition: 'locals.ok'
",
        );
        assert!(s.is_ok(), "no debe haber falso positivo: {s:?}");
    }

    /// No hay herencia de scopes: `EntornoMotor` materializa los de **su**
    /// definición. Rechazarlo al cargar coincide exactamente con el runtime.
    #[test]
    fn una_inline_valida_sus_lecturas_contra_sus_propias_declaraciones() {
        let m = error_de(
            "\
name: padre
locals:
  x: 1.0
subsequences:
  hija:
    locals:
      propia: 0.0
    main:
      - name: usa
        type: statement
        statement: 'locals.propia = locals.x + 1.0'
main:
  - name: c
    type: sequence_call
    sequence: hija
",
        );
        assert!(
            m.contains("locals.x"),
            "la hija lee un local que sólo existe en el padre: {m}"
        );
    }

    // --- Issue #17: escribir donde no se puede ---

    #[test]
    fn un_statement_que_escribe_en_file_globals_es_error_de_carga() {
        let m = error_de(
            "\
name: s
file_globals:
  counter: 0.0
locals:
  x: 0.0
main:
  - name: set_global
    type: statement
    statement: 'file_globals.counter = 1.0'
",
        );
        assert!(m.contains("file_globals.counter"), "{m}");
        assert!(m.contains("sólo lectura"), "dice por qué: {m}");
    }

    #[test]
    fn un_statement_que_escribe_en_parameters_de_la_raiz_es_error_de_carga() {
        let dir = std::env::temp_dir().join("anvil_17_params_raiz");
        std::fs::create_dir_all(&dir).unwrap();
        let raiz = dir.join("raiz.yaml");
        std::fs::write(
            &raiz,
            "name: raiz\nparameters: { val: 0.0 }\nlocals: { x: 0.0 }\nmain:\n  - name: set_param\n    type: statement\n    statement: 'parameters.val = 1.0'\n",
        )
        .unwrap();
        let m = match cargar_programa_de_archivo(raiz.to_str().unwrap()) {
            Err(ErrorCarga::Validacion(m)) => m,
            otro => panic!("se esperaba error de validación, no {otro:?}"),
        };
        assert!(m.contains("parameters.val"), "{m}");
        assert!(
            m.contains("raíz"),
            "dice que el problema es ser la raíz: {m}"
        );
    }

    /// La otra cara: desde una subsecuencia sí vale — es el modo documentado
    /// de devolver un valor al llamador (ADR-0010). Este test es el que impide
    /// que el arreglo de #17 se pase de frenada.
    #[test]
    fn un_statement_que_escribe_en_parameters_de_una_subsecuencia_es_valido() {
        let dir = std::env::temp_dir().join("anvil_17_params_sub");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("hija.yaml"),
            "name: hija\nparameters: { val: 0.0 }\nmain:\n  - name: devuelve\n    type: statement\n    statement: 'parameters.val = 1.0'\n",
        )
        .unwrap();
        let padre = dir.join("padre.yaml");
        std::fs::write(
            &padre,
            "name: padre\nlocals: { val: 0.0 }\nmain:\n  - name: c\n    type: sequence_call\n    sequence: ./hija.yaml\n    args: { val: locals.val }\n",
        )
        .unwrap();
        assert!(
            cargar_programa_de_archivo(padre.to_str().unwrap()).is_ok(),
            "escribir parameters desde una subsecuencia es legítimo"
        );
    }

    // --- Issue #20: un sequence_call no mide ---

    #[test]
    fn asigna_de_valor_medido_en_un_sequence_call_es_error_de_carga() {
        let m = error_de(
            "\
name: s
locals:
  my_num: 42.0
subsequences:
  hija:
    main:
      - name: p
        type: grpc
main:
  - name: call_sub
    type: sequence_call
    sequence: hija
    assign:
      my_num: '${result.measured_value}'
",
        );
        assert!(m.contains("measured_value"), "{m}");
        assert!(m.contains("no mide"), "dice por qué: {m}");
    }

    #[test]
    fn valor_medido_anidado_en_el_asigna_de_un_sequence_call_tambien_se_detecta() {
        let m = error_de(
            "\
name: s
locals:
  my_num: 42.0
subsequences:
  hija:
    main:
      - name: p
        type: grpc
main:
  - name: call_sub
    type: sequence_call
    sequence: hija
    assign:
      my_num: '${result.measured_value + 1.0}'
",
        );
        assert!(m.contains("measured_value"), "{m}");
    }

    /// `estado` y `mensaje` sí los produce un sequence call: siguen valiendo.
    /// Protege a `process_models/sequential.yaml` y `ejemplos/subsecuencia.yaml`.
    #[test]
    fn asigna_de_estado_en_un_sequence_call_sigue_siendo_valido() {
        let s = cargar_de_texto(
            "\
name: s
locals:
  veredicto: \"\"
subsequences:
  hija:
    main:
      - name: p
        type: grpc
main:
  - name: call_sub
    type: sequence_call
    sequence: hija
    assign:
      veredicto: '${result.status}'
",
        );
        assert!(s.is_ok(), "{s:?}");
    }

    // --- Issue #21: `ejecutores:` fuera de la raíz ---

    /// El caso silencioso: hoy devolvía `Ok` y descartaba la declaración de la
    /// hija, incluso contradiciendo a la de la raíz.
    #[test]
    fn una_subsecuencia_externa_con_ejecutores_es_error_de_carga() {
        let dir = std::env::temp_dir().join("anvil_21_ext");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("hija.yaml"),
            "name: hija\nexecutors:\n  - name: wasm_temp\n    type: grpc\n    host: 127.0.0.1\n    port: 9300\nmain:\n  - name: m\n    type: grpc\n",
        )
        .unwrap();
        let padre = dir.join("padre.yaml");
        std::fs::write(
            &padre,
            "name: padre\nmain:\n  - name: c\n    type: sequence_call\n    sequence: ./hija.yaml\n",
        )
        .unwrap();
        let m = match cargar_programa_de_archivo(padre.to_str().unwrap()) {
            Err(ErrorCarga::Validacion(m)) => m,
            otro => panic!("se esperaba error de validación, no {otro:?}"),
        };
        assert!(m.contains("executors"), "{m}");
        assert!(m.contains("raíz"), "dice dónde declararlos: {m}");
    }

    #[test]
    fn una_subsecuencia_inline_con_ejecutores_es_error_de_carga() {
        let m = error_de(
            "\
name: padre
subsequences:
  hija:
    executors:
      - name: e
        type: grpc
        host: 127.0.0.1
        port: 9300
    main:
      - name: m
        type: grpc
main:
  - name: c
    type: sequence_call
    sequence: hija
",
        );
        assert!(m.contains("executors"), "{m}");
        assert!(m.contains("raíz"), "{m}");
    }

    /// El caso (c1) del issue: el mensaje tiene que decir que el sitio es la
    /// raíz, no dejar al usuario creyendo que le falta en la subsecuencia.
    #[test]
    fn el_ejecutor_no_declarado_dice_donde_declararlo() {
        let dir = std::env::temp_dir().join("anvil_21_msg");
        std::fs::create_dir_all(&dir).unwrap();
        let raiz = dir.join("raiz.yaml");
        std::fs::write(
            &raiz,
            "name: raiz\nmain:\n  - name: m\n    type: grpc\n    executor: wasm_temp\n",
        )
        .unwrap();
        let m = match cargar_programa_de_archivo(raiz.to_str().unwrap()) {
            Err(ErrorCarga::Validacion(m)) => m,
            otro => panic!("se esperaba error de validación, no {otro:?}"),
        };
        assert!(m.contains("wasm_temp"), "{m}");
        assert!(m.contains("raíz"), "dice dónde declararlo: {m}");
    }

    /// El camino que **no** debe romperse: con `--process-model`, el PM y la
    /// secuencia del usuario sí pueden declarar `ejecutores:` (los dos entran
    /// por `cargar_de_archivo`, no por la cola de subsecuencias).
    #[test]
    fn con_process_model_el_pm_y_el_usuario_pueden_declarar_ejecutores() {
        let dir = std::env::temp_dir().join("anvil_21_pm");
        std::fs::create_dir_all(&dir).unwrap();
        let pm = dir.join("pm.yaml");
        std::fs::write(
            &pm,
            "name: sequential\nexecutors:\n  - name: del_pm\n    type: grpc\n    host: 127.0.0.1\n    port: 9300\nmain:\n  - name: correr\n    type: sequence_call\n    sequence: secuencia_usuario\n",
        )
        .unwrap();
        let usuario = dir.join("usuario.yaml");
        std::fs::write(
            &usuario,
            "name: usuario\nexecutors:\n  - name: del_usuario\n    type: grpc\n    host: 127.0.0.1\n    port: 9400\nmain:\n  - name: m\n    type: grpc\n    executor: del_usuario\n",
        )
        .unwrap();
        let prog = cargar_programa_con_pm(pm.to_str().unwrap(), usuario.to_str().unwrap()).unwrap();
        assert_eq!(prog.ejecutores.len(), 2, "los dos se fusionan");
    }
    // --- ADR-0020: parámetros by-value de un paso `grpc` -------------------

    /// La red de seguridad de la colisión de nombres. `parametros:` significa
    /// dos cosas según el `tipo`: en un `sequence_call` es by-reference y
    /// `{ canal: locals.canal }` es una **referencia** a la variable; en un
    /// `grpc` sería el **texto literal** `"locals.canal"`, y el paso mediría
    /// con una cadena en vez de con el número.
    ///
    /// Copiar un bloque de un sitio al otro no puede cambiar el significado
    /// en silencio (ADR-0019), así que ese caso no se traga.
    ///
    /// Visto en rojo quitando la comprobación de `SCOPES` en
    /// `entradas_de_paso`: el YAML carga sin rechistar y el parámetro viaja
    /// como texto.
    #[test]
    fn un_scope_sin_llaves_en_un_paso_grpc_no_se_traga_como_texto() {
        let yaml = "\
name: s
locals: { canal: 2.0 }
main:
  - name: medir
    inputs: { canal: locals.canal }
";
        let err = cargar_de_texto(yaml).unwrap_err();
        let ErrorCarga::Validacion(m) = &err else {
            panic!("tiene que ser error de validación, no de sintaxis: {err:?}");
        };
        assert!(m.contains("medir"), "nombra el paso: {m}");
        assert!(m.contains("canal"), "nombra el parámetro: {m}");
        assert!(m.contains("${locals.canal}"), "dice la forma correcta: {m}");
    }

    /// Y con `${...}` sí: es una expresión, y se parsea al cargar.
    #[test]
    fn un_scope_entre_llaves_es_una_expresion() {
        let yaml = "\
name: s
locals: { canal: 2.0 }
main:
  - name: medir
    inputs: { canal: '${locals.canal}' }
";
        let s = cargar_de_texto(yaml).unwrap();
        let e = s.pasos_main[0].entradas.as_ref().expect("hay parámetros");
        assert!(matches!(e[0], (ref n, EntradaPaso::Expresion(_)) if n == "canal"));
    }

    /// El tipo del literal es el del escalar YAML: `2` es número y `"2"` es
    /// texto. Es lo que viaja por el cable, así que confundirlos es medir
    /// otra cosa.
    #[test]
    fn el_tipo_del_literal_es_el_del_escalar_yaml() {
        let yaml = "\
name: s
main:
  - name: medir
    inputs:
      canal: 2
      etiqueta: '2'
      promediar: true
";
        let s = cargar_de_texto(yaml).unwrap();
        let e = s.pasos_main[0].entradas.as_ref().unwrap();
        // Ordenados por nombre: el orden del cable es determinista.
        assert_eq!(
            e,
            &vec![
                (
                    "canal".to_string(),
                    EntradaPaso::Literal(ValorDefinicion::Numero(2.0))
                ),
                (
                    "etiqueta".to_string(),
                    EntradaPaso::Literal(ValorDefinicion::Texto("2".into()))
                ),
                (
                    "promediar".to_string(),
                    EntradaPaso::Literal(ValorDefinicion::Bool(true))
                ),
            ]
        );
    }

    /// ADR-0020 §2: un `parametros:` que no es un mapa de escalares es error
    /// **de carga**, no de ejecución — es decidible sin banco.
    #[test]
    fn un_parametro_que_no_es_escalar_no_carga() {
        let yaml = "\
name: s
main:
  - name: medir
    inputs:
      canal: [1, 2]
";
        assert!(cargar_de_texto(yaml).is_err(), "una lista no es un escalar");
    }

    /// Un `sequence_call` sigue funcionando exactamente igual: su
    /// `parametros` es by-reference y no pasa por la ruta nueva.
    #[test]
    fn el_sequence_call_conserva_su_parametros_by_reference() {
        let yaml = "\
name: s
locals: { canal: 1.0 }
subsequences:
  hija:
    name: hija
    parameters: { canal: 0.0 }
    main:
      - name: p
main:
  - name: c
    type: sequence_call
    sequence: hija
    args: { canal: locals.canal }
";
        let s = cargar_de_texto(yaml).unwrap();
        let p = &s.pasos_main[0];
        assert!(p.entradas.is_none(), "un call no tiene parámetros by-value");
        assert_eq!(p.parametros.as_ref().unwrap()[0].param, "canal");
    }

    /// Y un `statement` sigue sin admitirlo, porque ahí no significa nada.
    #[test]
    fn un_statement_no_admite_parametros() {
        let yaml = "\
name: s
main:
  - name: p
    type: statement
    statement: 'locals.x = 1'
    inputs: { canal: 2 }
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("inputs")));
    }

    // ---------------------------------------------------------------------
    // ADR-0022: the object reference. What can be refused by reading the
    // files, with nothing connected and nothing measured.
    // ---------------------------------------------------------------------

    /// Writes a YAML into its own temp dir and loads it as a whole program,
    /// which is the unit these checks need: the declaration lives in a
    /// sequence and the `executors:` table it names lives in the root.
    fn programa_de(caso: &str, yaml: &str) -> Result<Programa, ErrorCarga> {
        let dir = std::env::temp_dir().join(format!("anvil_ref_{caso}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, yaml).unwrap();
        cargar_programa_de_archivo(y.to_str().unwrap())
    }

    const EJECUTORES_DOS: &str = "\
executors:
  - { name: banco, type: grpc, host: 127.0.0.1, port: 9101 }
  - { name: otro,  type: grpc, host: 127.0.0.1, port: 9102 }
";

    #[test]
    fn una_referencia_se_declara_con_su_ejecutor() {
        let prog = programa_de(
            "decl",
            &format!(
                "name: s\n{EJECUTORES_DOS}locals:\n  rack: {{ type: reference, executor: banco }}\n\
                 main:\n  - name: abrir\n    executor: banco\n    assign: {{ rack: result.outputs.rack }}\n"
            ),
        )
        .unwrap();
        assert_eq!(
            prog.raiz.locals["rack"],
            ValorDefinicion::Reference {
                executor: "banco".into()
            }
        );
        // Y no tiene valor: hasta que un paso la acuñe, ahí no hay nada.
        assert_eq!(prog.raiz.locals["rack"].a_value(), expr::Value::Nulo);
    }

    /// **Criterio 2 del encargo.** Una referencia que se le pasa a un paso de
    /// otro ejecutor se rechaza **antes de arrancar** — sin catálogo, sin red y
    /// sin `--with-executors`, porque lo único que hace falta es lo declarado.
    ///
    /// Visto fallar cambiando el `executor:` del paso a `banco`: el error
    /// desaparece, que es lo que dice que la comprobación mira a qué ejecutor
    /// se despacha y no simplemente que haya una referencia por medio.
    #[test]
    fn una_referencia_a_un_paso_de_otro_ejecutor_se_rechaza_al_cargar() {
        let yaml = format!(
            "name: s\n{EJECUTORES_DOS}locals:\n  rack: {{ type: reference, executor: banco }}\n\
             setup:\n  - name: abrir\n    executor: banco\n    assign: {{ rack: result.outputs.rack }}\n\
             main:\n  - name: medir\n    executor: otro\n    inputs: {{ rack: '${{locals.rack}}' }}\n"
        );
        let err = programa_de("cruzado", &yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m)
                if m.contains("medir") && m.contains("'otro'") && m.contains("banco")),
            "el error nombra el paso, el ejecutor al que se despacha y el dueño: {err}"
        );

        // El mismo YAML con el paso en su ejecutor carga sin queja.
        let bueno = yaml.replace("executor: otro", "executor: banco");
        programa_de("cruzado_ok", &bueno).expect("el mismo paso en su ejecutor es legítimo");
    }

    /// La otra punta de la referencia: el `assign` que la rellena tiene que
    /// venir del ejecutor que la variable declara.
    #[test]
    fn un_assign_desde_otro_ejecutor_a_una_referencia_se_rechaza() {
        let err = programa_de(
            "assign_cruzado",
            &format!(
                "name: s\n{EJECUTORES_DOS}locals:\n  rack: {{ type: reference, executor: banco }}\n\
                 main:\n  - name: abrir\n    executor: otro\n    assign: {{ rack: result.outputs.rack }}\n"
            ),
        )
        .unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("abrir") && m.contains("banco")),
            "{err}"
        );
    }

    /// **Criterio 3 del encargo, la mitad que decide el cargador.** Nada que
    /// no sea un ejecutor puede acuñar una referencia: ni un `statement`, que
    /// calcula, ni un campo de `result` que no sea una salida.
    ///
    /// Visto fallar quitando cada rechazo por separado: sin el primero el YAML
    /// carga y `locals.rack` acaba valiendo el número 1; sin el segundo, la
    /// medida.
    #[test]
    fn una_referencia_no_se_calcula() {
        let statement = programa_de(
            "stmt",
            &format!(
                "name: s\n{EJECUTORES_DOS}locals:\n  rack: {{ type: reference, executor: banco }}\n\
                 main:\n  - name: init\n    type: statement\n    statement: 'locals.rack = 1'\n"
            ),
        )
        .unwrap_err();
        assert!(
            matches!(&statement, ErrorCarga::Validacion(m) if m.contains("statement") && m.contains("rack")),
            "{statement}"
        );

        let medida = programa_de(
            "medida",
            &format!(
                "name: s\n{EJECUTORES_DOS}locals:\n  rack: {{ type: reference, executor: banco }}\n\
                 main:\n  - name: medir\n    executor: banco\n    assign: {{ rack: result.measured_value }}\n"
            ),
        )
        .unwrap_err();
        assert!(
            matches!(&medida, ErrorCarga::Validacion(m) if m.contains("medir") && m.contains("rack")),
            "{medida}"
        );
    }

    /// Una referencia no tiene forma literal, y por ahí no se cuela ninguna.
    #[test]
    fn una_referencia_escrita_a_mano_en_inputs_se_rechaza() {
        let err = programa_de(
            "literal",
            &format!(
                "name: s\n{EJECUTORES_DOS}main:\n  - name: medir\n    executor: banco\n    inputs: {{ rack: {{ type: reference, executor: banco }} }}\n"
            ),
        )
        .unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("rack") && m.contains("medir")),
            "{err}"
        );
    }

    /// Un componente WASM no tiene dónde guardar el objeto (ADR-0022 §8), y se
    /// dice al cargar en vez de esperar a que el puente lo rechace en marcha.
    #[test]
    fn una_referencia_de_un_ejecutor_wasm_se_rechaza() {
        let dir = std::env::temp_dir().join("anvil_ref_wasm");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.wasm"), b"\0asm\x0d\0\x01\0").unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(
            &y,
            "name: s\nexecutors:\n  - { name: comp, type: wasm, path: ./p.wasm }\n\
             locals:\n  rack: { type: reference, executor: comp }\n\
             main:\n  - name: abrir\n    executor: comp\n    assign: { rack: result.outputs.rack }\n",
        )
        .unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("wasm") && m.contains("rack")),
            "{err}"
        );
    }

    /// Una referencia sólo se declara en `locals:`. En `file_globals:` no se
    /// podría rellenar nunca (el motor rechaza escribirlas), y por
    /// `parameters:` está sin decidir.
    #[test]
    fn una_referencia_fuera_de_locals_se_rechaza() {
        for scope in ["file_globals", "parameters"] {
            let err = cargar_de_texto(&format!(
                "name: s\n{scope}:\n  rack: {{ type: reference, executor: banco }}\nmain:\n  - name: a\n"
            ))
            .unwrap_err();
            assert!(
                matches!(&err, ErrorCarga::Validacion(m) if m.contains(scope) && m.contains("locals")),
                "{scope}: {err}"
            );
        }
    }

    /// Un ejecutor con un dedazo dejaría vacía la comprobación de arriba, y
    /// pasar por vacío es peor que no comprobar.
    #[test]
    fn una_referencia_de_un_ejecutor_inexistente_se_rechaza() {
        let err = programa_de(
            "sin_ejecutor",
            &format!(
                "name: s\n{EJECUTORES_DOS}locals:\n  rack: {{ type: reference, executor: bancoo }}\nmain:\n  - name: a\n"
            ),
        )
        .unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("bancoo")),
            "{err}"
        );
    }

    /// Una declaración mal escrita se diagnostica por su nombre, no como
    /// «los datos no casan con ninguna variante» (#20).
    #[test]
    fn una_declaracion_mal_escrita_se_nombra() {
        let err = cargar_de_texto(
            "name: s\nlocals:\n  rack: { type: referencia, executor: banco }\nmain:\n  - name: a\n",
        )
        .unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("referencia") && m.contains("reference")),
            "{err}"
        );

        let sin_ejecutor =
            cargar_de_texto("name: s\nlocals:\n  rack: { type: reference }\nmain:\n  - name: a\n")
                .unwrap_err();
        assert!(
            matches!(&sin_ejecutor, ErrorCarga::Validacion(m) if m.contains("ejecutor")),
            "{sin_ejecutor}"
        );
    }
}
