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

use modelo::{Asignacion, DefinicionPaso, DefinicionSecuencia, Limite, Operador, TipoPaso, ValorDefinicion};
use serde::Deserialize;
use std::collections::HashMap;

/// Una secuencia como se lee del YAML, antes de traducirse al modelo del
/// motor. `deny_unknown_fields` hace que un campo no reconocido falle la
/// carga en vez de ignorarse en silencio.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecuenciaYaml {
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
/// el disco; `cargar_de_archivo` lo envuelve.
pub fn cargar_de_texto(texto: &str) -> Result<DefinicionSecuencia, ErrorCarga> {
    let yaml: SecuenciaYaml = noyalib::from_str(texto)?;

    validar(&yaml)?;

    // Validar y traducir los límites embebidos (RF-29) antes de mover los pasos.
    let traduce_pasos = |pasos: Vec<PasoYaml>| -> Result<Vec<DefinicionPaso>, ErrorCarga> {
        pasos.into_iter().map(PasoYaml::a_definicion).collect()
    };

    Ok(DefinicionSecuencia {
        nombre: yaml.nombre,
        pasos_setup: traduce_pasos(yaml.setup)?,
        pasos_main: traduce_pasos(yaml.main)?,
        pasos_cleanup: traduce_pasos(yaml.cleanup)?,
        locals: yaml.locals.into_iter().map(|(k, v)| (k, v.a_definicion())).collect(),
        parameters: yaml.parameters.into_iter().map(|(k, v)| (k, v.a_definicion())).collect(),
        file_globals: yaml.file_globals.into_iter().map(|(k, v)| (k, v.a_definicion())).collect(),
    })
}

/// Carga una secuencia desde un fichero YAML en disco.
pub fn cargar_de_archivo(ruta: &str) -> Result<DefinicionSecuencia, ErrorCarga> {
    let texto = std::fs::read_to_string(ruta)?;
    cargar_de_texto(&texto)
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

/// Reglas de negocio que el schema por sí solo no expresa.
fn validar(y: &SecuenciaYaml) -> Result<(), ErrorCarga> {
    if y.nombre.trim().is_empty() {
        return Err(ErrorCarga::Validacion("el nombre de la secuencia no puede estar vacío".into()));
    }
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

        // RF-27: tipo de paso. `grpc` (default) o `statement`.
        let tipo = match self.tipo.as_str() {
            "grpc" => TipoPaso::Grpc,
            "statement" => TipoPaso::Statement,
            otro => {
                return Err(ErrorCarga::Validacion(format!(
                    "el paso '{}' tiene tipo '{otro}' inválido (grpc|statement)",
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

        // Coherencia tipo ↔ statement.
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
        assert!(matches!(err, ErrorCarga::Sintaxis(_)), "main ausente debe ser error de schema, no de validación: {err}");
    }

    #[test]
    fn main_vacio_es_error_de_validacion() {
        let yaml = "nombre: s\nmain: []\n";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("main")));
    }

    #[test]
    fn nombre_vacio_es_error() {
        let yaml = "\
nombre: ''
main:
  - nombre: un_paso
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("nombre")));
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("reintentos")));
    }

    #[test]
    fn campo_desconocido_es_error() {
        // Desde M4, `disable`/`pause_on_fail`/`precondicion`/`asigna`/`tipo`/
        // `statement` son campos conocidos. Usamos uno realmente desconocido
        // (`foo`) para seguir probando `deny_unknown_fields` (fail-fast).
        let yaml = "\
nombre: s
main:
  - nombre: un_paso
    foo: bar
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(err, ErrorCarga::Sintaxis(_)), "campo desconocido debe ser error de schema: {err}");
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("min")), "{err}");
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains(">")), "{err}");
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("op")), "{err}");
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("op")), "{err}");
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("tipo")), "{err}");
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
        assert!(matches!(err, ErrorCarga::Sintaxis(_)), "{err}");
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
        assert!(matches!(err, ErrorCarga::Sintaxis(_)));
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("medir") && m.contains("precondición")),
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("'x'") && m.contains("medir")),
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("statement")), "{err}");
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("statement")), "{err}");
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
        assert!(matches!(err, ErrorCarga::Validacion(ref m) if m.contains("magia")), "{err}");
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
}