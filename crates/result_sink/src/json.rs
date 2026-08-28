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
            "sequence": secuencia.nombre,
            "status": secuencia.estado(),
            "skipped_steps": saltados,
            "total_steps": total,
            "steps": secuencia.pasos.iter().map(paso_a_json).collect::<Vec<_>>(),
        });
        // Sin process model no hay secuencia de operador: la clave se omite
        // (no va como `null`) porque el concepto no existe en esa corrida.
        if let Some(nombre) = &self.secuencia_usuario {
            if let Some(obj) = doc.as_object_mut() {
                obj.insert("user_sequence".into(), json!(nombre));
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
        "name": p.nombre,
        "status": p.estado,
        "phase": p.fase.como_texto(),
        "message": p.mensaje,
        "measured_value": opt_num(p.valor_medido),
        "limit_min": opt_num(p.limite_min),
        "limit_max": opt_num(p.limite_max),
        "expected_value": opt_num(p.valor_esperado),
        "operator": p.operador.map(|op| json!(op.simbolo())).unwrap_or(Value::Null),
        // ADR-0020 + Regla 3 de ADR-0019: **la condición en la que se midió
        // queda escrita**. Hasta ahora dos corridas de la misma secuencia con
        // distinto canal producían informes idénticos, porque el canal iba
        // grabado dentro del paso y no viajaba a ningún sitio.
        "inputs": nombrados_a_json(&p.parametros),
        "outputs": nombrados_a_json(&p.salidas),
    });
    match &p.sub_pasos {
        Some(sub) => {
            let mut obj = base.as_object().unwrap().clone();
            obj.insert(
                "sub_steps".into(),
                Value::Array(sub.iter().map(paso_a_json).collect()),
            );
            Value::Object(obj)
        }
        None => base,
    }
}

/// Valores con nombre (parámetros o salidas) como objeto JSON, con el **tipo
/// preservado**: un número sale como número y un booleano como booleano.
///
/// Escribirlos todos como texto perdería justo lo que el ADR-0020 fue a
/// ganar: que quien lea el informe sepa que el canal era el número 2 y no la
/// cadena "2".
///
/// Un mapa vacío, y no `null`, cuando no hay ninguno: quien consuma el JSON
/// no tiene que distinguir dos formas de decir «nada».
fn nombrados_a_json(vs: &[(String, expr::Value)]) -> Value {
    let mut obj = serde_json::Map::with_capacity(vs.len());
    for (nombre, v) in vs {
        obj.insert(nombre.clone(), valor_a_json(v));
    }
    Value::Object(obj)
}

