//! Paso de ejemplo "hola mundo" (M5-ext.2, ADR-0015): un componente WASM
//! que exporta `run` (interfaz `anvil:step`). Es la referencia de la guía
//! "escribe un paso en Rust, compílalo a .wasm y ejecútalo con Anvil".
//!
//! El componente NO sabe de gRPC ni de protobuf: recibe el nombre del paso,
//! el número de intento y sus parámetros ya evaluados, y devuelve un
//! resultado. Quien habla gRPC con el motor es el puente
//! (`anvil-puente-wasm`), que carga este componente y llama a `run`.
//!
//! Tampoco sabe de **versiones de contrato** (ADR-0015): el eco que el motor
//! comprueba lo responde el puente por él. Lo que sí le afecta es que el WIT
//! está versionado y viaja pegado al artefacto: desde `anvil:step@0.2.0` la
//! firma de `run` lleva `parametros`, y **la regla es recompilar** — no hay
//! capa de compatibilidad (ADR-0020 §4d). Un `.wasm` construido contra la
//! 0.1.0 no instancia.
//!
//! Compilar:
//!   cargo component build --manifest-path ejemplos/hola-paso/Cargo.toml
//!   # → ejemplos/hola-paso/target/wasm32-wasip1/debug/hola_paso.wasm
//!
//! Requiere `cargo component` instalado:
//!   cargo install cargo-component --locked

#[allow(warnings)]
mod bindings;

use bindings::exports::anvil::step::step::{Guest, Named, StepResult, Value};

struct Component;

impl Guest for Component {
    fn run(nombre: String, intento: i32, parametros: Vec<Named>) -> StepResult {
        // Los parámetros llegan **ya evaluados** (ADR-0020): el motor resolvió
        // las expresiones `${...}` del YAML contra su entorno antes de llamar.
        // Aquí no hay expresiones que interpretar, sólo valores.
        let saludo = match parametros.iter().find(|p| p.name == "a_quien") {
            Some(Named {
                value: Value::Text(a),
                ..
            }) => format!("hola {a}"),
            // Un parámetro con otro tipo no se convierte a texto en silencio:
            // si la secuencia dice `a_quien: 3`, quien la escribió se ha
            // equivocado y conviene que lo vea.
            Some(otro) => {
                return StepResult {
                    status: "error".to_string(),
                    message: format!("'{}' tiene que ser texto", otro.name),
                    measured_value: None,
                    outputs: Vec::new(),
                }
            }
            None => format!("hola {nombre}"),
        };
        StepResult {
            status: "pass".to_string(),
            message: format!("{saludo} (intento {intento})"),
            measured_value: Some(4.2),
            // Una salida con nombre, para que el ejemplo enseñe también este
            // lado del contrato: la lee `assign` como
            // `result.outputs.greeted`.
            outputs: vec![Named {
                name: "greeted".to_string(),
                value: Value::Number(1.0),
            }],
        }
    }
}

bindings::export!(Component with_types_in bindings);
