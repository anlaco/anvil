//! Sink CSV: una fila por paso, con quoting RFC-4180. Pensado para
//! humanos/Excel — los números van con el formato del cable (`5` y no
//! `5.0`, vía `modelo::proto::a_texto`), y los campos sin medida quedan
//! vacíos.
//!
//! El documento se ensambla a un `String` y se escribe de un tiro con
//! reintento (`reintento::escribir_con_reintentos`): los resultados de una
//! secuencia son pequeños, no merece la pena escribir fila a fila.

use modelo::proto::a_texto;
use modelo::{ResultadoSecuencia, ResultadoStep, ResultSink};
use std::io::Write;

use crate::reintento::escribir_con_reintentos;

/// Número de intentos de escritura ante fallos transitorios (RF-23).
const REINTENTOS: u32 = 3;

/// Columnas del CSV, en orden.
const CABECERA: &[&str] = &[
    "nombre_secuencia",
    "estado",
    "nombre_paso",
    "estado_paso",
    "mensaje",
    "valor_medido",
    "limite_min",
    "limite_max",
];

/// Verte el resultado a un `Write` como CSV (una fila por paso).
pub struct SinkCsv<W: Write> {
    salida: W,
}

impl<W: Write> SinkCsv<W> {
    pub fn nuevo(salida: W) -> Self {
        SinkCsv { salida }
    }
}

impl<W: Write> ResultSink for SinkCsv<W> {
    fn on_fin_secuencia(&mut self, secuencia: &ResultadoSecuencia) {
        let mut doc = String::new();
        fila(&mut doc, CABECERA.iter().map(|s| (*s).to_string()).collect());
        let estado = secuencia.estado();
        for p in &secuencia.pasos {
            fila(&mut doc, fila_paso(estado, p));
        }
        if let Err(e) = escribir_con_reintentos(&mut self.salida, REINTENTOS, doc.as_bytes()) {
            eprintln!("sink csv: no se pudo escribir: {e}");
        }
    }
}

/// Construye los 8 campos de una fila de paso, ya como `String`.
fn fila_paso(estado_secuencia: &str, p: &ResultadoStep) -> Vec<String> {
    vec![
        estado_secuencia.to_string(),
        p.estado.clone(),
        p.nombre.clone(),
        p.estado.clone(),
        p.mensaje.clone(),
        a_texto(p.valor_medido),
        a_texto(p.limite_min),
        a_texto(p.limite_max),
    ]
}

/// Escapa un campo según RFC-4180 y lo añade a `fila`: si contiene coma,
/// comilla, CR o LF, lo envuelve en comillas y duplica las comillas internas.
fn csv_campo(campo: &str, fila: &mut String) {
    let necesita = campo.contains(',')
        || campo.contains('"')
        || campo.contains('\n')
        || campo.contains('\r');
    if necesita {
        fila.push('"');
        for c in campo.chars() {
            if c == '"' {
                fila.push('"');
                fila.push('"');
            } else {
                fila.push(c);
            }
        }
        fila.push('"');
    } else {
        fila.push_str(campo);
    }
}

/// Escribe una fila (campos separados por coma, terminada en CRLF) al doc.
fn fila(doc: &mut String, campos: Vec<String>) {
    let mut primero = true;
    for c in &campos {
        if !primero {
            doc.push(',');
        }
        csv_campo(c, doc);
        primero = false;
    }
    doc.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use modelo::{ResultadoSecuencia, ResultadoStep};

    fn secuencia_ejemplo() -> ResultadoSecuencia {
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(ResultadoStep::medido("medir_voltaje", "fallo", "fuera de rango", 4.2, 4.5, 5.5));
        s.registra(ResultadoStep::nuevo("verificar_led", "paso", "led encendido"));
        s
    }

    #[test]
    fn cabecera_y_una_fila_por_paso() {
        let s = secuencia_ejemplo();
        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);

        let out = String::from_utf8(sink.salida).unwrap();
        let lineas: Vec<&str> = out.split("\r\n").collect();
        assert_eq!(lineas[0], "nombre_secuencia,estado,nombre_paso,estado_paso,mensaje,valor_medido,limite_min,limite_max");
        assert_eq!(lineas[1], "fallo,fallo,medir_voltaje,fallo,fuera de rango,4.2,4.5,5.5");
        assert_eq!(lineas[2], "fallo,paso,verificar_led,paso,led encendido,,,");
        assert!(lineas[3].is_empty(), "termina en CRLF");
    }

    #[test]
    fn cita_campos_con_coma_comilla_y_salto() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo("p", "paso", "hola, \"mundo\"\ny tal"));
        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);

        let out = String::from_utf8(sink.salida).unwrap();
        let segunda_linea = out.split("\r\n").nth(1).unwrap();
        assert!(segunda_linea.contains("\"hola, \"\"mundo\"\"\ny tal\""), "comilla doblada y campo entrecomillado: {segunda_linea}");
    }

    #[test]
    fn enteros_sin_decimales() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::medido("p", "paso", "ok", 5.0, 0.0, 10.0));
        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let out = String::from_utf8(sink.salida).unwrap();
        let fila = out.split("\r\n").nth(1).unwrap();
        assert!(fila.contains(",5,0,10"), "enteros como 5/0/10 sin decimales: {fila}");
    }
}