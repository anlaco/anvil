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

use modelo::{Argumento, Asignacion, DefinicionEjecutor, DefinicionPaso, DefinicionSecuencia, Limite, Operador, Programa, TipoEjecutor, TipoPaso, ValorDefinicion};
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
/// `asigna`, `statement`) vienen como texto y se parsean a AST en
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
    /// RF-27: `"grpc"` (default) o `"statement"`.
    #[serde(default = "tipo_por_defecto")]
    tipo: String,
    /// RF-27: sentencia(s) a ejecutar si `tipo == "statement"`. Texto → AST.
    #[serde(default)]
    statement: Option<String>,
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
    /// M5 (RF-38): si `true`, el sequence call invoca a la **secuencia
    /// usuario** (la que el operador pasa por CLI), inyectada por el
    /// cargador bajo `CLAVE_SECUENCIA_USUARIO` cuando se corre con
    /// `--process-model` (ADR-0016). Implica `tipo: sequence_call`; no
    /// admite `secuencia` ni `parametros` (MVP-parcial: la frontera
    /// PM↔usuario se comunica por `asigna`/`locals`). Sin `--process-model`,
    /// cargar un YAML con este flag es error (fail-fast).
    #[serde(default)]
    secuencia_usuario: bool,
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
                        "el ejecutor '{}' es 'wasm' pero no trae 'path'", self.nombre
                    )));
                };
                // El path debe existir (relativo al directorio del YAML),
                // como las subsecuencias externas (fail-fast al cargar). Se
                // guarda **normalizado a clave canónica** (M5/RF-38): el host
                // lo usa tal cual para instanciar el puente, sin re-resolver
                // contra el directorio de la secuencia usuario (que con un
                // process model es otro directorio).
                let ruta = normalizar_path(dir_yaml, Path::new(&path));
                if !ruta.exists() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el ejecutor '{}' es 'wasm' y su 'path' '{}' no existe",
                        self.nombre, path
                    )));
                }
                TipoEjecutor::Wasm { path: ruta.to_string_lossy().into_owned() }
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
        Ok(DefinicionEjecutor { nombre: self.nombre, tipo })
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
pub fn aplicar_override_ejecutores(programa: &mut Programa, overrides: &[String]) -> Result<usize, ErrorCarga> {
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
        ejecutor.tipo = TipoEjecutor::Grpc { host: host.to_string(), puerto };
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

    Ok(DefinicionSecuencia {
        nombre: y.nombre,
        pasos_setup: traduce_pasos(y.setup)?,
        pasos_main: traduce_pasos(y.main)?,
        pasos_cleanup: traduce_pasos(y.cleanup)?,
        locals: y.locals.into_iter().map(|(k, v)| (k, v.a_definicion())).collect(),
        parameters: y.parameters.into_iter().map(|(k, v)| (k, v.a_definicion())).collect(),
        file_globals: y.file_globals.into_iter().map(|(k, v)| (k, v.a_definicion())).collect(),
        subsecuencias,
    })
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
    Path::new(ruta).parent().unwrap_or_else(|| Path::new("")).to_path_buf()
}

