//! El modelo de datos del secuenciador y los mensajes de `paso.proto`:
//! mismos campos, mismos estados, mismo contrato en el cable.

pub mod proto;
pub mod result_sink;
pub use result_sink::{ResultSink, SinkCompuesto};

/// Lo que un paso YA corrido devolvió.
///
/// `estado` es uno de `"paso"`, `"fallo"` o `"error"` — se mantiene como
/// texto (y no como enum) porque viaja así en `paso.proto` y porque el
/// contrato admite pasos escritos en cualquier lenguaje.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultadoStep {
    pub nombre: String,
    pub estado: String,
    pub mensaje: String,
    pub valor_medido: Option<f64>,
    pub limite_min: Option<f64>,
    pub limite_max: Option<f64>,
}

impl ResultadoStep {
    /// Un resultado sin medida asociada (el caso común).
    pub fn nuevo(nombre: &str, estado: &str, mensaje: impl Into<String>) -> Self {
        ResultadoStep {
            nombre: nombre.to_string(),
            estado: estado.to_string(),
            mensaje: mensaje.into(),
            valor_medido: None,
            limite_min: None,
            limite_max: None,
        }
    }

    /// Un resultado con medida y límites (p. ej. una medida de voltaje).
    pub fn medido(
        nombre: &str,
        estado: &str,
        mensaje: impl Into<String>,
        valor: f64,
        min: f64,
        max: f64,
    ) -> Self {
        ResultadoStep {
            valor_medido: Some(valor),
            limite_min: Some(min),
            limite_max: Some(max),
            ..ResultadoStep::nuevo(nombre, estado, mensaje)
        }
    }

    pub fn paso(&self) -> bool {
        self.estado == "paso"
    }
}

/// El resultado agregado de una secuencia corrida.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResultadoSecuencia {
    pub nombre: String,
    pub pasos: Vec<ResultadoStep>,
}

impl ResultadoSecuencia {
    pub fn nueva(nombre: &str) -> Self {
        ResultadoSecuencia { nombre: nombre.to_string(), pasos: Vec::new() }
    }

    /// Añade un resultado de paso al agregado de la secuencia.
    pub fn registra(&mut self, paso: ResultadoStep) {
        self.pasos.push(paso);
    }

    /// Estado agregado de la secuencia. Un `error` en cualquier paso manda
    /// sobre un `fallo`; sin ninguno de los dos, la secuencia pasa.
    pub fn estado(&self) -> &'static str {
        if self.pasos.iter().any(|p| p.estado == "error") {
            "error"
        } else if self.pasos.iter().any(|p| p.estado == "fallo") {
            "fallo"
        } else {
            "paso"
        }
    }

    /// Reporte de la secuencia en texto. El formato es parte de la spec
    /// (RNF-08): no se toca sin querer tocar la especificación.
    ///
    /// Escribe al `Write` que se le pase, para que el sink de consola (y
    /// los tests) no se acoplen a stdout. `reporte()` (la API pública
    /// congelada) delega aquí con `stdout` y produce los mismos bytes que
    /// el `println!` original.
    pub fn reporte_a(&self, w: &mut impl std::io::Write) -> std::io::Result<()> {
        writeln!(w, "=== {}: {} ===", self.nombre, self.estado())?;
        for p in &self.pasos {
            writeln!(w, "  [{}] {}: {}", p.estado, p.nombre, p.mensaje)?;
        }
        Ok(())
    }

    /// Reporte de la secuencia a stdout. La API pública congelada (RNF-08):
    /// delega en `reporte_a` con `stdout` y traga el error de IO, igual
    /// que hacía el `println!` original.
    pub fn reporte(&self) {
        let _ = self.reporte_a(&mut std::io::stdout());
    }
}

/// Los datos que describen QUÉ correr — a diferencia de `ResultadoStep`,
/// que es lo que un paso ya corrido devolvió.
#[derive(Debug, Clone, PartialEq)]
pub struct DefinicionPaso {
    pub nombre: String,
    /// Número máximo de intentos (1 = sin reintentos).
    pub reintentos: u32,
}

impl DefinicionPaso {
    pub fn nuevo(nombre: &str, reintentos: u32) -> Self {
        DefinicionPaso { nombre: nombre.to_string(), reintentos }
    }
}

/// Una secuencia como datos: el motor la recorre sin saber qué hace cada
/// paso, invocándolos por gRPC por nombre.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DefinicionSecuencia {
    pub nombre: String,
    pub pasos_setup: Vec<DefinicionPaso>,
    pub pasos_main: Vec<DefinicionPaso>,
    pub pasos_cleanup: Vec<DefinicionPaso>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estado_agregado() {
        let mut s = ResultadoSecuencia::nueva("s");
        assert_eq!(s.estado(), "paso", "una secuencia vacía pasa");

        s.registra(ResultadoStep::nuevo("a", "paso", "ok"));
        assert_eq!(s.estado(), "paso");

        s.registra(ResultadoStep::nuevo("b", "fallo", "mal"));
        assert_eq!(s.estado(), "fallo");

        // error manda sobre fallo, aunque llegue después.
        s.registra(ResultadoStep::nuevo("c", "error", "peor"));
        assert_eq!(s.estado(), "error");
    }

    #[test]
    fn error_manda_aunque_llegue_antes() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo("a", "error", "peor"));
        s.registra(ResultadoStep::nuevo("b", "fallo", "mal"));
        assert_eq!(s.estado(), "error");
    }

    /// RNF-08: el formato textual de `reporte_a` es spec congelada.
    /// Este test congela los bytes exactos para detectar cambios
    /// accidentales.
    #[test]
    fn reporte_a_congela_el_formato() {
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(ResultadoStep::medido("medir_voltaje", "fallo", "voltaje fuera de rango", 4.2, 4.5, 5.5));
        s.registra(ResultadoStep::nuevo("verificar_led", "paso", "led encendido"));

        let mut out = Vec::new();
        s.reporte_a(&mut out).unwrap();

        let esperado = "\
=== basica: fallo ===
  [fallo] medir_voltaje: voltaje fuera de rango
  [paso] verificar_led: led encendido
";
        assert_eq!(String::from_utf8(out).unwrap(), esperado);
    }
}
