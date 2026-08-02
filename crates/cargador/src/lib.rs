//! Cargador de secuencias desde YAML: lee un fichero de secuencia y lo
//! traduce a `modelo::DefinicionSecuencia`. El motor no cambia (ADR-0005):
//! aquí sólo producimos los datos que el motor ya sabe recorrer.
//!
//! El schema de M1 es un **subconjunto estricto**: `nombre`, `reintentos`
//! y las tres secciones (`setup`, `main`, `cleanup`). Los campos de hitos
//! posteriores (`limite`, `disable`, `pause_on_fail`, `precondicion`,
//! `parametros`) **no se admiten** todavía —`deny_unknown_fields` los
//! rechaza al cargar (fail-fast) para que el schema crezca de forma
//! deliberada, no por accidente.

use modelo::{DefinicionPaso, DefinicionSecuencia};
use serde::Deserialize;

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
}

/// Un paso como se lee del YAML. `reintentos` por defecto es 1 (un solo
/// tiro) si se omite.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PasoYaml {
    nombre: String,
    #[serde(default = "reintentos_por_defecto")]
    reintentos: u32,
}

fn reintentos_por_defecto() -> u32 {
    1
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

    Ok(DefinicionSecuencia {
        nombre: yaml.nombre,
        pasos_setup: yaml.setup.into_iter().map(PasoYaml::a_definicion).collect(),
        pasos_main: yaml.main.into_iter().map(PasoYaml::a_definicion).collect(),
        pasos_cleanup: yaml.cleanup.into_iter().map(PasoYaml::a_definicion).collect(),
    })
}

/// Carga una secuencia desde un fichero YAML en disco.
pub fn cargar_de_archivo(ruta: &str) -> Result<DefinicionSecuencia, ErrorCarga> {
    let texto = std::fs::read_to_string(ruta)?;
    cargar_de_texto(&texto)
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
    fn a_definicion(self) -> DefinicionPaso {
        DefinicionPaso::nuevo(&self.nombre, self.reintentos)
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
        // El mismo contenido que crates/motor/src/bin/basica_datos.rs.
        let s = cargar_de_texto(basica_yaml()).unwrap();
        let esperada = modelo::DefinicionSecuencia {
            nombre: "basica".into(),
            pasos_setup: vec![DefinicionPaso::nuevo("conectar_equipo", 3)],
            pasos_main: vec![
                DefinicionPaso::nuevo("medir_voltaje", 1),
                DefinicionPaso::nuevo("verificar_led", 1),
            ],
            pasos_cleanup: vec![DefinicionPaso::nuevo("desconectar_equipo", 1)],
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
        assert!(matches!(err, ErrorCarga::Validacion(m) if m.contains("main")));
    }

    #[test]
    fn nombre_vacio_es_error() {
        let yaml = "\
nombre: ''
main:
  - nombre: un_paso
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(err, ErrorCarga::Validacion(m) if m.contains("nombre")));
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
        assert!(matches!(err, ErrorCarga::Validacion(m) if m.contains("reintentos")));
    }

    #[test]
    fn campo_desconocido_es_error() {
        // `limite` llega en M3; hoy se rechaza (fail-fast).
        let yaml = "\
nombre: s
main:
  - nombre: un_paso
    limite:
      tipo: rango
";
        let err = cargar_de_texto(yaml).unwrap_err();
        assert!(matches!(err, ErrorCarga::Sintaxis(_)), "campo desconocido debe ser error de schema: {err}");
    }

    #[test]
    fn yaml_mal_formado_es_error_de_sintaxis() {
        let err = cargar_de_texto("nombre: [sin cerrar").unwrap_err();
        assert!(matches!(err, ErrorCarga::Sintaxis(_)));
    }
}