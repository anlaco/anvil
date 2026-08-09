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
    Argumento, Asignacion, DefinicionEjecutor, DefinicionPaso, DefinicionSecuencia, Limite,
    Operador, Programa, TipoEjecutor, TipoPaso, ValorDefinicion,
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
    nombre: String,
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
    subsecuencias: HashMap<String, SecuenciaYaml>,
    /// M5-ext.1 (RF-36.3): ejecutores declarados en el YAML. Sin esta
    /// sección, todo paso va al ejecutor embebido (default, compat M4b).
    #[serde(default)]
    ejecutores: Vec<EjecutorYaml>,
}

/// Un ejecutor como se lee del YAML (`ejecutores:`), antes de traducirse a
/// `modelo::DefinicionEjecutor`. `deny_unknown_fields` (fail-fast) igual que
/// el resto del schema. La coherencia entre `tipo` y sus campos se valida en
/// [`EjecutorYaml::a_definicion`].
#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EjecutorYaml {
    nombre: String,
    /// `"embebido"` (default), `"wasm"` o `"grpc"`.
    #[serde(default = "tipo_ejecutor_por_defecto")]
    tipo: String,
    /// Sólo si `tipo == "wasm"`. Path relativo al directorio del YAML.
    #[serde(default)]
    path: Option<String>,
    /// Sólo si `tipo == "grpc"`. Host; puede ser no-loopback **sólo si se
    /// declara** (relajación acotada del loopback de ADR-0011).
    #[serde(default)]
    host: Option<String>,
    /// Sólo si `tipo == "grpc"`. Puerto.
    #[serde(default)]
    puerto: Option<u16>,
}

fn tipo_ejecutor_por_defecto() -> String {
    "embebido".into()
}

/// Un literal de variable declarado en el YAML (scopes de M4). El tipo se
/// infiere del escalar YAML: `true`→bool, `4.5`→número, `"A-2026"`→texto.
/// `untagged` prueba variantes en orden; `Bool` primero evita que `true` se
/// intente como `f64`.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
enum ValorYaml {
    Bool(bool),
    Numero(f64),
    Texto(String),
}

impl ValorYaml {
    fn a_definicion(self) -> ValorDefinicion {
        match self {
            ValorYaml::Bool(b) => ValorDefinicion::Bool(b),
            ValorYaml::Numero(x) => ValorDefinicion::Numero(x),
            ValorYaml::Texto(s) => ValorDefinicion::Texto(s),
        }
    }
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
    nombre: String,
    #[serde(default = "reintentos_por_defecto")]
    reintentos: u32,
    #[serde(default)]
    limite: Option<LimiteYaml>,
    /// RF-34: si `true`, el motor salta el paso sin invocarlo.
    #[serde(default)]
    disable: bool,
    /// RF-34: si `true` y el paso falla, el motor detiene la fase en curso.
    #[serde(default)]
    pause_on_fail: bool,
    /// RF-33: expresión booleana; si es falsa, el paso se salta sin gastar
    /// intento. Texto → AST en `a_definicion`.
    #[serde(default)]
    precondicion: Option<String>,
    /// RF-31: mapa `nombre_local -> expr`; el motor vuelca cada `expr` (sobre
    /// `resultado`/scopes) a la Local. Texto → AST en `a_definicion`.
    #[serde(default)]
    asigna: Option<HashMap<String, String>>,
    /// RF-27: `"grpc"` (default), `"statement"`, `"sequence_call"` o
    /// `"pass_fail"`.
    #[serde(default = "tipo_por_defecto")]
    tipo: String,
    /// RF-27: sentencia(s) a ejecutar si `tipo == "statement"`. Texto → AST.
    #[serde(default)]
    statement: Option<String>,
    /// RF-25 (ADR-0018): expresión booleana del veredicto si
    /// `tipo == "pass_fail"`. Texto → AST en `a_definicion`.
    #[serde(default)]
    condicion: Option<String>,
    /// M4b (RF-27): destino del sequence call si `tipo == "sequence_call"`.
    /// Un **nombre** (subsecuencia inline del mismo archivo) o un **path
    /// relativo** (archivo externo); se distingue con [`es_path`]. Texto.
    #[serde(default)]
    secuencia: Option<String>,
    /// M4b (RF-27): argumentos by-reference del sequence call, mapa
    /// `nombre_parameter -> "locals.X"`. Cada valor se parsea a AST y se
    /// valida como `Expresion::Var { scope: Locals, .. }` (un lvalue local).
    #[serde(default)]
    parametros: Option<HashMap<String, String>>,
    /// M5-ext.1 (RF-36.3): nombre del ejecutor que atiende este paso. Debe
    /// existir en `ejecutores` de la secuencia (fail-fast al cargar). Si se
    /// omite, el paso va al ejecutor embebido (default).
    #[serde(default)]
    ejecutor: Option<String>,
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
    /// `"rango"` o `"comparacion"`.
    tipo: String,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    esperado: Option<f64>,
}