/// Normaliza un path relativo a `base` resolviendo `.` y `..` de forma
/// lógica (sin IO, sin resolver symlinks): la clave canónica estable para
/// `programa.archivos` y para detectar ciclos.
pub fn normalizar_path(base: &Path, rel: &Path) -> PathBuf {
    let mut out = if rel.is_absolute() { PathBuf::new() } else { base.to_path_buf() };
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

/// Carga un **programa** desde un fichero YAML en disco (M4b, RF-27): la
/// secuencia raíz más todas las subsecuencias de **archivos externos**
/// referenciadas por path, ya resueltas y validadas.
///
/// Hace tres cosas (fail-fast, antes de ejecutar nada):
/// 1. **Carga** la raíz y, recursivamente, los archivos externos a los que
///    apuntan los `sequence_call` por path. Los paths se **reescriben** a su
///    clave canónica ([`normalizar_path`]) en cada `DefinicionPaso.secuencia`, así
///    el motor los resuelve con un mero `programa.archivos[clave]` (sin
///    conocer el sistema de ficheros, ADR-0005).
/// 2. **Valida** cada `sequence_call`: que el destino exista (inline por
///    nombre o archivo por path), que cada argumento `locals.X` esté
///    declarado en `locals` de la secuencia contenedora, y que la **firma**
///    encaje (claves de `parametros` == `parameters` de la subsecuencia).
/// 3. **Detecta ciclos** en el grafo de llamadas (por nombre inline o por
///    path): `A → B → A` es error.
pub fn cargar_programa_de_archivo(ruta: &str) -> Result<Programa, ErrorCarga> {
    let programa = cargar_programa_resuelto(ruta)?;
    validar_programa(&programa, ruta)?;
    Ok(programa)
}

/// Carga un **programa con process model** (M5, RF-38, ADR-0016): el
/// process model (`ruta_pm`) es la raíz y la **secuencia usuario**
/// (`ruta_usuario`, la que el operador pasa por CLI) se inyecta como
/// subsecuencia externa bajo [`modelo::CLAVE_SECUENCIA_USUARIO`]. El PM la
/// invoca con un paso `secuencia_usuario: true`; el motor la resuelve como
/// cualquier subsecuencia externa (sin cambios de semántica).
///
/// Fail-fast:
/// - El PM debe invocar a la secuencia usuario (algún paso con
///   `secuencia_usuario: true`); si no, error.
/// - La secuencia usuario no puede usar `secuencia_usuario` (sólo el PM).
/// - Colisiones de claves de subsecuencias externas o de nombres de
///   ejecutores entre PM y usuario → error.
/// - Ciclos: si la secuencia usuario referencia al PM (o a sí misma), el
///   grafo completo se revalida y se detecta.
pub fn cargar_programa_con_process_model(
    ruta_pm: &str,
    ruta_usuario: &str,
) -> Result<Programa, ErrorCarga> {
    let mut programa = cargar_programa_resuelto(ruta_pm)?;
    let usuario = cargar_programa_de_archivo(ruta_usuario)?;

    if !tiene_secuencia_usuario(&programa) {
        return Err(ErrorCarga::Validacion(format!(
            "el process model '{ruta_pm}' no invoca a la secuencia usuario \
             (ningún paso con 'secuencia_usuario: true')"
        )));
    }
    if programa.archivos.contains_key(modelo::CLAVE_SECUENCIA_USUARIO) {
        return Err(ErrorCarga::Validacion(format!(
            "el process model '{ruta_pm}' ya declara una subsecuencia externa con la \
             clave reservada '{}'",
            modelo::CLAVE_SECUENCIA_USUARIO
        )));
    }
    programa.archivos.insert(modelo::CLAVE_SECUENCIA_USUARIO.to_string(), usuario.raiz);
    for (clave, sub) in usuario.archivos {
        if programa.archivos.contains_key(&clave) {
            return Err(ErrorCarga::Validacion(format!(
                "colisión de subsecuencia externa '{clave}' entre el process model \
                 y la secuencia usuario"
            )));
        }
        programa.archivos.insert(clave, sub);
    }
    for (nombre, def) in usuario.ejecutores {
        if programa.ejecutores.contains_key(&nombre) {
            return Err(ErrorCarga::Validacion(format!(
                "colisión de ejecutor '{nombre}' entre el process model y la secuencia usuario"
            )));
        }
        programa.ejecutores.insert(nombre, def);
    }

    validar_programa(&programa, ruta_pm)?;
    Ok(programa)
}

/// ¿Algún paso del programa (raíz o subsecuencias externas) invoca a la
/// secuencia usuario (`secuencia_usuario: true`)?
fn tiene_secuencia_usuario(programa: &Programa) -> bool {
    let mut encontrado = false;
    let mut revisa = |def: &DefinicionSecuencia| {
        for paso in def.pasos_setup.iter().chain(&def.pasos_main).chain(&def.pasos_cleanup) {
            if paso.secuencia_usuario {
                encontrado = true;
            }
        }
    };
    revisa(&programa.raiz);
    for sub in programa.archivos.values() {
        revisa(sub);
    }
    encontrado
}

/// Carga la raíz y resuelve los archivos externos (sin validar el grafo).
/// Lo usa [`cargar_programa_de_archivo`] y [`cargar_programa_con_process_model`]
/// (que valida tras inyectar la secuencia usuario).
fn cargar_programa_resuelto(ruta: &str) -> Result<Programa, ErrorCarga> {
    let raiz = cargar_de_archivo(ruta)?;
    let dir_base = dir_de(ruta);

    // M5-ext.1 (RF-36.3): la tabla de ejecutores declarada en `ejecutores:`
    // del YAML de la **raíz**. Se re-lee el fichero para esa sección (la
    // `DefinicionSecuencia` no la lleva: es dato del `Programa`). Nombres
    // duplicados → error (fail-fast).
    let texto = std::fs::read_to_string(ruta)?;
    let yaml_raiz: SecuenciaYaml = noyalib::from_str(&texto)?;
    let mut ejecutores = HashMap::new();
    for y in yaml_raiz.ejecutores {
        let def = y.a_definicion(&dir_base)?;
        if ejecutores.contains_key(&def.nombre) {
            return Err(ErrorCarga::Validacion(format!(
                "el ejecutor '{}' está declarado más de una vez en 'ejecutores:'",
                def.nombre
            )));
        }
        ejecutores.insert(def.nombre.clone(), def);
    }

    let mut programa = Programa { raiz, archivos: HashMap::new(), ejecutores };

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

    Ok(programa)
}

/// Valida el grafo de llamadas del programa (lvalues, firmas, ciclos) y
/// que los `secuencia_usuario` estén resueltos. `ruta` sólo para mensajes.
fn validar_programa(programa: &Programa, ruta: &str) -> Result<(), ErrorCarga> {
    let dir_base = dir_de(ruta);
    let id_raiz = normalizar_path(&dir_base, Path::new(ruta)).to_string_lossy().into_owned();
    let mut camino: Vec<String> = Vec::new();
    visitar(programa, &id_raiz, &programa.raiz, &mut camino)
}

/// Recorre una `DefinicionSecuencia` (y sus `subsecuencias` inline) y, por
/// cada `sequence_call` por path, reescribe su `secuencia` a la clave
/// canónica y encola el archivo para cargarlo. Las inline (por nombre) se
/// dejan tal cual: el motor las resuelve en `def.subsecuencias`.
///
/// M5 (RF-38): un paso con `secuencia_usuario: true` se reescribe a
/// `secuencia: Some(CLAVE_SECUENCIA_USUARIO)` (sin encolar nada: la
/// secuencia usuario la inyecta `cargar_programa_con_process_model`). Así
/// el motor la resuelve como cualquier subsecuencia externa por path, sin
/// aprender un caso nuevo.
fn procesar_secuencia(
    def: &mut DefinicionSecuencia,
    dir: &Path,
    cola: &mut Vec<(String, PathBuf)>,
) -> Result<(), ErrorCarga> {
    for paso in def.pasos_setup.iter_mut().chain(&mut def.pasos_main).chain(&mut def.pasos_cleanup) {
        if paso.tipo == TipoPaso::SequenceCall {
            if paso.secuencia_usuario {
                paso.secuencia = Some(modelo::CLAVE_SECUENCIA_USUARIO.to_string());
                continue;
            }
            if let Some(sec) = paso.secuencia.as_ref() {
                if es_path(sec) {
                    let path_dest = normalizar_path(dir, Path::new(sec));
                    let clave = path_dest.to_string_lossy().into_owned();
                    cola.push((
                        clave.clone(),
                        path_dest.parent().unwrap_or_else(|| Path::new("")).to_path_buf(),
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
        return Err(ErrorCarga::Validacion(format!("ciclo de subsecuencias: {trail}")));
    }
    camino.push(id.to_string());
    for paso in def.pasos_setup.iter().chain(&def.pasos_main).chain(&def.pasos_cleanup) {
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
        // M5 (RF-38): un paso `secuencia_usuario` apunta a la secuencia
        // usuario, inyectada bajo `CLAVE_SECUENCIA_USUARIO` por
        // `cargar_programa_con_process_model`. Sin `--process-model`, la
        // clave no existe → error claro (fail-fast).
        if paso.secuencia_usuario {
            let sub = programa.archivos.get(modelo::CLAVE_SECUENCIA_USUARIO).ok_or_else(|| {
                ErrorCarga::Validacion(format!(
                    "el paso '{}' usa 'secuencia_usuario: true' pero no se corre con \
                     --process-model (no hay secuencia usuario que invocar)",
                    paso.nombre
                ))
            })?;
            validar_call(paso, def, sub, modelo::CLAVE_SECUENCIA_USUARIO)?;
            visitar(programa, modelo::CLAVE_SECUENCIA_USUARIO, sub, camino)?;
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
    let args: Vec<&Argumento> = paso.parametros.as_ref().map(|v| v.iter().collect()).unwrap_or_default();
    // Lvalues: la forma `Var{Locals, campo}` ya se validó en `a_definicion`;
    // aquí validamos que `campo` esté declarado en `locals` del padre.
    for a in &args {
        if let expr::Expresion::Var { scope: expr::Scope::Locals, campo } = &a.origen {
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
            Some(texto) => Some(
                expr::parse_expresion(extraer_expr(texto))
                    .map_err(|e| ErrorCarga::Validacion(format!(
                        "precondición del paso '{}' inválida: {e}", self.nombre
                    )))?,
            ),
            None => None,
        };

        // RF-31: cada `asigna` es `nombre_local -> expr`. La expr se evalúa
        // sobre `resultado`/scopes y el motor la vuelca a Locals.
        let asigna = match self.asigna {
            Some(mapa) => Some(
                mapa.into_iter()
                    .map(|(var, texto)| {
                        let expr = expr::parse_expresion(extraer_expr(&texto))
                            .map_err(|e| ErrorCarga::Validacion(format!(
                                "asigna '{}' del paso '{}': {e}", var, self.nombre
                            )))?;
                        Ok(Asignacion { var, expr })
                    })
                    .collect::<Result<Vec<_>, ErrorCarga>>()?,
            ),
            None => None,
        };

        // RF-27: tipo de paso. `grpc` (default), `statement` o `sequence_call` (M4b).
        // M5 (RF-38): `secuencia_usuario: true` implica `sequence_call` (el
        // operador no tiene que escribirlo); si declara otro tipo, error.
        let tipo = match self.tipo.as_str() {
            "grpc" if self.secuencia_usuario => TipoPaso::SequenceCall,
            "grpc" => TipoPaso::Grpc,
            "statement" => TipoPaso::Statement,
            "sequence_call" => TipoPaso::SequenceCall,
            otro => {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' tiene tipo '{otro}' inválido (grpc|statement|sequence_call)",
                    self.nombre
                )))
            }
        };

        // RF-27: el statement se parsea a una lista de sentencias.
        let statement = match self.statement.as_deref() {
            Some(texto) => Some(
                expr::parse_sentencias(texto).map_err(|e| ErrorCarga::Validacion(format!(
                    "statement del paso '{}' inválido: {e}", self.nombre
                )))?,
            ),
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
                        let origen = expr::parse_expresion(extraer_expr(&texto))
                            .map_err(|e| ErrorCarga::Validacion(format!(
                                "parámetro '{param}' del sequence call '{}': {e}", self.nombre
                            )))?;
                        match &origen {
                            expr::Expresion::Var { scope: expr::Scope::Locals, .. } => {}
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
                "el paso '{}' es 'statement' pero no trae 'statement'", self.nombre
            )));
        }
        if matches!(tipo, TipoPaso::Grpc) && statement.is_some() {
            return Err(ErrorCarga::Validacion(format!(
                "el paso '{}' es 'grpc' pero trae 'statement' (reservado para 'statement')",
                self.nombre
            )));
        }
        if matches!(tipo, TipoPaso::SequenceCall) {
            if self.secuencia_usuario {
                // M5 (RF-38): la secuencia usuario se inyecta; no admite
                // destino propio ni argumentos (MVP-parcial: la frontera
                // PM↔usuario se comunica por `asigna`/`locals`).
                if self.secuencia.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{}' usa 'secuencia_usuario: true' y trae 'secuencia' \
                         (la secuencia usuario se inyecta; no se declara destino)",
                        self.nombre
                    )));
                }
                if parametros.is_some() {
                    return Err(ErrorCarga::Validacion(format!(
                        "el paso '{}' usa 'secuencia_usuario: true' y trae 'parametros' \
                         (MVP-parcial: la frontera PM↔usuario se comunica por 'asigna'/'locals')",
                        self.nombre
                    )));
                }
            } else if self.secuencia.is_none() {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' es 'sequence_call' pero no trae 'secuencia'", self.nombre
                )));
            }
            if statement.is_some() {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' es 'sequence_call' pero trae 'statement' (reservado para 'statement')",
                    self.nombre
                )));
            }
            if limite.is_some() {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' es 'sequence_call' y trae 'limite': un sequence call no mide",
                    self.nombre
                )));
            }
            if self.reintentos > 1 {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' es 'sequence_call' con reintentos={}: no admite reintentos \
                     (sus pasos internos declaran los suyos)",
                    self.nombre, self.reintentos
                )));
            }
        }
        if matches!(tipo, TipoPaso::Grpc | TipoPaso::Statement) && (self.secuencia.is_some() || parametros.is_some()) {
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
            secuencia: self.secuencia,
            secuencia_usuario: self.secuencia_usuario,
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
                DefinicionPaso::con_limite("medir_voltaje", 1, Limite::Rango { min: 4.5, max: 5.5 }),
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
        assert!(matches!(&err, ErrorCarga::Sintaxis(_)), "main ausente debe ser error de schema, no de validación: {err}");
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
        assert!(matches!(&err, ErrorCarga::Sintaxis(_)), "campo desconocido debe ser error de schema: {err}");
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
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("reservado para 'sequence_call'")));
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
        assert!(matches!(&err, ErrorCarga::Sintaxis(_)), "inline sin main: error de schema: {err}");
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
        std::fs::write(&hija, "nombre: hija\nparameters: { canal: 0.0 }\nmain:\n  - nombre: m\n    tipo: grpc\n").unwrap();
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
        assert_eq!(prog.archivos.get(clave).map(|d| d.nombre.as_str()).unwrap_or(""), "hija");
    }

    /// Ciclo por path (A → B → A) se detecta al cargar el programa.
    #[test]
    fn programa_detecta_ciclo_por_path() {
        let dir = std::env::temp_dir().join(format!("anvil_m4b_{}", "ciclo"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.yaml"), "nombre: a\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./b.yaml\n").unwrap();
        std::fs::write(dir.join("b.yaml"), "nombre: b\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./a.yaml\n").unwrap();
        let err = cargar_programa_de_archivo(dir.join("a.yaml").to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("ciclo")));
    }

    /// Firma que no encaja (falta un parámetro) → error al cargar el programa.
    #[test]
    fn programa_firma_no_encaja_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m4b_{}", "firma"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&dir.join("h.yaml"), "nombre: h\nparameters: { canal: 0.0, extra: 0.0 }\nmain:\n  - nombre: m\n").unwrap();
        std::fs::write(&dir.join("p.yaml"), "nombre: p\nlocals: { canal: 1.0 }\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./h.yaml\n    parametros: { canal: locals.canal }\n").unwrap();
        let err = cargar_programa_de_archivo(dir.join("p.yaml").to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("firma")));
    }

    /// Argumento `locals.X` no declarado en el padre → error al cargar.
    #[test]
    fn programa_lvalue_no_declarado_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m4b_{}", "lvalue"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&dir.join("h.yaml"), "nombre: h\nparameters: { canal: 0.0 }\nmain:\n  - nombre: m\n").unwrap();
        std::fs::write(&dir.join("p.yaml"), "nombre: p\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./h.yaml\n    parametros: { canal: locals.inventado }\n").unwrap();
        let err = cargar_programa_de_archivo(dir.join("p.yaml").to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("locals.inventado")));
    }

    /// Path no encontrado → error de lectura al cargar el programa.
    #[test]
    fn programa_path_no_encontrado_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m4b_{}", "nofile"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&dir.join("p.yaml"), "nombre: p\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./no_existe.yaml\n").unwrap();
        let err = cargar_programa_de_archivo(dir.join("p.yaml").to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Lectura(_)));
    }

    /// El ejemplo `ejemplos/subsecuencia.yaml` carga como programa: la
    /// subsecuencia externa `./medir_fuentes.yaml` se resuelve, la inline
    /// `init_comun` se enlaza por nombre y la firma/lvalues validan.
    #[test]
    fn ejemplo_subsecuencia_carga_como_programa() {
        let ruta = format!("{}/../../ejemplos/subsecuencia.yaml", env!("CARGO_MANIFEST_DIR"));
        let prog = cargar_programa_de_archivo(&ruta)
            .unwrap_or_else(|e| panic!("no carga el programa {ruta}: {e}"));
        assert_eq!(prog.raiz.nombre, "basica");
        assert_eq!(prog.raiz.subsecuencias.len(), 1, "una inline: init_comun");
        assert_eq!(prog.archivos.len(), 1, "una externa: medir_fuentes.yaml");
        // El call externo reescribe su `secuencia` a la clave canónica (path).
        let call_ext = &prog.raiz.pasos_main[1];
        assert_eq!(call_ext.tipo, modelo::TipoPaso::SequenceCall);
        assert!(es_path(call_ext.secuencia.as_deref().unwrap()), "path reescrito");
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
        assert_eq!(s.pasos_main[0].limite, Some(Limite::Rango { min: 4.5, max: 5.5 }));
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
            Some(Limite::Comparacion { op: Operador::Ge, esperado: 1000.0 })
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
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("min")), "{err}");
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
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains(">")), "{err}");
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
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("op")), "{err}");
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
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("op")), "{err}");
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
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("tipo")), "{err}");
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
        lim.insert("medir_voltaje".to_string(), Limite::Rango { min: 4.5, max: 5.5 });
        let n = aplicar_limites(&mut s, &lim);
        assert_eq!(n, 1, "solo medir_voltaje recibió límite");
        assert_eq!(s.pasos_main[0].limite, Some(Limite::Rango { min: 4.5, max: 5.5 }));
        // Los demás pasos siguen sin límite.
        assert_eq!(s.pasos_main[1].limite, None, "verificar_led no estaba en el sidecar");
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
        lim.insert("medir_voltaje".to_string(), Limite::Rango { min: 4.0, max: 6.0 });
        aplicar_limites(&mut s, &lim);
        assert_eq!(s.pasos_main[0].limite, Some(Limite::Rango { min: 4.0, max: 6.0 }), "el sidecar overridea el embebido");
    }

    #[test]
    fn property_loader_ignora_nombres_que_no_estan_en_la_secuencia() {
        let mut s = cargar_de_texto(basica_yaml()).unwrap();
        let mut lim = HashMap::new();
        lim.insert("paso_que_no_existe".to_string(), Limite::Rango { min: 0.0, max: 1.0 });
        assert_eq!(aplicar_limites(&mut s, &lim), 0, "ningún paso coincide");
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
        assert_eq!(lim.get("medir_voltaje"), Some(&Limite::Rango { min: 4.5, max: 5.5 }));
        assert_eq!(
            lim.get("verificar_frecuencia"),
            Some(&Limite::Comparacion { op: Operador::Ge, esperado: 1000.0 })
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
        assert_eq!(s.file_globals.get("lote"), Some(&ValorDefinicion::Texto("A-2026-08".into())));
        assert_eq!(s.file_globals.get("umbral"), Some(&ValorDefinicion::Numero(4.5)));
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
        assert!(s.pasos_main[0].precondicion.is_some(), "la precondición debe parsearse a AST");
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
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("medir") && m.contains("precondición")),
            "el error debe mencionar el paso y la sección: {err}");
    }

    #[test]
    fn asigna_se_parsea_y_acepta_las_dos_formas() {
        let yaml = "\
nombre: s
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
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("'x'") && m.contains("medir")),
            "el error debe mencionar la var y el paso: {err}");
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
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("statement")), "{err}");
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
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("statement")), "{err}");
    }

    #[test]
    fn statement_se_parsea_a_sentencias() {
        let yaml = "\
nombre: s
main:
  - nombre: init
    tipo: statement
    statement: 'locals.ok = false; locals.contador = 0'
";
        let s = cargar_de_texto(yaml).unwrap();
        let stmts = s.pasos_main[0].statement.as_ref().unwrap();
        assert_eq!(stmts.len(), 2, "dos sentencias separadas por ';'");
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
        assert!(matches!(&err, ErrorCarga::Validacion(ref m) if m.contains("magia")), "{err}");
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
        let ruta = format!("{}/../../ejemplos/variables.yaml", env!("CARGO_MANIFEST_DIR"));
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
        std::fs::write(&y, "\
nombre: demo
ejecutores:
  - { nombre: embebido, tipo: embebido }
  - { nombre: python, tipo: grpc, host: 127.0.0.1, puerto: 9101 }
main:
  - nombre: a
  - nombre: b
    ejecutor: python
").unwrap();
        let prog = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap();
        assert_eq!(prog.ejecutores.len(), 2);
        assert_eq!(prog.ejecutores["embebido"].tipo, TipoEjecutor::Embebido);
        assert_eq!(
            prog.ejecutores["python"].tipo,
            TipoEjecutor::Grpc { host: "127.0.0.1".into(), puerto: 9101 }
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
        std::fs::write(&y, "nombre: s\nmain:\n  - nombre: a\n    ejecutor: inventado\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("inventado")), "{err}");
    }

    /// `tipo: wasm` sin `path` → error; con `path` inexistente → error.
    #[test]
    fn wasm_sin_path_o_inexistente_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "wasm"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: p, tipo: wasm }\nmain:\n  - nombre: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("'path'")), "{err}");

        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: p, tipo: wasm, path: ./no_existe.wasm }\nmain:\n  - nombre: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("no existe")), "{err}");
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
        let clave = dir.join("p.wasm").to_string_lossy().into_owned();
        assert_eq!(prog.ejecutores["p"].tipo, TipoEjecutor::Wasm { path: clave });
    }

    /// `grpc` sin `host`/`puerto` → error; `grpc` con `path` → error.
    #[test]
    fn grpc_incompleto_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "grpc"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: p, tipo: grpc, host: 127.0.0.1 }\nmain:\n  - nombre: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("'host' y 'puerto'")), "{err}");

        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: p, tipo: grpc, host: 127.0.0.1, puerto: 9101, path: ./p.wasm }\nmain:\n  - nombre: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("'path'")), "{err}");
    }

    /// `embebido` con campos de más → error; tipo desconocido → error.
    #[test]
    fn embebido_con_campos_y_tipo_desconocido_son_errores() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "emb"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: e, tipo: embebido, path: ./p.wasm }\nmain:\n  - nombre: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("no aplican")), "{err}");

        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: e, tipo: raro }\nmain:\n  - nombre: a\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("raro")), "{err}");
    }

    /// Dos ejecutores con el mismo nombre → error. Nombre reservado → error.
    #[test]
    fn nombres_duplicados_y_reservados_son_errores() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "dups"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: a, tipo: embebido }\n  - { nombre: a, tipo: embebido }\nmain:\n  - nombre: p\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("más de una vez")), "{err}");

        std::fs::write(&y, format!("nombre: s\nejecutores:\n  - {{ nombre: {NOMBRE_EMBEDIDO_RESERVADO}, tipo: embebido }}\nmain:\n  - nombre: p\n")).unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("reservado")), "{err}");
    }

    /// `ejecutor` en un paso `statement`/`sequence_call` → error (es gRPC-only).
    #[test]
    fn ejecutor_en_paso_no_grpc_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5ext_{}", "tipo"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("s.yaml");
        std::fs::write(&y, "nombre: s\nejecutores:\n  - { nombre: e, tipo: embebido }\nmain:\n  - nombre: a\n    tipo: statement\n    statement: 'locals.x = 1'\n    ejecutor: e\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("reservado para 'grpc'")), "{err}");
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
        let ruta = format!("{}/../../ejemplos/demo_ejecutores.yaml", env!("CARGO_MANIFEST_DIR"));
        let prog = cargar_programa_de_archivo(&ruta)
            .unwrap_or_else(|e| panic!("no carga el programa {ruta}: {e}"));
        assert_eq!(prog.raiz.nombre, "demo_ejecutores");
        assert_eq!(prog.ejecutores.len(), 2, "embebido + python");
        assert_eq!(
            prog.ejecutores["python"].tipo,
            TipoEjecutor::Grpc { host: "127.0.0.1".into(), puerto: 9101 }
        );
        assert_eq!(prog.raiz.pasos_main[0].ejecutor, None, "verificar_led → embebido");
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
        let n = aplicar_override_ejecutores(&mut prog, &["py=192.168.1.50:9200".to_string()]).unwrap();
        assert_eq!(n, 1);
        assert_eq!(prog.ejecutores["py"].tipo, TipoEjecutor::Grpc { host: "192.168.1.50".into(), puerto: 9200 });

        // Convertir un embebido en grpc (el usuario fuerza remoto).
        let n = aplicar_override_ejecutores(&mut prog, &["e=192.168.1.60:9300".to_string()]).unwrap();
        assert_eq!(n, 1);
        assert_eq!(prog.ejecutores["e"].tipo, TipoEjecutor::Grpc { host: "192.168.1.60".into(), puerto: 9300 });

        // Formato inválido → error; nombre no declarado → error.
        let err = aplicar_override_ejecutores(&mut prog, &["mal_formado".to_string()]).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("esperado")), "{err}");
        let err = aplicar_override_ejecutores(&mut prog, &["zzz=1.2.3.4:1".to_string()]).unwrap_err();
        assert!(matches!(&err, ErrorCarga::Validacion(m) if m.contains("no está declarado")), "{err}");
    }

    // --- M5 (RF-38): process model Sequential (ADR-0016) ---

    /// `secuencia_usuario: true` implica `sequence_call` y se reescribe a la
    /// clave canónica de la secuencia usuario al cargar el programa.
    #[test]
    fn secuencia_usuario_implica_sequence_call() {
        let yaml = "\
nombre: pm
main:
  - nombre: test_uut
    secuencia_usuario: true
";
        let s = cargar_de_texto(yaml).unwrap();
        let paso = &s.pasos_main[0];
        assert_eq!(paso.tipo, modelo::TipoPaso::SequenceCall);
        assert!(paso.secuencia_usuario);
        assert_eq!(paso.secuencia, None, "el destino se inyecta al cargar el programa");
    }

    /// `secuencia_usuario: true` con `secuencia` o `parametros` → error
    /// (la frontera PM↔usuario se comunica por `asigna`/`locals`).
    #[test]
    fn secuencia_usuario_con_destino_o_parametros_es_error() {
        let casos = [
            (
                "nombre: pm\nmain:\n  - nombre: c\n    secuencia_usuario: true\n    secuencia: ./h.yaml\n",
                "no se declara destino",
            ),
            (
                "nombre: pm\nmain:\n  - nombre: c\n    secuencia_usuario: true\n    parametros: { p: locals.x }\n",
                "MVP-parcial",
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

    /// Sin `--process-model`, un YAML con `secuencia_usuario: true` falla al
    /// cargar el programa (no hay secuencia usuario que invocar).
    #[test]
    fn secuencia_usuario_sin_process_model_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5_{}", "sin_pm"));
        std::fs::create_dir_all(&dir).unwrap();
        let y = dir.join("pm.yaml");
        std::fs::write(&y, "nombre: pm\nmain:\n  - nombre: c\n    secuencia_usuario: true\n").unwrap();
        let err = cargar_programa_de_archivo(y.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("--process-model")),
            "{err}"
        );
    }

    /// `cargar_programa_con_process_model`: el PM es la raíz, la secuencia
    /// usuario se inyecta bajo `CLAVE_SECUENCIA_USUARIO` y el call se
    /// reescribe a esa clave.
    #[test]
    fn process_model_inyecta_secuencia_usuario() {
        let dir = std::env::temp_dir().join(format!("anvil_m5_{}", "pm_ok"));
        std::fs::create_dir_all(&dir).unwrap();
        let pm = dir.join("pm.yaml");
        std::fs::write(
            &pm,
            "nombre: pm\nlocals: { estado_uut: '' }\nmain:\n  - nombre: test_uut\n    secuencia_usuario: true\n    asigna: { estado_uut: '${resultado.estado}' }\n",
        )
        .unwrap();
        let usuario = dir.join("usuario.yaml");
        std::fs::write(&usuario, "nombre: usuario\nmain:\n  - nombre: m\n").unwrap();

        let prog = cargar_programa_con_process_model(pm.to_str().unwrap(), usuario.to_str().unwrap()).unwrap();
        assert_eq!(prog.raiz.nombre, "pm", "el PM es la raíz");
        let call = &prog.raiz.pasos_main[0];
        assert_eq!(call.secuencia.as_deref(), Some(modelo::CLAVE_SECUENCIA_USUARIO));
        let sub = prog.archivos.get(modelo::CLAVE_SECUENCIA_USUARIO).unwrap();
        assert_eq!(sub.nombre, "usuario");
    }

    /// Un PM que no invoca a la secuencia usuario → error claro.
    #[test]
    fn process_model_sin_invocacion_usuario_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5_{}", "pm_sin_call"));
        std::fs::create_dir_all(&dir).unwrap();
        let pm = dir.join("pm.yaml");
        std::fs::write(&pm, "nombre: pm\nmain:\n  - nombre: m\n").unwrap();
        let usuario = dir.join("usuario.yaml");
        std::fs::write(&usuario, "nombre: usuario\nmain:\n  - nombre: m\n").unwrap();

        let err = cargar_programa_con_process_model(pm.to_str().unwrap(), usuario.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("no invoca a la secuencia usuario")),
            "{err}"
        );
    }

    /// La secuencia usuario no puede usar `secuencia_usuario` (sólo el PM):
    /// al cargarla como programa (sin `--process-model`) ya falla.
    #[test]
    fn process_model_usuario_con_secuencia_usuario_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5_{}", "pm_usuario_flag"));
        std::fs::create_dir_all(&dir).unwrap();
        let pm = dir.join("pm.yaml");
        std::fs::write(&pm, "nombre: pm\nmain:\n  - nombre: c\n    secuencia_usuario: true\n").unwrap();
        let usuario = dir.join("usuario.yaml");
        std::fs::write(&usuario, "nombre: usuario\nmain:\n  - nombre: c\n    secuencia_usuario: true\n").unwrap();

        let err = cargar_programa_con_process_model(pm.to_str().unwrap(), usuario.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("--process-model")),
            "{err}"
        );
    }

    /// La secuencia usuario no puede referenciar al PM: el PM se carga como
    /// subsecuencia externa del usuario y su `secuencia_usuario` falla (no
    /// hay `--process-model` en ese contexto) — el ciclo queda cortado por
    /// construcción.
    #[test]
    fn process_model_usuario_no_puede_referenciar_al_pm() {
        let dir = std::env::temp_dir().join(format!("anvil_m5_{}", "pm_usuario_a_pm"));
        std::fs::create_dir_all(&dir).unwrap();
        let pm = dir.join("pm.yaml");
        std::fs::write(&pm, "nombre: pm\nmain:\n  - nombre: c\n    secuencia_usuario: true\n").unwrap();
        let usuario = dir.join("usuario.yaml");
        std::fs::write(&usuario, "nombre: usuario\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./pm.yaml\n").unwrap();

        let err = cargar_programa_con_process_model(pm.to_str().unwrap(), usuario.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("--process-model")),
            "{err}"
        );
    }

    /// Colisión de subsecuencia externa entre PM y usuario → error.
    #[test]
    fn process_model_colision_de_subsecuencia_es_error() {
        let dir = std::env::temp_dir().join(format!("anvil_m5_{}", "pm_colision"));
        std::fs::create_dir_all(&dir).unwrap();
        let pm = dir.join("pm.yaml");
        std::fs::write(
            &pm,
            "nombre: pm\nmain:\n  - nombre: c\n    secuencia_usuario: true\n  - nombre: c2\n    tipo: sequence_call\n    secuencia: ./comun.yaml\n",
        )
        .unwrap();
        std::fs::write(&dir.join("comun.yaml"), "nombre: comun\nmain:\n  - nombre: m\n").unwrap();
        let usuario = dir.join("usuario.yaml");
        std::fs::write(
            &usuario,
            "nombre: usuario\nmain:\n  - nombre: c\n    tipo: sequence_call\n    secuencia: ./comun.yaml\n",
        )
        .unwrap();

        let err = cargar_programa_con_process_model(pm.to_str().unwrap(), usuario.to_str().unwrap()).unwrap_err();
        assert!(
            matches!(&err, ErrorCarga::Validacion(m) if m.contains("colisión")),
            "{err}"
        );
    }

    /// El ejemplo `ejemplos/process_model_sequential.yaml` carga como PM con
    /// la secuencia usuario inyectada.
    #[test]
    fn ejemplo_process_model_carga_con_usuario() {
        let pm = format!("{}/../../ejemplos/process_model_sequential.yaml", env!("CARGO_MANIFEST_DIR"));
        let usuario = format!("{}/../../ejemplos/basica.yaml", env!("CARGO_MANIFEST_DIR"));
        let prog = cargar_programa_con_process_model(&pm, &usuario)
            .unwrap_or_else(|e| panic!("no carga el PM {pm}: {e}"));
        assert_eq!(prog.raiz.nombre, "process_model_sequential");
        let call = prog.raiz.pasos_main.iter().find(|p| p.secuencia_usuario).expect("el PM invoca al usuario");
        assert_eq!(call.secuencia.as_deref(), Some(modelo::CLAVE_SECUENCIA_USUARIO));
        assert_eq!(
            prog.archivos.get(modelo::CLAVE_SECUENCIA_USUARIO).map(|d| d.nombre.as_str()),
            Some("basica")
        );
    }
}