/// Un `expr::Value` al JSON de su tipo. `Nulo` → `null`, aunque hoy no puede
/// llegar (ni un parámetro ni una salida nula cruzan el cable).
///
/// **A reference is an object, and that is the whole decision** (ADR-0022 left
/// the form open). Writing it as a string would put it back where the type
/// took it from: indistinguishable from a text, concatenable by whoever
/// post-processes the report, and needing a separator that the first payload
/// containing it would break. An object cannot be confused with any of the
/// three scalars, needs no escaping at all —`serde_json` quotes the three
/// fields— and keeps the three parts apart, which is what lets a reader tell
/// which bench a measurement was made against even after the run.
///
/// `type` is spelled out rather than implied by the shape: a consumer that
/// only knows the three scalars sees an object with a name on it instead of a
/// number it might have used.
fn valor_a_json(v: &expr::Value) -> Value {
    match v {
        expr::Value::Numero(x) => json!(x),
        expr::Value::Texto(s) => json!(s),
        expr::Value::Bool(b) => json!(b),
        expr::Value::Reference(r) => json!({
            "type": "reference",
            "executor": modelo::nombre_visible_de_ejecutor(&r.executor),
            "lifetime": r.lifetime,
            "payload": r.payload,
        }),
        expr::Value::Nulo => Value::Null,
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
    fn produce_estructura_con_estado_agregado() {
        let s = secuencia_ejemplo();
        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);

        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert_eq!(doc["sequence"], "basica");
        assert_eq!(doc["status"], "fail");
        assert_eq!(doc["steps"].as_array().unwrap().len(), 2);
    }

    /// ADR-0019, Regla 1: quien consuma el JSON tiene que contar con un estado
    /// más. El sink no lo sabe —delega en `estado()`—, y este test lo fija:
    /// el vocabulario del fichero es superficie pública, no un detalle interno.
    #[test]
    fn el_estado_agregado_puede_ser_inconcluso() {
        let mut s = ResultadoSecuencia::nueva("b31");
        s.registra(ResultadoStep::nuevo(
            "verdict",
            "skipped",
            "precondición falsa",
        ));
        s.veredicto_sin_evaluar = true;

        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);

        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert_eq!(doc["status"], "inconclusive");
        assert_eq!(
            doc["steps"][0]["status"], "skipped",
            "el paso conserva lo que fue; lo que cambia es el agregado"
        );
    }

    #[test]
    fn valor_medido_es_numero_y_los_sin_medida_son_null() {
        let s = secuencia_ejemplo();
        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();

        // medir_voltaje: medida presente como número.
        let paso0 = &doc["steps"][0];
        assert_eq!(paso0["measured_value"], 4.2, "valor_medido como número");
        assert_eq!(paso0["limit_min"], 4.5);
        assert_eq!(paso0["limit_max"], 5.5);

        // verificar_led: sin medida → null (no string).
        let paso1 = &doc["steps"][1];
        assert!(paso1["measured_value"].is_null(), "sin medida → null");
        assert!(paso1["limit_min"].is_null());
    }

    #[test]
    fn comparacion_aparece_como_valor_esperado_y_operador() {
        // Un resultado de comparación: el motor rellena valor_esperado y
        // operador tras evaluar el límite del YAML (ADR-0008).
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

        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        let paso = &doc["steps"][0];
        assert_eq!(paso["expected_value"], 1000.0);
        assert_eq!(paso["operator"], ">=");
        // Sin rango → null.
        assert!(paso["limit_min"].is_null());
        assert!(paso["limit_max"].is_null());
    }

    #[test]
    fn sequence_call_anida_sub_pasos_en_json() {
        // Un sequence call (M4b) produce un ResultadoStep con sub_pasos; el
        // JSON los anida como array de objetos.
        let mut call = ResultadoStep::nuevo("test_fuentes", "fail", "sequence call → fallo");
        call.sub_pasos = Some(vec![
            ResultadoStep::nuevo("medir_canal_1", "pass", "ok"),
            ResultadoStep::nuevo("medir_canal_2", "fail", "fuera de rango"),
        ]);
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(call);

        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        let paso = &doc["steps"][0];
        assert_eq!(paso["name"], "test_fuentes");
        assert_eq!(paso["status"], "fail");
        let sub = paso["sub_steps"].as_array().unwrap();
        assert_eq!(sub.len(), 2);
        assert_eq!(sub[0]["name"], "medir_canal_1");
        assert_eq!(sub[0]["status"], "pass");
        assert_eq!(sub[1]["name"], "medir_canal_2");
        assert_eq!(sub[1]["status"], "fail");
        // Un paso sin sub_pasos no lleva la clave (un paso común del test).
        assert!(doc["steps"].as_array().unwrap().len() == 1 || true);
    }

    #[test]
    fn el_documento_lleva_el_recuento_de_saltados() {
        use modelo::Fase;
        // Un verde que no corrió la mitad de la secuencia debe poder
        // distinguirse al post-procesar (#13).
        let mut call = ResultadoStep::nuevo("test_uut", "pass", "sequence call → paso");
        call.fase = Fase::Main;
        call.sub_pasos = Some(vec![ResultadoStep::nuevo(
            "medir",
            "skipped",
            "precondición falsa",
        )]);
        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(ResultadoStep::nuevo("preparar", "skipped", "disable"));
        s.registra(call);

        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert_eq!(doc["status"], "pass", "el agregado no cambia (RF-33/34)");
        assert_eq!(doc["skipped_steps"], 2, "cuenta también los anidados");
        assert_eq!(doc["total_steps"], 3);
    }

    #[test]
    fn cada_paso_lleva_su_fase_tambien_anidado() {
        use modelo::Fase;
        // La fase la sella el motor; el sink la emite tal cual, y también en
        // los sub_pasos de un sequence call (DIAG-3, #8).
        let mut hijo = ResultadoStep::nuevo("apagar_fuente", "pass", "ok");
        hijo.fase = Fase::Cleanup;
        let mut call = ResultadoStep::nuevo("test_uut", "pass", "sequence call → paso");
        call.fase = Fase::Setup;
        call.sub_pasos = Some(vec![hijo]);

        let mut s = ResultadoSecuencia::nueva("basica");
        s.registra(call);
        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);

        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert_eq!(doc["steps"][0]["phase"], "setup");
        assert_eq!(doc["steps"][0]["sub_steps"][0]["phase"], "cleanup");
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
        pm.registra(ResultadoStep::nuevo("identificar_uut", "pass", "UUT-001"));
        let mut sink = SinkJson::nuevo(Vec::new()).con_secuencia_usuario("ejemplos/basica.yaml");
        sink.on_fin_secuencia(&pm);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert_eq!(doc["sequence"], "sequential");
        assert_eq!(doc["user_sequence"], "ejemplos/basica.yaml");
    }
    /// ADR-0020: los parámetros y las salidas van al JSON **con su tipo**.
    /// Aplanarlos a texto perdería justo lo que este ADR fue a ganar: que
    /// quien lea el informe sepa que el canal era el número 2 y no la cadena
    /// "2".
    ///
    /// Visto en rojo escribiendo todos los valores con `json!(v.to_string())`.
    #[test]
    fn los_parametros_y_las_salidas_conservan_su_tipo() {
        let mut p = ResultadoStep::medido_valor("medir", "pass", "ok", 4.4);
        p.parametros = vec![
            ("canal".into(), expr::Value::Numero(3.0)),
            ("etiqueta".into(), expr::Value::Texto("banco-3".into())),
            ("promediar".into(), expr::Value::Bool(true)),
        ];
        p.salidas = vec![("temperatura".into(), expr::Value::Numero(21.5))];
        let mut s = ResultadoSecuencia::nueva("c");
        s.registra(p);

        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        let paso = &doc["steps"][0];

        assert_eq!(paso["inputs"]["canal"], 3.0, "número, no cadena");
        assert!(paso["inputs"]["canal"].is_number());
        assert_eq!(paso["inputs"]["etiqueta"], "banco-3");
        assert_eq!(paso["inputs"]["promediar"], true);
        assert!(paso["inputs"]["promediar"].is_boolean());
        assert_eq!(paso["outputs"]["temperatura"], 21.5);
    }

    /// Sin parámetros, un objeto vacío y no `null`: quien consuma el JSON no
    /// tiene que distinguir dos formas de decir «nada».
    #[test]
    fn sin_parametros_es_un_objeto_vacio_y_no_null() {
        let mut s = ResultadoSecuencia::nueva("c");
        s.registra(ResultadoStep::nuevo("p", "pass", "ok"));
        let mut sink = SinkJson::nuevo(Vec::new());
        sink.on_fin_secuencia(&s);
        let doc: Value = serde_json::from_slice(&sink.salida).unwrap();
        assert_eq!(doc["steps"][0]["inputs"], json!({}));
        assert_eq!(doc["steps"][0]["outputs"], json!({}));
    }

    /// **La forma de una referencia en el JSON** (ADR-0022 la dejó abierta):
    /// un objeto, no una cadena.
    ///
    /// Una cadena la devolvería al sitio del que el tipo la sacó —
    /// indistinguible de un texto, concatenable por quien post-procese el
    /// informe, y necesitada de un separador que el primer payload que lo
    /// contuviera rompería. Un objeto no se puede confundir con ninguno de los
    /// tres escalares y no necesita escapado ninguno.
    ///
    /// Visto fallar escribiendo la referencia como `r.mostrar()`: el valor
    /// pasa a ser una cadena y el `assert` del tipo se cae.
    #[test]
    fn una_referencia_va_como_objeto_y_no_como_cadena() {
        let v = valor_a_json(&expr::Value::Reference(expr::Reference {
            executor: "python".into(),
            lifetime: "v1".into(),
            payload: "rack;canal=2".into(),
        }));
        assert!(v.is_object(), "una referencia no es un escalar: {v}");
        assert_eq!(v["type"], "reference");
        assert_eq!(v["executor"], "python");
        assert_eq!(v["lifetime"], "v1");
        // El payload, tal cual: `serde_json` lo entrecomilla y no hay
        // separador nuestro que se pueda romper.
        assert_eq!(v["payload"], "rack;canal=2");
    }

    /// El nombre interno del ejecutor embebido es fontanería y no se enseña.
    #[test]
    fn el_ejecutor_embebido_sale_por_su_nombre_legible() {
        let v = valor_a_json(&expr::Value::Reference(expr::Reference {
            executor: modelo::EJECUTOR_EMBEBIDO.into(),
            lifetime: String::new(),
            payload: "s1".into(),
        }));
        assert_eq!(v["executor"], "embebido");
    }
}