impl LimiteYaml {
    /// Traduce a `modelo::Limite`, validando que los campos cuadren con el
    /// `tipo` declarado. `nombre_paso` solo para mensajes de error.
    fn a_limite(&self, nombre_paso: &str) -> Result<Limite, ErrorCarga> {
        match self.tipo.as_str() {
            "rango" => {
                let Some(min) = self.min else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite rango sin 'min'"
                    )));
                };
                let Some(max) = self.max else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite rango sin 'max'"
                    )));
                };
                if min > max {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite rango con min ({min}) > max ({max})"
                    )));
                }
                if self.op.is_some() || self.esperado.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite rango con campos 'op'/'esperado' (no aplican a un rango)"
                    )));
                }
                Ok(Limite::Rango { min, max })
            }
            "comparacion" => {
                let Some(op_texto) = &self.op else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite comparacion sin 'op'"
                    )));
                };
                let Some(op) = Operador::de_texto(op_texto) else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite comparacion con 'op' inválido '{op_texto}' (eq/ne/lt/le/gt/ge)"
                    )));
                };
                let Some(esperado) = self.esperado else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite comparacion sin 'esperado'"
                    )));
                };
                if self.min.is_some() || self.max.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{nombre_paso}' tiene un límite comparacion con campos 'min'/'max' (no aplican a una comparacion)"
                    )));
                }
                Ok(Limite::Comparacion { op, esperado })
            }
            otro => Err(ErrorCarga::Validacion(format!(
                "el paso '{nombre_paso}' tiene un límite con tipo '{otro}' desconocido (rango|comparacion)"
            ))),
        }
    }
}

/// Nombre de ejecutor reservado del motor (clave interna de la conexión al
/// ejecutor embebido). No declarable en el YAML: el cargador lo rechaza.
pub const NOMBRE_EMBEDIDO_RESERVADO: &str = "__anvil_embebido__";

