//! The catalog, out as data (ADR-0044).
//!
//! `Describe` has been answered since ADR-0021 and read in exactly one place:
//! `comprueba_firmas`, which compares it against a sequence someone already
//! wrote and then throws it away. This is the other reader — the one ADR-0025
//! §4 called *the* enumerate *operation, a tooling operation for an editor*,
//! and which until now existed as three `--list` flags printing prose that
//! nothing consumes.
//!
//! Two rules shape what comes out, and both are the same rule:
//!
//! - **A declared executor always has an entry.** One that does not describe
//!   itself says so, with the reason. Leaving it out would read as "serves no
//!   steps", which is a different statement and a false one — the distinction
//!   ADR-0028 was written to protect and the false green of ADR-0019 Rule 2.
//! - **A type crosses as its name.** `number`, `text`, `boolean`, `reference`,
//!   `unspecified` — the vocabulary the YAML and the report already use, not
//!   the enum's integers. Whoever reads this should never have to know the
//!   wire encoding, and `unspecified` has to keep reading as *unchecked*
//!   rather than as a key somebody forgot to fill in.
//!
//! Assembled by hand with `serde_json::json!`, like `result_sink::json`, for
//! the reason written there: `modelo` carries no `serde` derives on purpose.

use modelo::proto::{value::Dato, Catalog, ParameterSpec, StepSpec, Value, ValueType};
use serde_json::{json, Map, Value as Json};

use crate::catalogo::{Catalogos, Descripcion};

/// The shape of this document.
///
/// It is a public surface the moment anything parses it, so it carries a
/// version of its own the way the event stream does (ADR-0029 §5). It moves
/// only when the **meaning** of a key changes; adding keys is compatible and
/// does not touch it. It is not `contract`, which is the wire's, and not the
/// engine's version, which moves for other reasons.
pub const DESCRIBE_VERSION: u32 = 1;

/// The name a `ValueType` travels under.
///
/// `Unspecified` is proto3's default, so it is also what an executor that said
/// nothing about a parameter produces. It is named rather than omitted: the
/// reader has to be able to tell *"this is unchecked"* from *"this key is
/// missing"*.
fn tipo(v: ValueType) -> &'static str {
    match v {
        ValueType::Unspecified => "unspecified",
        ValueType::Number => "number",
        ValueType::Text => "text",
        ValueType::Boolean => "boolean",
        ValueType::Reference => "reference",
    }
}

/// A `Value` as JSON, for a parameter's declared default.
///
/// A reference cannot be a default — it names a live object in a process that
/// has not started yet — so it is rendered as null with its own key rather
/// than pretending to be a value someone could write in a sequence.
fn valor(v: &Value) -> Json {
    match &v.dato {
        Some(Dato::Numero(n)) => json!(n),
        Some(Dato::Texto(t)) => json!(t),
        Some(Dato::Booleano(b)) => json!(b),
        Some(Dato::Reference(_)) => Json::Null,
        None => Json::Null,
    }
}

fn parametro(p: &ParameterSpec) -> Json {
    let mut m = Map::new();
    m.insert("name".into(), json!(p.name));
    m.insert("type".into(), json!(tipo(p.value_type())));
    m.insert("required".into(), json!(p.required));
    m.insert("doc".into(), json!(p.doc));
    // ADR-0021 §5: informative, and the step applies it — the engine sends
    // nothing. `has_default` says whether one was declared, because a declared
    // default of `null` and no default at all are different things.
    m.insert("has_default".into(), json!(p.default.is_some()));
    m.insert(
        "default".into(),
        p.default.as_ref().map(valor).unwrap_or(Json::Null),
    );
    Json::Object(m)
}

fn paso(s: &StepSpec) -> Json {
    json!({
        "name": s.name,
        "doc": s.doc,
        "inputs": s.inputs.iter().map(parametro).collect::<Vec<_>>(),
        "outputs": s.outputs.iter().map(|o| json!({
            "name": o.name,
            "type": tipo(o.value_type()),
            "doc": o.doc,
        })).collect::<Vec<_>>(),
    })
}

fn descrito(c: &Catalog) -> Json {
    json!({
        "describes": true,
        // ADR-0022 §6: empty is legitimate — a component cannot hold objects,
        // so it mints no life. Empty and absent are the same here, and the
        // reader is told which by the string being empty rather than the key
        // being gone.
        "lifetime": c.lifetime,
        "contract": c.contract,
        "steps": c.steps.iter().map(paso).collect::<Vec<_>>(),
    })
}

