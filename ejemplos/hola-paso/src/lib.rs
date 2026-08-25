//! Paso de ejemplo "hola mundo" (M5-ext.2, ADR-0015): un componente WASM
//! que exporta `run` (interfaz `anvil:paso`). Es la referencia de la guía
//! "escribe un paso en Rust, compílalo a .wasm y ejecútalo con Anvil".
//!
//! El componente NO sabe de gRPC ni de protobuf: recibe el nombre del paso,
//! el número de intento y sus parámetros ya evaluados, y devuelve un
//! resultado. Quien habla gRPC con el motor es el puente
//! (`anvil-puente-wasm`), que carga este componente y llama a `run`.
//!
//! Tampoco sabe de **versiones de contrato** (ADR-0015): el eco que el motor
//! comprueba lo responde el puente por él. Lo que sí le afecta es que el WIT
//! está versionado y viaja pegado al artefacto: desde `anvil:paso@0.2.0` la
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

use bindings::exports::anvil::paso::paso::{Guest, Nombrado, Resultado, Valor};

struct Component;

impl Guest for Component {
    fn run(nombre: String, intento: i32, parametros: Vec<Nombrado>) -> Resultado {
        // Los parámetros llegan **ya evaluados** (ADR-0020): el motor resolvió
        // las expresiones `${...}` del YAML contra su entorno antes de llamar.
        // Aquí no hay expresiones que interpretar, sólo valores.
        let saludo = match parametros.iter().find(|p| p.nombre == "a_quien") {
            Some(Nombrado {
                valor: Valor::Texto(a),
                ..
            }) => format!("hola {a}"),
            // Un parámetro con otro tipo no se convierte a texto en silencio:
            // si la secuencia dice `a_quien: 3`, quien la escribió se ha
            // equivocado y conviene que lo vea.
            Some(otro) => {
                return Resultado {
                    estado: "error".to_string(),
                    mensaje: format!("'{}' tiene que ser texto", otro.nombre),
                    valor_medido: None,
                    salidas: Vec::new(),
                }
            }
            None => format!("hola {nombre}"),
        };
        Resultado {
            estado: "paso".to_string(),
            mensaje: format!("{saludo} (intento {intento})"),
            valor_medido: Some(4.2),
            // Una salida con nombre, para que el ejemplo enseñe también este
            // lado del contrato: la lee `asigna` como
            // `resultado.salidas.saludados`.
            salidas: vec![Nombrado {
                nombre: "saludados".to_string(),
                valor: Valor::Numero(1.0),
            }],
        }
    }
}

bindings::export!(Component with_types_in bindings);
