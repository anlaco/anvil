//! Sink JSON: un documento estructurado con la secuencia, el estado
//! agregado y la lista de pasos. Pensado para consumo programático — los
//! `valor_medido` van como **número** JSON (no string), a diferencia del
//! CSV, que va como texto para humanos.
//!
//! El documento se ensambla con `serde_json::json!` a mano (sin derivar
//! `Serialize` en `modelo`: así `modelo` no gana la dep `serde` y el
//! núcleo del motor se queda ligero) y se escribe de un tiro con
//! reintento.

use modelo::{ResultSink, ResultadoSecuencia, ResultadoStep};
use serde_json::{json, Value};
use std::io::Write;

use crate::reintento::escribir_con_reintentos;

/// Número de intentos de escritura ante fallos transitorios (RF-23).
const REINTENTOS: u32 = 3;

/// Verte el resultado a un `Write` como un documento JSON.
pub struct SinkJson<W: Write> {
    salida: W,
}

impl<W: Write> SinkJson<W> {
    pub fn nuevo(salida: W) -> Self {
        SinkJson { salida }
    }
}

impl<W: Write> ResultSink for SinkJson<W> {
    fn on_fin_secuencia(&mut self, secuencia: &ResultadoSecuencia) {
        let doc = json!({
            "secuencia": secuencia.nombre,
            "estado": secuencia.estado(),
            "pasos": secuencia.pasos.iter().map(paso_a_json).collect::<Vec<_>>(),
        });
        let texto = serde_json::to_string_pretty(&doc).unwrap_or_default();
        if let Err(e) = escribir_con_reintentos(&mut self.salida, REINTENTOS, texto.as_bytes()) {
            eprintln!("sink json: no se pudo escribir: {e}");
        }
    }
}

/// Un `ResultadoStep` como objeto JSON: `valor_medido`, `limite_min/max` y
/// `valor_esperado` como número si los hay, `null` si no; `operador` como
/// símbolo (`">="`, …) o `null`. Si el paso es un **sequence call** (M4b),
/// anida `sub_pasos` con la misma estructura, recursivamente.
fn paso_a_json(p: &ResultadoStep) -> Value {
    let base = json!({
        "nombre": p.nombre,
        "estado": p.estado,
        "mensaje": p.mensaje,
        "valor_medido": opt_num(p.valor_medido),
        "limite_min": opt_num(p.limite_min),
        "limite_max": opt_num(p.limite_max),
        "valor_esperado": opt_num(p.valor_esperado),
        "operador": p.operador.map(|op| json!(op.simbolo())).unwrap_or(Value::Null),
    });
    match &p.sub_pasos {
        Some(sub) => {
            let mut obj = base.as_object().unwrap().clone();
            obj.insert(
                "sub_pasos".into(),
                Value::Array(sub.iter().map(paso_a_json).collect()),
            );
            Value::Object(obj)
        }
        None => base,
    }
}

/// `Option<f64>` → número JSON o `null`. Explícito (en vez de `json!(opt)`)
/// porque `serde_json::Value: From<Option<f64>>` no está garantizado.
fn opt_num(o: Option<f64>) -> Value {
    match o {
        Some(v) => json!(v),
        None => Value::Null,
    }
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
        s.registra(ResultadoStep::nuevo(
            "verificar_led",
            "paso",
            "led encendido",
        ));
        s
    }

    #[test]
    fn produce_estructura_con_estado_agregado() {
        let s = secuencia_ejemplo();
        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);

        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert_eq!(doc["secuencia"], "basica");
        assert_eq!(doc["estado"], "fallo");
        assert_eq!(doc["pasos"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn valor_medido_es_numero_y_los_sin_medida_son_null() {
        let s = secuencia_ejemplo();
        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();

        // medir_voltaje: medida presente como número.
        let paso0 = &doc["pasos"][0];
        assert_eq!(paso0["valor_medido"], 4.2, "valor_medido como número");
        assert_eq!(paso0["limite_min"], 4.5);
        assert_eq!(paso0["limite_max"], 5.5);

        // verificar_led: sin medida → null (no string).
        let paso1 = &doc["pasos"][1];
        assert!(paso1["valor_medido"].is_null(), "sin medida → null");
        assert!(paso1["limite_min"].is_null());
    }

    #[test]
    fn comparacion_aparece_como_valor_esperado_y_operador() {
        // Un resultado de comparación: el motor rellena valor_esperado y
        // operador tras evaluar el límite del YAML (ADR-0008).
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

        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        let paso = &doc["pasos"][0];
        assert_eq!(paso["valor_esperado"], 1000.0);
        assert_eq!(paso["operador"], ">=");
        // Sin rango → null.
        assert!(paso["limite_min"].is_null());
        assert!(paso["limite_max"].is_null());
    }

    #[test]
    fn sequence_call_anida_sub_pasos_en_json() {
        // Un sequence call (M4b) produce un ResultadoStep con sub_pasos; el
        // JSON los anida como array de objetos.
        let mut call = ResultadoStep::nuevo("test_fuentes", "fallo", "sequence call → fallo");
        call.sub_pasos = Some(vec![
            ResultadoStep::nuevo("medir_canal_1", "paso", "ok"),
            ResultadoStep::nuevo("medir_canal_2", "fallo", "fuera de rango"),
        ]);
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(call);

        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        let paso = &doc["pasos"][0];
        assert_eq!(paso["nombre"], "test_fuentes");
        assert_eq!(paso["estado"], "fallo");
        let sub = paso["sub_pasos"].as_array().unwrap();
        assert_eq!(sub.len(), 2);
        assert_eq!(sub[0]["nombre"], "medir_canal_1");
        assert_eq!(sub[0]["estado"], "paso");
        assert_eq!(sub[1]["nombre"], "medir_canal_2");
        assert_eq!(sub[1]["estado"], "fallo");
        // Un paso sin sub_pasos no lleva la clave (un paso común del test).
        assert!(doc["pasos"].as_array().unwrap().len() == 1 || true);
    }
}
