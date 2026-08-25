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

/// Columnas del CSV, en orden. `fase` se añadió al **final** a propósito:
/// así quien lea por índice las diez originales no se rompe.
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
    "fase",
    // ADR-0020, al final por el mismo motivo que `fase`. Un CSV no puede
    // tener columnas distintas por fila, así que los valores con nombre van
    // compactados en una sola celda (ver `nombrados_a_csv`) en vez de una
    // columna por parámetro — que es lo que rompería el formato en cuanto dos
    // pasos declararan parámetros distintos.
    "parametros",
    "salidas",
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
        fila(
            &mut doc,
            CABECERA.iter().map(|s| (*s).to_string()).collect(),
        );
        let estado = secuencia.estado();
        for p in &secuencia.pasos {
            // `padre` es el prefijo de ruta para los sub-pasos anidados
            // (M4b): el paso top-level no lleva prefijo; sus sub-pasos se
            // aplanean como `padre/hijo` (recursivo). Sin columnas nuevas.
            escribe_filas(&mut doc, &secuencia.nombre, estado, p, &p.nombre);
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
fn escribe_filas(
    doc: &mut String,
    nombre_secuencia: &str,
    estado_secuencia: &str,
    p: &ResultadoStep,
    prefijo: &str,
) {
    fila(
        doc,
        fila_paso(nombre_secuencia, estado_secuencia, p, prefijo),
    );
    if let Some(sub) = &p.sub_pasos {
        for sp in sub {
            let hijo = format!("{prefijo}/{}", sp.nombre);
            escribe_filas(doc, nombre_secuencia, estado_secuencia, sp, &hijo);
        }
    }
}

/// Construye los campos de una fila de paso, ya como `String`. `nombre`
/// es el `nombre_paso` a emitir (el original o el prefijo `padre/hijo` para
/// sub-pasos aplanados).
fn fila_paso(
    nombre_secuencia: &str,
    estado_secuencia: &str,
    p: &ResultadoStep,
    nombre: &str,
) -> Vec<String> {
    vec![
        nombre_secuencia.to_string(),
        estado_secuencia.to_string(),
        nombre.to_string(),
        p.estado.clone(),
        p.mensaje.clone(),
        a_texto(p.valor_medido),
        a_texto(p.limite_min),
        a_texto(p.limite_max),
        a_texto(p.valor_esperado),
        p.operador
            .map(|op| op.simbolo().to_string())
            .unwrap_or_default(),
        p.fase.como_texto().to_string(),
        nombrados_a_csv(&p.parametros),
        nombrados_a_csv(&p.salidas),
    ]
}

/// Valores con nombre en una sola celda: `canal=2;etiqueta=banco-3`.
///
/// El separador es `;` y no `,` para no forzar el entrecomillado de RFC-4180
/// en la mayoría de los casos; si aun así aparece una coma en un valor de
/// texto, `csv_campo` lo escapa igual que a cualquier otro campo.
///
/// Los números se escriben con `a_texto`, el mismo formato que el resto de
/// medidas del fichero: una única fuente de verdad para cómo se escribe un
/// número, en vez de dos que se separan con el tiempo.
fn nombrados_a_csv(vs: &[(String, expr::Value)]) -> String {
    vs.iter()
        .map(|(n, v)| {
            let valor = match v {
                expr::Value::Numero(x) => a_texto(Some(*x)),
                expr::Value::Texto(s) => s.clone(),
                expr::Value::Bool(b) => b.to_string(),
                expr::Value::Nulo => String::new(),
            };
            format!("{n}={valor}")
        })
        .collect::<Vec<_>>()
        .join(";")
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
            "fail",
            "fuera de rango",
            4.2,
            4.5,
            5.5,
        ));
        s.registra(ResultadoStep::nuevo(
            "verificar_led",
            "pass",
            "led encendido",
        ));
        s
    }

    #[test]
    fn cabecera_y_una_fila_por_paso() {
        let s = secuencia_ejemplo();
        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);

        let out = String::from_utf8(sink.salida).unwrap();
        let lineas: Vec<&str> = out.split("\r\n").collect();
        assert_eq!(lineas[0], "nombre_secuencia,estado,nombre_paso,estado_paso,mensaje,valor_medido,limite_min,limite_max,valor_esperado,operador,fase,parametros,salidas");
        // rango: valor_esperado/operador vacíos (no aplican a un rango).
        // primer campo = nombre de la secuencia (DEF-2), segundo = su estado agregado.
        assert_eq!(
            lineas[1],
            "basica,fail,medir_voltaje,fail,fuera de rango,4.2,4.5,5.5,,,main,,"
        );
        // sin medida ni límite: valor_medido..operador vacíos, y la fase al final.
        assert_eq!(
            lineas[2],
            "basica,fail,verificar_led,pass,led encendido,,,,,,main,,"
        );
        assert!(lineas[3].is_empty(), "termina en CRLF");
    }

    /// ADR-0019, Regla 1: la columna `estado` (la de la secuencia) puede traer
    /// `inconcluso`. La columna `estado_paso` no: `inconcluso` lo produce el
    /// motor al agregar, y sólo él.
    #[test]
    fn el_estado_agregado_puede_ser_inconcluso() {
        let mut s = ResultadoSecuencia::nueva("b31");
        s.registra(ResultadoStep::nuevo(
            "verdict",
            "skipped",
            "precondición falsa",
        ));
        s.veredicto_sin_evaluar = true;

        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let out = String::from_utf8(sink.salida).unwrap();
        assert_eq!(
            out.split("\r\n").nth(1).unwrap(),
            "b31,inconclusive,verdict,skipped,precondición falsa,,,,,,main,,"
        );
    }

    #[test]
    fn comparacion_llena_valor_esperado_y_operador() {
        use modelo::Operador;
        let mut s = ResultadoSecuencia::nueva("s");
        let mut r = ResultadoStep::medido_valor(
            "verificar_frecuencia",
            "fail",
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
        assert!(
            fila.contains(",990,,,1000,>="),
            "fila de comparación: {fila}"
        );
    }

    #[test]
    fn cita_campos_con_coma_comilla_y_salto() {
        let mut s = ResultadoSecuencia::nueva("s");
        s.registra(ResultadoStep::nuevo("p", "pass", "hola, \"mundo\"\ny tal"));
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
        s.registra(ResultadoStep::medido("p", "pass", "ok", 5.0, 0.0, 10.0));
        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let out = String::from_utf8(sink.salida).unwrap();
        let fila = out.split("\r\n").nth(1).unwrap();
        assert!(
            fila.contains(",5,0,10"),
            "enteros como 5/0/10 sin decimales: {fila}"
        );
    }

    #[test]
    fn sequence_call_aplanea_sub_pasos_con_prefijo() {
        // Un sequence call (M4b): el call se emite con su nombre, y cada
        // sub-paso como fila extra con `nombre_paso = call/hijo`. El
        // aplanado no añade columnas propias.
        let mut call = ResultadoStep::nuevo("test_fuentes", "fail", "sequence call → fallo");
        call.sub_pasos = Some(vec![
            ResultadoStep::nuevo("medir_canal_1", "pass", "ok"),
            ResultadoStep::nuevo("medir_canal_2", "fail", "fuera de rango"),
        ]);
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(call);
        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let out = String::from_utf8(sink.salida).unwrap();
        let lineas: Vec<&str> = out.split("\r\n").collect();
        assert_eq!(
            lineas[0],
            "nombre_secuencia,estado,nombre_paso,estado_paso,mensaje,valor_medido,limite_min,limite_max,valor_esperado,operador,fase,parametros,salidas"
        );
        // Call. Primer campo = nombre de secuencia, segundo = estado agregado
        // (fallo, el mismo en las tres filas).
        assert!(
            lineas[1].contains("basica,fail,test_fuentes,fail,"),
            "fila del call: {}",
            lineas[1]
        );
        // Sub-pasos aplanados con prefijo.
        assert!(
            lineas[2].contains("basica,fail,test_fuentes/medir_canal_1,pass,ok,"),
            "sub-paso 1: {}",
            lineas[2]
        );
        assert!(
            lineas[3].contains("basica,fail,test_fuentes/medir_canal_2,fail,fuera de rango,"),
            "sub-paso 2: {}",
            lineas[3]
        );
    }

    #[test]
    fn la_ultima_columna_lleva_la_fase_de_cada_paso() {
        use modelo::Fase;
        // Un fallo de Setup, uno de Main y uno de Cleanup: al post-procesar
        // hay que poder distinguirlos (DIAG-3, #8).
        let mut s = ResultadoSecuencia::nueva("basica");
        for (nombre, fase) in [
            ("conectar_dut", Fase::Setup),
            ("medir_voltaje", Fase::Main),
            ("apagar_fuente", Fase::Cleanup),
        ] {
            let mut r = ResultadoStep::nuevo(nombre, "pass", "ok");
            r.fase = fase;
            s.registra(r);
        }
        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let out = String::from_utf8(sink.salida).unwrap();
        let lineas: Vec<&str> = out.split("\r\n").collect();
        assert!(lineas[1].ends_with(",setup,,"), "setup: {}", lineas[1]);
        assert!(lineas[2].ends_with(",main,,"), "main: {}", lineas[2]);
        assert!(lineas[3].ends_with(",cleanup,,"), "cleanup: {}", lineas[3]);
    }
    /// ADR-0020 + Regla 3 de ADR-0019: **la condición en la que se midió
    /// queda escrita**. Antes de esto, dos corridas de la misma secuencia con
    /// distinto canal producían ficheros idénticos.
    ///
    /// Visto en rojo devolviendo `String::new()` en `nombrados_a_csv`: las
    /// dos celdas salen vacías y el CSV vuelve a no distinguir las corridas.
    #[test]
    fn los_parametros_y_las_salidas_van_a_su_celda() {
        let mut p = ResultadoStep::medido_valor("medir", "pass", "ok", 4.4);
        p.parametros = vec![
            ("canal".into(), expr::Value::Numero(3.0)),
            ("etiqueta".into(), expr::Value::Texto("banco-3".into())),
        ];
        p.salidas = vec![("temperatura".into(), expr::Value::Numero(21.5))];
        let mut s = ResultadoSecuencia::nueva("c");
        s.registra(p);

        let mut sink = SinkCsv::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc = String::from_utf8(sink.salida).unwrap();
        let fila = doc.split("\r\n").nth(1).expect("una fila de paso");
        let celdas: Vec<&str> = fila.split(',').collect();
        assert_eq!(
            celdas[11], "canal=3;etiqueta=banco-3",
            "los parámetros enviados, con el número sin decimales"
        );
        assert_eq!(celdas[12], "temperatura=21.5");
    }

    /// Dos corridas con distinto canal ya **no** producen el mismo fichero.
    /// Es la frase del ADR convertida en test.
    #[test]
    fn dos_corridas_con_distinto_canal_ya_no_dan_el_mismo_csv() {
        let csv_de = |canal: f64| {
            let mut p = ResultadoStep::medido_valor("medir", "pass", "ok", 4.2);
            p.parametros = vec![("canal".into(), expr::Value::Numero(canal))];
            let mut s = ResultadoSecuencia::nueva("c");
            s.registra(p);
            let mut sink = SinkCsv::nuevo(Vec::new());
            sink.on_fin_secuencia(&s);
            String::from_utf8(sink.salida).unwrap()
        };
        assert_ne!(csv_de(1.0), csv_de(2.0));
    }
}
