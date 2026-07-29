//! El modelo de datos del secuenciador y los mensajes de `paso.proto`.
//!
//! Port de `secuenciador/modelo.ana` + `secuenciador/rpc/paso_codec.ana`.
//! La especificación no cambia con la migración a Rust: mismos campos,
//! mismos estados, mismo contrato en el cable.

pub mod proto;

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

    /// Port de `ejecutor.ana:registra`.
    pub fn registra(&mut self, paso: ResultadoStep) {
        self.pasos.push(paso);
    }

    /// Port de `ejecutor.ana:estado_de`. Un `error` en cualquier paso manda
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

    /// Port de `ejecutor.ana:reporte`. El formato es parte de la spec: la
    /// salida debe ser idéntica a la de la versión Ana.
    pub fn reporte(&self) {
        println!("=== {}: {} ===", self.nombre, self.estado());
        for p in &self.pasos {
            println!("  [{}] {}: {}", p.estado, p.nombre, p.mensaje);
        }
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
}