impl EjecutorYaml {
    /// Traduce a `modelo::DefinicionEjecutor`, validando la coherencia entre
    /// `tipo` y sus campos (fail-fast). `dir_yaml` es el directorio del
    /// archivo que declara el ejecutor: los paths `wasm` se resuelven
    /// relativo a él.
    fn a_definicion(self, dir_yaml: &Path) -> Result<DefinicionEjecutor, ErrorCarga> {
        if self.nombre == NOMBRE_EMBEDIDO_RESERVADO {
            return Err(ErrorCarga::Validacion(format!(
                "el ejecutor '{NOMBRE_EMBEDIDO_RESERVADO}' está reservado; elige otro nombre"
            )));
        }
        let tipo = match self.tipo.as_str() {
            "embebido" => {
                if self.path.is_some() || self.host.is_some() || self.puerto.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'embebido' pero trae 'path'/'host'/'puerto' (no aplican)",
                        self.nombre
                    )));
                }
                TipoEjecutor::Embebido
            }
            "wasm" => {
                if self.host.is_some() || self.puerto.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'wasm' pero trae 'host'/'puerto' (sólo aplican a 'grpc')",
                        self.nombre
                    )));
                }
                let Some(path) = self.path else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'wasm' pero no trae 'path'",
                        self.nombre
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
                        self.nombre, path
                    )));
                }
                // El path debe existir (relativo al directorio del YAML),
                // como las subsecuencias externas (fail-fast al cargar).
                let ruta = normalizar_path(dir_yaml, Path::new(&path));
                if !ruta.exists() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'wasm' y su 'path' '{}' no existe",
                        self.nombre, path
                    )));
                }
                TipoEjecutor::Wasm { path }
            }
            "grpc" => {
                if self.path.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'grpc' pero trae 'path' (sólo aplica a 'wasm')",
                        self.nombre
                    )));
                }
                let (Some(host), Some(puerto)) = (self.host, self.puerto) else {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'grpc' pero no trae 'host' y 'puerto'",
                        self.nombre
                    )));
                };
                TipoEjecutor::Grpc { host, puerto }
            }
            otro => {
                return Err(ErrorCarga::Validacion(format!(
                    "el ejecutor '{}' tiene tipo '{otro}' desconocido (embebido|wasm|grpc)",
                    self.nombre
                )))
            }
        };
        Ok(DefinicionEjecutor {
            nombre: self.nombre,
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
}

impl std::fmt::Display for ErrorCarga {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorCarga::Lectura(e) => write!(f, "no se pudo leer el fichero: {e}"),
            ErrorCarga::Sintaxis(e) => write!(f, "YAML inválido: {e}"),
            ErrorCarga::Validacion(m) => write!(f, "secuencia inválida: {m}"),
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

/// Carga una secuencia desde texto YAML. Es el punto testeable sin tocar
/// el disco; `cargar_de_archivo` lo envuelve. No resuelve sequence calls (ni
/// valida lvalues contra la secuencia padre): para eso, usar
/// [`cargar_programa_de_archivo`].
pub fn cargar_de_texto(texto: &str) -> Result<DefinicionSecuencia, ErrorCarga> {
    let yaml: SecuenciaYaml = noyalib::from_str(texto)?;
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
    if y.nombre.trim().is_empty() {
        match fallback {
            Some(k) => y.nombre = k.to_string(),
            None => {
                return Err(ErrorCarga::Validacion(
                    "el nombre de la secuencia no puede estar vacío".into(),
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
    for (k, sub) in y.subsecuencias {
        subsecuencias.insert(k.clone(), secuencia_yaml_a_definicion(sub, Some(&k))?);
    }

    let def = DefinicionSecuencia {
        nombre: y.nombre,
        pasos_setup: traduce_pasos(y.setup)?,
        pasos_main: traduce_pasos(y.main)?,
        pasos_cleanup: traduce_pasos(y.cleanup)?,
        locals: y
            .locals
            .into_iter()
            .map(|(k, v)| (k, v.a_definicion()))
            .collect(),
        parameters: y
            .parameters
            .into_iter()
            .map(|(k, v)| (k, v.a_definicion()))
            .collect(),
        file_globals: y
            .file_globals
            .into_iter()
            .map(|(k, v)| (k, v.a_definicion()))
            .collect(),
        subsecuencias,
    };
    validar_lvalues(&def)?;
    Ok(def)
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
                let declarado = match scope {
                    expr::Scope::Locals => def.locals.contains_key(campo),
                    expr::Scope::Parameters => def.parameters.contains_key(campo),
                    // FileGlobals/Resultado no son lvalues válidos aquí; el
                    // motor los rechaza al evaluar (error de evaluación, no
                    // silencioso). Nada que validar al cargar.
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
    let yaml: SecuenciaYaml = noyalib::from_str(&texto)?;
    for y in yaml.ejecutores {
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
        let mut sub = cargar_de_texto(&texto)?;
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
    visitar(&programa, &id_raiz, &programa.raiz, &mut camino)?;

    Ok(programa)
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
        let mut sub = cargar_de_texto(&texto)?;
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
    visitar(&programa, &id_pm, &programa.raiz, &mut camino)?;

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
                    "el paso '{}' referencia el ejecutor '{nombre}' que no está en 'ejecutores:'",
                    paso.nombre
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
    let mapa: HashMap<String, LimiteYaml> = noyalib::from_str(&texto)?;
    mapa.into_iter()
        .map(|(nombre, l)| Ok((nombre.clone(), l.a_limite(&nombre)?)))
        .collect()
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
        if p.reintentos == 0 {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' tiene reintentos 0; el mínimo es 1",
                p.nombre
            )));
        }
        if p.nombre.trim().is_empty() {
            return Err(ErrorCarga::Validacion(
                "un paso tiene el nombre vacío".into(),
            ));
        }
    }
    Ok(())
}

impl PasoYaml {
    fn a_definicion(self) -> Result<DefinicionPaso, ErrorCarga> {
        let limite = match self.limite {
            Some(l) => Some(l.a_limite(&self.nombre)?),
            None => None,
        };

        // RF-33: la precondición se parsea a AST aquí (fail-fast). Un error de
        // sintaxis se reporta con el nombre del paso.
        let precondicion = match self.precondicion.as_deref() {
            Some(texto) => Some(expr::parse_expresion(extraer_expr(texto)).map_err(|e| {
                ErrorCarga::Validacion(format!(
                    "precondición del paso '{}' inválida: {e}",
                    self.nombre
                ))
            })?),
            None => None,
        };

        // RF-31: cada `asigna` es `nombre_local -> expr`. La expr se evalúa
        // sobre `resultado`/scopes y el motor la vuelca a Locals.
        let asigna = match self.asigna {
            Some(mapa) => Some(
                mapa.into_iter()
                    .map(|(var, texto)| {
                        let expr = expr::parse_expresion(extraer_expr(&texto)).map_err(|e| {
                            ErrorCarga::Validacion(format!(
                                "asigna '{}' del paso '{}': {e}",
                                var, self.nombre
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
        let tipo = match self.tipo.as_str() {
            "grpc" => TipoPaso::Grpc,
            "statement" => TipoPaso::Statement,
            "sequence_call" => TipoPaso::SequenceCall,
            "pass_fail" => TipoPaso::PassFail,
            otro => {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' tiene tipo '{otro}' inválido \
                     (grpc|statement|sequence_call|pass_fail)",
                    self.nombre
                )))
            }
        };

        // RF-27: el statement se parsea a una lista de sentencias.
        let statement = match self.statement.as_deref() {
            Some(texto) => Some(expr::parse_sentencias(texto).map_err(|e| {
                ErrorCarga::Validacion(format!(
                    "statement del paso '{}' inválido: {e}",
                    self.nombre
                ))
            })?),
            None => None,
        };

        // RF-25 (ADR-0018): la condición del veredicto se parsea a AST aquí
        // (fail-fast), igual que la precondición. Bool estricto al evaluar:
        // que sea booleana no se sabe hasta el runtime (tipos dinámicos), así
        // que un no-Bool es `error` de ejecución, no de carga.
        let condicion = match self.condicion.as_deref() {
            Some(texto) => Some(expr::parse_expresion(extraer_expr(texto)).map_err(|e| {
                ErrorCarga::Validacion(format!(
                    "condición del paso '{}' inválida: {e}",
                    self.nombre
                ))
            })?),
            None => None,
        };

        // M4b (RF-27): argumentos by-reference del sequence call. Cada valor
        // es "locals.X" y se parsea a AST; se valida que sea un lvalue local
        // puro (`Expresion::Var { scope: Locals, .. }`). Que el `campo`
        // exista en `locals` de la secuencia contenedora se valida al
        // resolver el programa (ver `cargar_programa_de_archivo`).
        let parametros = match self.parametros {
            Some(mapa) if !mapa.is_empty() => Some(
                mapa.into_iter()
                    .map(|(param, texto)| {
                        let origen = expr::parse_expresion(extraer_expr(&texto)).map_err(|e| {
                            ErrorCarga::Validacion(format!(
                                "parámetro '{param}' del sequence call '{}': {e}",
                                self.nombre
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
                                    self.nombre
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
                self.nombre
            )));
        }
        if !matches!(tipo, TipoPaso::Statement) && statement.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' pero trae 'statement' (reservado para 'statement')",
                self.nombre, self.tipo
            )));
        }
        // RF-25 (ADR-0018): un `pass_fail` es su condición; sin ella no hay
        // veredicto que dar.
        if matches!(tipo, TipoPaso::PassFail) && condicion.is_none() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'pass_fail' pero no trae 'condicion'",
                self.nombre
            )));
        }
        if !matches!(tipo, TipoPaso::PassFail) && condicion.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' pero trae 'condicion' (reservado para 'pass_fail')",
                self.nombre, self.tipo
            )));
        }
        if matches!(tipo, TipoPaso::SequenceCall) && self.secuencia.is_none() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'sequence_call' pero no trae 'secuencia'",
                self.nombre
            )));
        }
        // Ni un sequence call ni un pass_fail miden: el primero agrega los
        // resultados de sus pasos, el segundo evalúa variables ya pobladas.
        if matches!(tipo, TipoPaso::SequenceCall | TipoPaso::PassFail) && limite.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' y trae 'limite': no mide",
                self.nombre, self.tipo
            )));
        }
        if matches!(tipo, TipoPaso::SequenceCall) && self.reintentos > 1 {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'sequence_call' con reintentos={}: no admite reintentos \
                 (sus pasos internos declaran los suyos)",
                self.nombre, self.reintentos
            )));
        }
        // Un `pass_fail` es puro y determinista (el motor evalúa una
        // expresión, sin red): reintentarlo daría el mismo veredicto. Se
        // rechaza en vez de aceptarlo e ignorarlo en silencio.
        if matches!(tipo, TipoPaso::PassFail) && self.reintentos > 1 {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'pass_fail' con reintentos={}: no admite reintentos \
                 (evalúa una expresión, el resultado no cambia entre intentos)",
                self.nombre, self.reintentos
            )));
        }
        // Un `pass_fail` no produce `resultado.*`, así que su `asigna` no
        // volcaría nada. Rechazarlo en vez de ignorarlo: un `asigna` que no se
        // aplica es la clase de fallo silencioso de DEF-3.
        if matches!(tipo, TipoPaso::PassFail) && asigna.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'pass_fail' y trae 'asigna': un pass_fail no produce \
                 'resultado.*' que volcar (usa un paso 'statement' aparte)",
                self.nombre
            )));
        }
        if !matches!(tipo, TipoPaso::SequenceCall)
            && (self.secuencia.is_some() || parametros.is_some())
        {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' pero trae 'secuencia'/'parametros' (reservado para 'sequence_call')",
                self.nombre, self.tipo
            )));
        }
        // M5-ext.1 (RF-36.3): `ejecutor` sólo aplica a un paso `Grpc` (los
        // `statement`/`sequence_call` son motor-side y no van por gRPC).
        if !matches!(tipo, TipoPaso::Grpc) && self.ejecutor.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es '{}' pero trae 'ejecutor' (reservado para 'grpc')",
                self.nombre, self.tipo
            )));
        }

        Ok(DefinicionPaso {
            nombre: self.nombre,
            reintentos: self.reintentos,
            limite,
            disable: self.disable,
            pause_on_fail: self.pause_on_fail,
            precondicion,
            asigna,
            tipo,
            statement,
            condicion,
            secuencia: self.secuencia,
            parametros,
            ejecutor: self.ejecutor,
        })
    }
}

