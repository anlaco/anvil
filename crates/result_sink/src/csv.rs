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
    "sequence_name",
    "status",
    "step_name",
    "step_status",
    "message",
    "measured_value",
    "limit_min",
    "limit_max",
    "expected_value",
    "operator",
    "phase",
    // ADR-0020, al final por el mismo motivo que `fase`. Un CSV no puede
    // tener columnas distintas por fila, así que los valores con nombre van
    // compactados en una sola celda (ver `nombrados_a_csv`) en vez de una
    // columna por parámetro — que es lo que rompería el formato en cuanto dos
    // pasos declararan parámetros distintos.
    "inputs",
    "outputs",
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
                expr::Value::Reference(r) => referencia_a_token(r),
                expr::Value::Nulo => String::new(),
            };
            format!("{n}={valor}")
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// A reference in one CSV cell: `ref:<executor>/<lifetime>/<payload>`, each
/// part percent-encoded (ADR-0022 left the form open).
///
/// **The encoding is the point, not the shape.** This cell packs several pairs
/// as `name=value` joined by `;`, and [`csv_campo`] escapes only what RFC-4180
/// asks of it — comma, quote, CR, LF. A payload carrying a `;` or an `=` would
/// therefore split the cell into pairs that were never there, and it would do
/// it **in silence**: the file still parses, and the row reads as if the bench
/// had been something else. A payload is minted by the executor and opaque to
/// Anvil, so there is no character it can be promised not to contain.
///
/// So the three parts are percent-encoded and the result carries no character
/// that means anything to this format. `ref:` marks it as a reference rather
/// than a text that happens to look like a path.
///
/// The same corruption is still reachable through a **text** output holding a
/// `;` or an `=`, and this does not fix that: changing how texts are written
/// would change files already being read (see issue on the CSV separator).
fn referencia_a_token(r: &expr::Reference) -> String {
    format!(
        "ref:{}/{}/{}",
        pct(modelo::nombre_visible_de_ejecutor(&r.executor)),
        pct(&r.lifetime),
        pct(&r.payload)
    )
}

/// Percent-encodes everything this format gives a meaning to, plus `%` itself
/// so the encoding is reversible, plus the control characters.
///
/// Deliberately not "everything but alphanumerics": a reference is meant to be
/// read off a row by a person, and encoding a hyphen would make a plain
/// `rack-1` unreadable for no gain.
fn pct(s: &str) -> String {
    let mut fuera = String::with_capacity(s.len());
    for c in s.chars() {
        // `%` first: it is the escape character and must be encoded, or the
        // decoding is ambiguous. `;` and `=` split the cell, `/` separates the
        // three parts, and `,` `"` CR LF are the CSV field's own.
        if matches!(c, '%' | ';' | '=' | '/' | ',' | '"') || c.is_control() {
            for b in c.to_string().bytes() {
                fuera.push_str(&format!("%{b:02X}"));
            }
        } else {
            fuera.push(c);
        }
    }
    fuera
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
        assert_eq!(lineas[0], "sequence_name,status,step_name,step_status,message,measured_value,limit_min,limit_max,expected_value,operator,phase,inputs,outputs");
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
            "sequence_name,status,step_name,step_status,message,measured_value,limit_min,limit_max,expected_value,operator,phase,inputs,outputs"
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

    /// **Criterio 5 del encargo.** Una carga opaca con `;` y `=` dentro no
    /// corrompe la celda (ADR-0022, §Consecuencias: *«que no se corrompa nada
    /// es requisito»*).
    ///
    /// La celda empaqueta varios pares `nombre=valor` unidos por `;`, y
    /// `csv_campo` sólo escapa lo que le pide RFC-4180 —coma, comilla, CR,
    /// LF—. Un payload con un `;` partiría la celda en pares que nunca
    /// existieron, **y en silencio**: el fichero sigue siendo CSV válido y la
    /// fila se lee como si el banco hubiera sido otro. Y no hay carácter que
    /// se le pueda prohibir a un payload: lo acuña el ejecutor y es opaco.
    ///
    /// Visto fallar sustituyendo `pct` por la identidad: la celda pasa a
    /// tener cuatro pares y a decir que el canal es `2` cuando nadie lo mandó.
    #[test]
    fn una_carga_opaca_con_punto_y_coma_no_corrompe_la_celda() {
        let referencia = expr::Value::Reference(expr::Reference {
            executor: "python".into(),
            lifetime: "v1".into(),
            payload: "rack;canal=2".into(),
        });
        let celda = nombrados_a_csv(&[
            ("rack".into(), referencia),
            ("canal".into(), expr::Value::Numero(7.0)),
        ]);
        // Dos pares y no cuatro: el `;` y el `=` del payload no separan nada.
        let pares: Vec<&str> = celda.split(';').collect();
        assert_eq!(pares.len(), 2, "la celda se partió: {celda}");
        assert_eq!(pares[1], "canal=7");
        assert!(
            !pares[0].trim_start_matches("rack=").contains('='),
            "el payload no puede traer un '=' suelto: {celda}"
        );
        // Y sigue siendo reversible: el payload original se recupera.
        let payload = pares[0].rsplit('/').next().unwrap();
        assert_eq!(descodifica(payload), "rack;canal=2");
    }

    /// La codificación es reversible, que es lo que la separa de «borrar los
    /// caracteres molestos»: quien lea el informe tiene que poder recuperar el
    /// identificador con el que se midió.
    #[test]
    fn la_codificacion_de_una_referencia_es_reversible() {
        for crudo in ["s1", "a/b", "100%", "x=1;y=2", "con,coma", "con\"comilla"] {
            assert_eq!(descodifica(&pct(crudo)), crudo, "roundtrip de {crudo:?}");
        }
        assert_eq!(pct("rack-1"), "rack-1", "lo legible se queda legible");
    }

    /// El decodificador del test, a propósito escrito aquí y no en el sink:
    /// Anvil **escribe** referencias y no las lee de vuelta, así que tener un
    /// decodificador en producción sería tener código que nadie ejerce.
    fn descodifica(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut fuera: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                fuera.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            } else {
                fuera.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(fuera).unwrap()
    }
}
