//! Paso de ejemplo "hola mundo" (M5-ext.2, ADR-0015): un componente WASM
//! que exporta `run` (interfaz `anvil:paso`). Es la referencia de la guía
//! "escribe un paso en Rust, compílalo a .wasm y ejecútalo con Anvil".
//!
//! El componente NO sabe de gRPC ni de protobuf: sólo recibe el nombre del
//! paso y el número de intento, y devuelve un resultado. Quien habla gRPC
//! con el motor es el puente (`anvil-puente-wasm`), que carga este
//! componente y llama a `run`.
//!
//! Compilar:
//!   cargo component build --manifest-path ejemplos/hola-paso/Cargo.toml
//!   # → ejemplos/hola-paso/target/wasm32-wasip1/debug/hola_paso.wasm
//!
//! Requiere `cargo component` instalado:
//!   cargo install cargo-component --locked

#[allow(warnings)]
mod bindings;

use bindings::exports::anvil::paso::paso::{Guest, Resultado};

struct Component;

impl Guest for Component {
    fn run(nombre: String, intento: i32) -> Resultado {
        Resultado {
            estado: "paso".to_string(),
            mensaje: format!("hola {nombre} (intento {intento})"),
            valor_medido: Some(4.2),
        }
    }
}

bindings::export!(Component with_types_in bindings);