/// Si `texto` es de la forma `${expr}` (toda la cadena), devuelve `expr`;
/// si no, devuelve `texto` tal cual. Así `asigna` admite las dos formas
/// `x: resultado.valor_medido` y `x: "${resultado.valor_medido}"`. La
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
nombre: basica
setup:
  - nombre: conectar_equipo
    reintentos: 3
main:
  - nombre: medir_voltaje
    reintentos: 1
    limite:
      tipo: rango
      min: 4.5
      max: 5.5
  - nombre: verificar_led
    reintentos: 1
cleanup:
  - nombre: desconectar_equipo
    reintentos: 1
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
nombre: s
main:
  - nombre: un_paso
";
        let s = cargar_de_texto(yaml).unwrap();
        assert_eq!(s.pasos_main[0].reintentos, 1);
    }

    #[test]
    fn setup_y_cleanup_son_opcionales() {
        let yaml = "\
nombre: s
main:
  - nombre: un_paso
";
        let s = cargar_de_texto(yaml).unwrap();
        assert!(s.pasos_setup.is_empty());
        assert!(s.pasos_cleanup.is_empty());
    }

    #[test]
    fn main_ausente_es_error() {
        let yaml = "nombre: s\n";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Sintaxis(_)),
            "main ausente debe ser error de schema, no de validación: {err}"
        );
    }

    #[test]
    fn main_vacio_es_error_de_validacion() {
        let yaml = "nombre: s\nmain: []\n";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("main")));
    }

    #[test]
    fn nombre_vacio_es_error() {
        let yaml = "\
nombre: ''
main:
  - nombre: un_paso
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("nombre")));
    }

    #[test]
    fn reintentos_cero_es_error() {
        let yaml = "\
nombre: s
main:
  - nombre: un_paso
    reintentos: 0
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("reintentos")));
    }

    #[test]
    fn campo_desconocido_es_error() {
        // Desde M4, `disable`/`pause_on_fail`/`precondicion`/`asigna`/`tipo`/
        // `statement` son campos conocidos; desde M4b también `secuencia`/
        // `parametros` (paso) y `subsecuencias` (secuencia). Usamos uno
        // realmente desconocido (`foo`) para seguir probando
        // `deny_unknown_fields` (fail-fast).
        let yaml = "\
nombre: s
main:
  - nombre: un_paso
    foo: bar
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Sintaxis(_)),
            "campo desconocido debe ser error de schema: {err}"
        );
    }

    // --- M4b: sequence call, subsecuencias inline y por path ---

    /// Una subsecuencia inline se carga en `subsecuencias` y un `sequence_call`
    /// por nombre la referencia. El programa resuelve sin archivos externos.
    #[test]
    fn sequence_call_inline_se_carga_y_resuelve() {
        let yaml = "\
nombre: padre
locals: { ok: false }
subsecuencias:
  init:
    parameters: { canal: 0.0, listo: false }
    main:
      - nombre: comprobar
        tipo: statement
        statement: 'parameters.listo = (parameters.canal >= 0.0)'
main:
  - nombre: preparar
    tipo: sequence_call
    secuencia: init
    parametros: { canal: locals.ok, listo: locals.ok }
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
nombre: s
main:
  - nombre: c
    tipo: sequence_call
    secuencia: ./h.yaml
    parametros: { p: file_globals.g }
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("locals.X")));
    }

    #[test]
    fn argumento_expresion_no_es_lvalue_es_error() {
        let yaml = "\
nombre: s
main:
  - nombre: c
    tipo: sequence_call
    secuencia: ./h.yaml
    parametros: { p: 'locals.x + 1' }
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("by-reference")));
    }

    /// Sequence call sin `secuencia` → error; con `limite` → error;
    /// con `reintentos > 1` → error; con `statement` → error.
    #[test]
    fn sequence_call_mal_usado_es_error() {
        let casos = [
            ("nombre: s\nmain:\n  - nombre: c\n    tipo: sequence_call\n", "no trae 'secuencia'"),
            ("nombre: s\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: x\n    limite: { tipo: rango, min: 1, max: 2 }\n", "no mide"),
            ("nombre: s\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: x\n    reintentos: 2\n", "no admite reintentos"),
            ("nombre: s\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: x\n    statement: 'locals.y = 1'\n", "reservado para 'statement'"),
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
nombre: s
locals:
  v: 0.0
main:
  - nombre: verificar_dut
    tipo: pass_fail
    condicion: 'locals.v > 4.9 && locals.v < 5.1'
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
nombre: s
locals:
  v: 0.0
main:
  - nombre: verificar_dut
    tipo: pass_fail
    condicion: '${locals.v > 4.9}'
";
        assert!(cargar_de_texto(yaml).is_ok());
    }

    /// `pass_fail` sin `condicion` → error; `condicion` fuera de un
    /// `pass_fail` → error; con `limite`/`reintentos`/`asigna` → error.
    #[test]
    fn pass_fail_mal_usado_es_error() {
        let casos = [
            (
                "nombre: s\nmain:\n  - nombre: v\n    tipo: pass_fail\n",
                "no trae 'condicion'",
            ),
            (
                "nombre: s\nmain:\n  - nombre: v\n    condicion: 'true'\n",
                "reservado para 'pass_fail'",
            ),
            (
                "nombre: s\nmain:\n  - nombre: v\n    tipo: statement\n    statement: 'locals.x = 1'\n    condicion: 'true'\n",
                "reservado para 'pass_fail'",
            ),
            (
                "nombre: s\nmain:\n  - nombre: v\n    tipo: pass_fail\n    condicion: 'true'\n    limite: { tipo: rango, min: 1, max: 2 }\n",
                "no mide",
            ),
            (
                "nombre: s\nmain:\n  - nombre: v\n    tipo: pass_fail\n    condicion: 'true'\n    reintentos: 2\n",
                "no admite reintentos",
            ),
            (
                "nombre: s\nlocals:\n  x: 0.0\nmain:\n  - nombre: v\n    tipo: pass_fail\n    condicion: 'true'\n    asigna: { x: '1.0' }\n",
                "no produce 'resultado.*'",
            ),
            (
                "nombre: s\nmain:\n  - nombre: v\n    tipo: pass_fail\n    condicion: 'locals.v >'\n",
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
nombre: s
main:
  - nombre: c
    secuencia: ./h.yaml
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
nombre: s
subsecuencias:
  init:
    nombre: init
main:
  - nombre: p
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
nombre: s
subsecuencias:
  init:
    main:
      - nombre: p
main:
  - nombre: m
";
        let s = cargar_de_texto(yaml).unwrap();
        assert_eq!(s.subsecuencias.get("init").unwrap().nombre, "init");
    }

    /// `deny_unknown_fields` también aplica dentro de `subsecuencias`: una
    /// inline con un campo raro falla.
    #[test]
    fn inline_con_campo_desconocido_es_error() {
        let yaml = "\
nombre: s
subsecuencias:
  init:
    nombre: init
    main:
      - nombre: p
    foo: bar
main:
  - nombre: p
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Sintaxis(_)));
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
            "nombre: hija\nparameters: { canal: 0.0 }\nmain:\n  - nombre: m\n    tipo: grpc\n",
        )
        .unwrap();
        let padre = dir.join("padre.yaml");
        std::fs::write(
            &padre,
            "nombre: padre\nlocals: { canal: 1.0 }\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./hija.yaml\n    parametros: { canal: locals.canal }\n",
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
            "nombre: a\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./b.yaml\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("b.yaml"),
            "nombre: b\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./a.yaml\n",
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
            "nombre: h\nparameters: { canal: 0.0, extra: 0.0 }\nmain:\n  - nombre: m\n",
        )
        .unwrap();
        std::fs::write(dir.join("p.yaml"), "nombre: p\nlocals: { canal: 1.0 }\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./h.yaml\n    parametros: { canal: locals.canal }\n").unwrap();
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
            "nombre: h\nparameters: { canal: 0.0 }\nmain:\n  - nombre: m\n",
        )
        .unwrap();
        std::fs::write(dir.join("p.yaml"), "nombre: p\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./h.yaml\n    parametros: { canal: locals.inventado }\n").unwrap();
        let err = cargar_programa_de_archivo(dir.join("p.yaml").to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("locals.inventado")));
    }

    /// Path no encontrado → error de lectura al cargar el programa.
    #[test]
    fn programa_path_no_encontrado_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m4b_{}", "nofile"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.yaml"), "nombre: p\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./no_existe.yaml\n").unwrap();
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
nombre: s
main:
  - nombre: medir_voltaje
    limite:
      tipo: rango
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
nombre: s
main:
  - nombre: verificar_frecuencia
    limite:
      tipo: comparacion
      op: ge
      esperado: 1000.0
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
nombre: s
main:
  - nombre: m
    limite:
      tipo: rango
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
nombre: s
main:
  - nombre: m
    limite:
      tipo: rango
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
nombre: s
main:
  - nombre: m
    limite:
      tipo: comparacion
      op: mayor_que
      esperado: 1000.0
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
nombre: s
main:
  - nombre: m
    limite:
      tipo: rango
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
nombre: s
main:
  - nombre: m
    limite:
      tipo: ventana
      min: 4.5
      max: 5.5
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("tipo")),
            "{err}"
        );
    }

    #[test]
    fn limite_campo_desconocido_dentro_del_limite_es_error() {
        // deny_unknown_fields en LimiteYaml: un campo raro dentro del límite.
        let yaml = "\
nombre: s
main:
  - nombre: m
    limite:
      tipo: rango
      min: 4.5
      max: 5.5
      tolerancia: 0.1
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Sintaxis(_)), "{err}");
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
nombre: s
main:
  - nombre: medir_voltaje
    limite:
      tipo: rango
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
            "nombre: pm\nmain:\n  - nombre: test_uut\n    tipo: sequence_call\n    secuencia: secuencia_usuario\n",
        )
        .unwrap();
        let usuario = dir.join("usuario.yaml");
        std::fs::write(
            &usuario,
            "nombre: usuario\nmain:\n  - nombre: medir_voltaje\n    tipo: grpc\n",
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
            "nombre: hija\nmain:\n  - nombre: medir_voltaje\n    tipo: grpc\n",
        )
        .unwrap();
        let padre = dir.join("padre.yaml");
        std::fs::write(
            &padre,
            "nombre: padre\nsubsecuencias:\n  inline:\n    nombre: inline\n    main:\n      - nombre: medir_voltaje\n        tipo: grpc\nmain:\n  - nombre: medir_voltaje\n    tipo: grpc\n  - nombre: c1\n    tipo: sequence_call\n    secuencia: ./hija.yaml\n  - nombre: c2\n    tipo: sequence_call\n    secuencia: inline\n",
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
            cargar_de_texto("nombre: hija\nmain:\n  - nombre: solo_en_la_hija\n    tipo: grpc\n")
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

    #[test]
    fn cargar_limites_de_texto_valida_entradas() {
        // Versión sin disco de cargar_limites_de_archivo para testear directo.
        let texto = "\
medir_voltaje:
  tipo: rango
  min: 4.5
  max: 5.5
verificar_frecuencia:
  tipo: comparacion
  op: ge
  esperado: 1000.0
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
nombre: s
file_globals:
  lote: \"A-2026-08\"
  umbral: 4.5
locals:
  voltaje: 0.0
  ok: false
parameters: {}
main:
  - nombre: un_paso
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
nombre: s
main:
  - nombre: un_paso
    disable: true
    pause_on_fail: true
";
        let s = cargar_de_texto(yaml).unwrap();
        assert!(s.pasos_main[0].disable);
        assert!(s.pasos_main[0].pause_on_fail);
        // Sin los campos: defaults false (compat con M3).
        let s2 = cargar_de_texto("nombre: s\nmain:\n  - nombre: otro\n").unwrap();
        assert!(!s2.pasos_main[0].disable);
        assert!(!s2.pasos_main[0].pause_on_fail);
    }

    #[test]
    fn precondicion_se_parsea_a_ast() {
        let yaml = "\
nombre: s
main:
  - nombre: medir
    precondicion: 'locals.contador > 0 && resultado.valor_medido != nothing'
";
        let s = cargar_de_texto(yaml).unwrap();
        assert!(
            s.pasos_main[0].precondicion.is_some(),
            "la precondición debe parsearse a AST"
        );
    }

    #[test]
    fn precondicion_mal_formada_es_error_de_validacion_con_nombre() {
        let yaml = "\
nombre: s
main:
  - nombre: medir
    precondicion: 'locals.contador >'
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
nombre: s
locals:
  voltaje: 0.0
  ok: false
main:
  - nombre: medir
    asigna:
      voltaje: resultado.valor_medido
      ok: '${resultado.estado == \"paso\"}'
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
nombre: s
main:
  - nombre: medir
    asigna:
      x: 'resultado.valor_medido +'
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
nombre: s
main:
  - nombre: init
    tipo: statement
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
nombre: s
main:
  - nombre: init
    tipo: grpc
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
nombre: s
locals:
  ok: false
  contador: 0
main:
  - nombre: init
    tipo: statement
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
nombre: s
parameters:
  p: 0.0
main:
  - nombre: medir_voltaje
    asigna: { p: '${resultado.valor_medido}' }
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
nombre: s
locals:
  voltaje: 0.0
main:
  - nombre: medir_voltaje
    asigna: { voltage: '${resultado.valor_medido}' }
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
nombre: s
main:
  - nombre: init
    tipo: statement
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
nombre: s
main:
  - nombre: init
    tipo: statement
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
nombre: s
parameters:
  canal: 0.0
main:
  - nombre: ajustar_canal
    tipo: statement
    statement: 'parameters.canal = parameters.canal + 1.0'
";
        assert!(cargar_de_texto(yaml).is_ok());
    }

    #[test]
    fn asigna_sobre_parameter_declarado_en_subsecuencia_inline_es_error() {
        // La validación baja también a las inline, con sus propios scopes.
        let yaml = "\
nombre: s
subsecuencias:
  init:
    parameters:
      p: 0.0
    main:
      - nombre: medir
        asigna: { p: '${resultado.valor_medido}' }
main:
  - nombre: c
    tipo: sequence_call
    secuencia: init
    parametros: {}
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
nombre: s
main:
  - nombre: init
    tipo: magia
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("magia")),
            "{err}"
        );
    }

    #[test]
    fn tipo_omitido_es_grpc_por_defecto() {
        let s = cargar_de_texto("nombre: s\nmain:\n  - nombre: un_paso\n").unwrap();
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
nombre: demo
ejecutores:
  - { nombre: embebido, tipo: embebido }
  - { nombre: python, tipo: grpc, host: 127.0.0.1, puerto: 9101 }
main:
  - nombre: a
  - nombre: b
    ejecutor: python
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
        std::fs::write(&y, "nombre: s\nmain:\n  - nombre: a\n").unwrap();
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
        std::fs::write(
            &y,
            "nombre: s\nmain:\n  - nombre: a\n    ejecutor: inventado\n",
        )
        .unwrap();
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
                "nombre: s\nejecutores:\n  - {{ nombre: p, tipo: wasm, path: {} }}\nmain:\n  - nombre: a\n",
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
            "nombre: s\nejecutores:\n  - { nombre: p, tipo: wasm }\nmain:\n  - nombre: a\n",
        )
        .unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("'path'")),
            "{err}"
        );

        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: p, tipo: wasm, path: ./no_existe.wasm }\nmain:\n  - nombre: a\n").unwrap();
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
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: p, tipo: wasm, path: ./p.wasm }\nmain:\n  - nombre: a\n    ejecutor: p\n").unwrap();
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
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: p, tipo: grpc, host: 127.0.0.1 }\nmain:\n  - nombre: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("'host' y 'puerto'")),
            "{err}"
        );

        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: p, tipo: grpc, host: 127.0.0.1, puerto: 9101, path: ./p.wasm }\nmain:\n  - nombre: a\n").unwrap();
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
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: e, tipo: embebido, path: ./p.wasm }\nmain:\n  - nombre: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("no aplican")),
            "{err}"
        );

        std::fs::write(
            &y,
            "nombre: s\nejecutores:\n  - { nombre: e, tipo: raro }\nmain:\n  - nombre: a\n",
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
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: a, tipo: embebido }\n  - { nombre: a, tipo: embebido }\nmain:\n  - nombre: p\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("más de una vez")),
            "{err}"
        );

        std::fs::write(&y, format!("nombre: s\nejecutores:\n  - {{ nombre: {NOMBRE_EMBEDIDO_RESERVADO}, tipo: embebido }}\nmain:\n  - nombre: p\n")).unwrap();
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
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: e, tipo: embebido }\nmain:\n  - nombre: a\n    tipo: statement\n    statement: 'locals.x = 1'\n    ejecutor: e\n").unwrap();
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
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: e, tipo: embebido, foo: bar }\nmain:\n  - nombre: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Sintaxis(_)), "{err}");
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

    /// Override `--ejecutor nombre=host:puerto`: re-apunta un grpc, convierte
    /// un embebido, y falla si el nombre no está declarado.
    #[test]
    fn override_de_ejecutores() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "override"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: e, tipo: embebido }\n  - { nombre: py, tipo: grpc, host: 127.0.0.1, puerto: 9101 }\nmain:\n  - nombre: a\n").unwrap();
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
nombre: sequential
locals: { uut_id: \"\", estado_usuario: \"\" }
setup:
  - nombre: identificar_uut
    reintentos: 1
    asigna: { uut_id: \"${resultado.mensaje}\" }
main:
  - nombre: correr_secuencia_usuario
    tipo: sequence_call
    secuencia: secuencia_usuario
    asigna: { estado_usuario: \"${resultado.estado}\" }
cleanup:
  - nombre: notificar_resultado
    reintentos: 1
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
        std::fs::write(dir.join("pm.yaml"), "nombre: pm\nmain:\n  - nombre: x\n").unwrap();
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
            "nombre: pm\nmain:\n  - nombre: a\n    tipo: sequence_call\n    secuencia: secuencia_usuario\n  - nombre: b\n    tipo: sequence_call\n    secuencia: secuencia_usuario\n",
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
            "nombre: pm\nsubsecuencias:\n  secuencia_usuario:\n    nombre: secuencia_usuario\n    main:\n      - nombre: x\nmain:\n  - nombre: a\n    tipo: sequence_call\n    secuencia: secuencia_usuario\n",
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
            "nombre: u\nparameters: { p: 0.0 }\nmain:\n  - nombre: m\n",
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
            "nombre: u\nmain:\n  - nombre: vuelta\n    tipo: sequence_call\n    secuencia: ./pm.yaml\n",
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
}
