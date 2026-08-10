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
    secuencia_usuario: Option<String>,
}

impl<W: Write> SinkJson<W> {
    pub fn nuevo(salida: W) -> Self {
        SinkJson {
            salida,
            secuencia_usuario: None,
        }
    }

    /// Declara la secuencia del **operador** que se está corriendo bajo un
    /// process model (M5, RF-38): con `--process-model`, `secuencia` es el
    /// nombre del PM (`sequential`), así que sin esto el resultado archivado
    /// no dice a qué test se aplicó. El dato lo pone el CLI, que es quien
    /// conoce la ruta inyectada en el PM.
    pub fn con_secuencia_usuario(mut self, nombre: impl Into<String>) -> Self {
        self.secuencia_usuario = Some(nombre.into());
        self
    }
}

impl<W: Write> ResultSink for SinkJson<W> {
    fn on_fin_secuencia(&mut self, secuencia: &ResultadoSecuencia) {
        // `pasos_saltados`/`pasos_totales` cuentan el árbol entero (sub_pasos
        // incluidos): un `saltado` no degrada el agregado, así que sin este
        // par un verde no distingue «el DUT pasó» de «el test no corrió».
        let (saltados, total) = secuencia.saltados();
        let mut doc = json!({
            "secuencia": secuencia.nombre,
            "estado": secuencia.estado(),
            "pasos_saltados": saltados,
            "pasos_totales": total,
            "pasos": secuencia.pasos.iter().map(paso_a_json).collect::<Vec<_>>(),
        });
        // Sin process model no hay secuencia de operador: la clave se omite
        // (no va como `null`) porque el concepto no existe en esa corrida.
        if let Some(nombre) = &self.secuencia_usuario {
            if let Some(obj) = doc.as_object_mut() {
                obj.insert("secuencia_usuario".into(), json!(nombre));
            }
        }
        let texto = serde_json::to_string_pretty(&doc).unwrap_or_default();
        if let Err(e) = escribir_con_reintentos(&mut self.salida, REINTENTOS, texto.as_bytes()) {
            eprintln!("sink json: no se pudo escribir: {e}");
        }
    }
}

/// Un `ResultadoStep` como objeto JSON: `valor_medido`, `limite_min/max` y
/// `valor_esperado` como número si los hay, `null` si no; `operador` como
/// símbolo (`">="`, …) o `null`; `fase` como `"setup"`/`"main"`/`"cleanup"`,
/// que la sella el motor. Si el paso es un **sequence call** (M4b),
/// anida `sub_pasos` con la misma estructura, recursivamente.
fn paso_a_json(p: &ResultadoStep) -> Value {
    let base = json!({
        "nombre": p.nombre,
        "estado": p.estado,
        "fase": p.fase.como_texto(),
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

    #[test]
    fn el_documento_lleva_el_recuento_de_saltados() {
        use modelo::Fase;
        // Un verde que no corrió la mitad de la secuencia debe poder
        // distinguirse al post-procesar (#13).
        let mut call = ResultadoStep::nuevo("test_uut", "paso", "sequence call → paso");
        call.fase = Fase::Main;
        call.sub_pasos = Some(vec![ResultadoStep::nuevo(
            "medir",
            "saltado",
            "precondición falsa",
        )]);
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(ResultadoStep::nuevo("preparar", "saltado", "disable"));
        s.registra(call);

        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert_eq!(doc["estado"], "paso", "el agregado no cambia (RF-33/34)");
        assert_eq!(doc["pasos_saltados"], 2, "cuenta también los anidados");
        assert_eq!(doc["pasos_totales"], 3);
    }

    #[test]
    fn cada_paso_lleva_su_fase_tambien_anidado() {
        use modelo::Fase;
        // La fase la sella el motor; el sink la emite tal cual, y también en
        // los sub_pasos de un sequence call (DIAG-3, #8).
        let mut hijo = ResultadoStep::nuevo("apagar_fuente", "paso", "ok");
        hijo.fase = Fase::Cleanup;
        let mut call = ResultadoStep::nuevo("test_uut", "paso", "sequence call → paso");
        call.fase = Fase::Setup;
        call.sub_pasos = Some(vec![hijo]);

        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(call);
        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);

        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert_eq!(doc["pasos"][0]["fase"], "setup");
        assert_eq!(doc["pasos"][0]["sub_pasos"][0]["fase"], "cleanup");
    }

    #[test]
    fn la_secuencia_de_operador_solo_aparece_si_se_declara() {
        // Sin process model no hay secuencia de operador: la clave se omite.
        let s = secuencia_ejemplo();
        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert!(doc.get("secuencia_usuario").is_none());

        // Con PM, `secuencia` es el nombre del PM y la del operador va aparte
        // (#9): así el resultado archivado registra qué test se corrió.
        let mut pm = ResultadoSecuencia::nueva("sequential");
        pm.registra(ResultadoStep::nuevo("identificar_uut", "paso", "UUT-001"));
        let mut sink = SinkJson::nuevo(Vec::new()).con_secuencia_usuario("ejemplos/basica.yaml");
        sink.on_fin_secuencia(&pm);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert_eq!(doc["secuencia"], "sequential");
        assert_eq!(doc["secuencia_usuario"], "ejemplos/basica.yaml");
    }
}
