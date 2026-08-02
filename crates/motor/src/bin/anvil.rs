//! CLI de anvil: lee una secuencia desde YAML, la traduce a
//! `DefinicionSecuencia` y la corre contra el ejecutor de pasos por gRPC.
//! Es la semilla del CLI headless del roadmap (M5:
//! `wasmtime run anvil.wasm secuencia.yaml`).
//!
//! Correr (desde la raíz del repo), con el ejecutor ya levantado en otra
//! terminal (ver `basica_datos.rs`):
//!   wasmtime -S cli -S tcp=y -S inherit-network=y --dir=. \
//!     target/wasm32-wasip2/debug/anvil.wasm ejemplos/basica.yaml
//!
//! `--dir=.` expone el directorio actual al WASM para que pueda leer el
//! YAML desde disco; la ruta se pasa como primer argumento.

use cargador::cargar_de_archivo;
use motor::Motor;

fn main() {
    let ruta = match std::env::args().nth(1) {
        Some(r) => r,
        None => {
            eprintln!("uso: anvil <secuencia.yaml>");
            std::process::exit(2);
        }
    };

    let definicion = match cargar_de_archivo(&ruta) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("no se pudo cargar la secuencia '{ruta}': {e}");
            std::process::exit(1);
        }
    };
    println!("secuencia '{}' cargada ({} pasos)", definicion.nombre, definicion.pasos_main.len());

    let mut motor = match Motor::conecta("127.0.0.1", 9100) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("no se pudo conectar al ejecutor de pasos: {e}");
            std::process::exit(1);
        }
    };
    println!("conectado al ejecutor de pasos");

    match motor.ejecuta_secuencia(&definicion) {
        Ok(secuencia) => secuencia.reporte(),
        Err(e) => {
            eprintln!("la secuencia se interrumpió: {e}");
            std::process::exit(1);
        }
    }
}