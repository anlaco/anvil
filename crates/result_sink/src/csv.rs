//! Sink CSV: una fila por paso, con quoting RFC-4180. Pensado para
//! humanos/Excel — los números van con el formato del cable (`5` y no
//! `5.0`, vía `modelo::proto::a_texto`), y los campos sin medida quedan
//! vacíos.
//!
//! El documento se ensambla a un `String` y se escribe de un tiro con
//! reintento (`reintento::escribir_con_reintentos`): los resultados de una
//! secuencia son pequeños, no merece la pena escribir fila a fila.

use modelo::proto::a_texto;
use modelo::{ResultSink, ResultadoSecuencia, ResultadoStep};
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
    "valor_esperado",
    "operador",
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
            // `padre` es el prefijo de ruta para los sub-pasos anidados
            // (M4b): el paso top-level no lleva prefijo; sus sub-pasos se
            // aplanean como `padre/hijo` (recursivo). Sin columnas nuevas.
            escribe_filas(&mut doc, estado, p, &p.nombre);
        }
        if let Err(e) = escribir_con_reintentos(&mut self.salida, REINTENTOS, doc.as_bytes()) {
            eprintln!("sink csv: no se pudo escribir: {e}");
        }
    }
}

/// Aplanea un paso y, recursivamente, sus `sub_pasos` como filas CSV. El
/// `prefijo` es el `nombre_paso` acumulado (`padre/hijo/...`); el paso en
/// curso se emite con `prefijo` como `nombre_paso` y, si tiene sub-pasos,
/// éstos se emiten a continuación con `prefijo/sub.nombre`.
fn escribe_filas(doc: &mut String, estado_secuencia: &str, p: &ResultadoStep, prefijo: &str) {
    fila(doc, fila_paso(estado_secuencia, p, prefijo));
    if let Some(sub) = &p.sub_pasos {
        for sp in sub {
            let hijo = format!("{prefijo}/{}", sp.nombre);
            escribe_filas(doc, estado_secuencia, sp, &hijo);
        }
    }
}

/// Construye los 10 campos de una fila de paso, ya como `String`. `nombre`
/// es el `nombre_paso` a emitir (el original o el prefijo `padre/hijo` para
/// sub-pasos aplanados).
fn fila_paso(estado_secuencia: &str, p: &ResultadoStep, nombre: &str) -> Vec<String> {
    vec![
        estado_secuencia.to_string(),
        p.estado.clone(),
        nombre.to_string(),
        p.estado.clone(),
        p.mensaje.clone(),
        a_texto(p.valor_medido),
        a_texto(p.limite_min),
        a_texto(p.limite_max),
        a_texto(p.valor_esperado),
        p.operador.map(|op| op.simbolo().to_string()).unwrap_or_default(),
    ]
}

/// Escapa un campo según RFC-4180 y lo añade a `fila`: si contiene coma,
/// comilla, CR o LF, lo envuelve en comillas y duplica las comillas internas.
fn csv_campo(campo: &str, fila: &mut String) {
    let necesita =
        campo.contains(',') || campo.contains('"') || campo.contains('\n') || campo.contains('\r');
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
        s.registra(ResultadoStep::medido(
            "medir_voltaje",
            "fallo",
            "fuera de rango",
            4.2,
            4.5,
            5.5,
        ));
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
        assert_eq!(lineas[0], "nombre_secuencia,estado,nombre_paso,estado_paso,mensaje,valor_medido,limite_min,limite_max,valor_esperado,operador");
        // rango: valor_esperado/operador vacíos (no aplican a un rango).
        assert_eq!(lineas[1], "fallo,fallo,medir_voltaje,fallo,fuera de rango,4.2,4.5,5.5,,");
        // sin medida ni límite: los últimos cinco campos vacíos.
        assert_eq!(lineas[2], "fallo,paso,verificar_led,paso,led encendido,,,,,");
        assert!(lineas[3].is_empty(), "termina en CRLF");
    }

    #[test]
    fn comparacion_llena_valor_esperado_y_operador() {
        use modelo::Operador;
        let mut s = ResultadoSecuencia::nueva("s");
        let mut r = ResultadoStep::medido_valor(
            "verificar_frecuencia",
            "fallo",
            "990 >= 1000 no cumplido",
            990.0,
        );
        r.operador = Some(Operador::Ge);
        r.valor_esperado = Some(1000.0);
        s.registra(r);

        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let out = String::from_utf8(sink.salida).unwrap();
        let fila = out.split("\r\n").nth(1).unwrap();
        // valor_medido=990, limite_min/max vacíos, valor_esperado=1000, operador=">=".
        assert!(fila.contains(",990,,,1000,>="), "fila de comparación: {fila}");
    }

    #[test]
    fn cita_campos_con_coma_comilla_y_salto() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo("p", "paso", "hola, \"mundo\"\ny tal"));
        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);

        let out = String::from_utf8(sink.salida).unwrap();
        let segunda_linea = out.split("\r\n").nth(1).unwrap();
        assert!(
            segunda_linea.contains("\"hola, \"\"mundo\"\"\ny tal\""),
            "comilla doblada y campo entrecomillado: {segunda_linea}"
        );
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

    #[test]
    fn sequence_call_aplanea_sub_pasos_con_prefijo() {
        // Un sequence call (M4b): el call se emite con su nombre, y cada
        // sub-paso como fila extra con `nombre_paso = call/hijo`. La
        // cabecera no cambia (sin columnas nuevas).
        let mut call = ResultadoStep::nuevo("test_fuentes", "fallo", "sequence call → fallo");
        call.sub_pasos = Some(vec![
            ResultadoStep::nuevo("medir_canal_1", "paso", "ok"),
            ResultadoStep::nuevo("medir_canal_2", "fallo", "fuera de rango"),
        ]);
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(call);
        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let out = String::from_utf8(sink.salida).unwrap();
        let lineas: Vec<&str> = out.split("\r\n").collect();
        // Cabecera sin cambios.
        assert_eq!(
            lineas[0],
            "nombre_secuencia,estado,nombre_paso,estado_paso,mensaje,valor_medido,limite_min,limite_max,valor_esperado,operador"
        );
        // Call.
        assert!(lineas[1].contains(",fallo,test_fuentes,fallo,"), "fila del call: {}", lineas[1]);
        // Sub-pasos aplanados con prefijo.
        assert!(
            lineas[2].contains(",paso,test_fuentes/medir_canal_1,paso,ok,"),
            "sub-paso 1: {}",
            lineas[2]
        );
        assert!(
            lineas[3].contains(",fallo,test_fuentes/medir_canal_2,fallo,fuera de rango,"),
            "sub-paso 2: {}",
            lineas[3]
        );
    }
}