/// The whole document: every declared executor, described or not.
pub fn catalogos_a_json(catalogos: &Catalogos) -> Json {
    let mut ejecutores = Map::new();
    for (nombre, d) in catalogos {
        ejecutores.insert(
            nombre.clone(),
            match d {
                Descripcion::Describe(c) => descrito(c),
                // The reason travels. "It does not answer Describe" and "there
                // is no open connection" send whoever reads this to two
                // different places, and collapsing them to `false` would send
                // them to neither.
                Descripcion::NoDescribe(motivo) => json!({
                    "describes": false,
                    "reason": motivo,
                    "steps": [],
                }),
            },
        );
    }
    json!({
        "describe_version": DESCRIBE_VERSION,
        "executors": Json::Object(ejecutores),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use modelo::proto::OutputSpec;

    fn spec(name: &str) -> StepSpec {
        StepSpec {
            name: name.into(),
            inputs: vec![],
            outputs: vec![],
            doc: String::new(),
        }
    }

    #[test]
    fn un_ejecutor_que_describe_saca_sus_pasos_con_tipos_por_nombre() {
        let mut s = spec("multimetro/medir");
        s.doc = "Mide tensión".into();
        s.inputs = vec![ParameterSpec::required("canal", ValueType::Number)];
        s.outputs = vec![OutputSpec::nueva("tension", ValueType::Number, "V")];
        let catalogos: Catalogos = [(
            "demo".to_string(),
            Descripcion::Describe(Catalog::descrito(vec![s])),
        )]
        .into();

        let j = catalogos_a_json(&catalogos);
        let paso = &j["executors"]["demo"]["steps"][0];
        assert_eq!(paso["name"], "multimetro/medir");
        assert_eq!(paso["doc"], "Mide tensión");
        // El tipo sale por su nombre, no por el número del enum: quien lea
        // esto no tiene por qué conocer la codificación del cable.
        assert_eq!(paso["inputs"][0]["type"], "number");
        assert_eq!(paso["inputs"][0]["required"], true);
        assert_eq!(paso["outputs"][0]["type"], "number");
        assert_eq!(j["describe_version"], DESCRIBE_VERSION);
    }

    #[test]
    fn un_ejecutor_que_declina_sale_igual_y_dice_por_que() {
        // Omitirlo se leería como «no sirve pasos», que es otra afirmación y
        // además falsa: el falso verde de ADR-0019 regla 2, y justo la
        // distinción que ADR-0028 existe para proteger.
        let catalogos: Catalogos = [(
            "viejo".to_string(),
            Descripcion::NoDescribe("no contesta a Describe".into()),
        )]
        .into();

        let j = catalogos_a_json(&catalogos);
        let e = &j["executors"]["viejo"];
        assert_eq!(e["describes"], false);
        assert_eq!(e["reason"], "no contesta a Describe");
        assert!(e["steps"].as_array().unwrap().is_empty());
    }

    #[test]
    fn un_catalogo_vacio_no_es_lo_mismo_que_no_describirse() {
        // «No sirvo nada» es una afirmación; «no me describo» es silencio.
        let catalogos: Catalogos = [
            (
                "vacio".to_string(),
                Descripcion::Describe(Catalog::descrito(vec![])),
            ),
            (
                "mudo".to_string(),
                Descripcion::NoDescribe("no se le preguntó".into()),
            ),
        ]
        .into();

        let j = catalogos_a_json(&catalogos);
        assert_eq!(j["executors"]["vacio"]["describes"], true);
        assert_eq!(j["executors"]["mudo"]["describes"], false);
        assert!(j["executors"]["vacio"]["reason"].is_null());
    }

    #[test]
    fn un_parametro_opcional_distingue_sin_default_de_default_nulo() {
        let mut s = spec("p");
        // `optional` siempre trae default; «sin default» es un `required`
        // que el ejecutor no rellenó, o un opcional cuyo default no cruzó.
        let mut a = ParameterSpec::optional("a", ValueType::Text, expr::Value::Texto("y".into()));
        a.default = None;
        s.inputs = vec![
            a,
            ParameterSpec::optional("b", ValueType::Text, expr::Value::Texto("x".into())),
        ];
        let catalogos: Catalogos = [(
            "e".to_string(),
            Descripcion::Describe(Catalog::descrito(vec![s])),
        )]
        .into();

        let j = catalogos_a_json(&catalogos);
        let inputs = &j["executors"]["e"]["steps"][0]["inputs"];
        assert_eq!(inputs[0]["required"], false);
        assert_eq!(inputs[0]["has_default"], false);
        assert!(inputs[0]["default"].is_null());
        assert_eq!(inputs[1]["has_default"], true);
        assert_eq!(inputs[1]["default"], "x");
    }

    #[test]
    fn un_tipo_que_el_ejecutor_no_dijo_sale_como_unspecified() {
        // Proto3 hace `Unspecified` el default, así que es también lo que
        // produce un ejecutor que no dijo nada del parámetro. Se nombra en vez
        // de omitirse: «sin comprobar» y «falta la clave» son distintos.
        let mut s = spec("p");
        s.inputs = vec![ParameterSpec::required("x", ValueType::Unspecified)];
        let catalogos: Catalogos = [(
            "e".to_string(),
            Descripcion::Describe(Catalog::descrito(vec![s])),
        )]
        .into();

        let j = catalogos_a_json(&catalogos);
        assert_eq!(
            j["executors"]["e"]["steps"][0]["inputs"][0]["type"],
            "unspecified"
        );
    }
}
