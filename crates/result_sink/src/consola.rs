//! Sink de consola: el reporte textual **congelado** (RNF-08). No es un
//! formato nuevo: reutiliza `ResultadoSecuencia::reporte_a` para producir
//! los mismos bytes que el `println!` original, pero como un sink más del
//! lifecycle. Así quien dependía del formato de consola lo sigue teniendo
//! sin que el motor sepa que imprime.

use modelo::{ResultSink, ResultadoSecuencia};

/// Verte el resultado a un `Write` con el formato textual congelado.
///
/// Genérico sobre `W: std::io::Write` para que los tests lo comprueben con
/// un `Vec<u8>`; el bin pasa `std::io::stdout`. Los errores de escritura
/// se tragan (best-effort, RF-23) y se loguean a stderr: un fallo de
/// consola no rompe la secuencia.
pub struct SinkConsola<W: std::io::Write> {
    salida: W,
}

impl<W: std::io::Write> SinkConsola<W> {
    pub fn nuevo(salida: W) -> Self {
        SinkConsola { salida }
    }
}

impl<W: std::io::Write> ResultSink for SinkConsola<W> {
    fn on_fin_secuencia(&mut self, secuencia: &ResultadoSecuencia) {
        if let Err(e) = secuencia.reporte_a(&mut self.salida) {
            eprintln!("sink consola: no se pudo escribir el reporte: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modelo::{ResultadoSecuencia, ResultadoStep};

    #[test]
    fn produce_el_formato_congelado() {
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(ResultadoStep::medido(
            "medir_voltaje",
            "fail",
            "voltaje fuera de rango",
            4.2,
            4.5,
            5.5,
        ));
        s.registra(ResultadoStep::nuevo(
            "verificar_led",
            "pass",
            "led encendido",
        ));

        let mut sink = SinkConsola::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);

        let esperado = "\
=== basica: fail ===
  [fail] medir_voltaje: voltaje fuera de rango
  [pass] verificar_led: led encendido
";
        assert_eq!(String::from_utf8(sink.salida).unwrap(), esperado);
    }
}
