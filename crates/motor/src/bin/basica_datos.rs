//! La secuencia de ejemplo "basica" expresada como **datos** y corrida por
//! el motor genérico: cada paso se invoca por gRPC contra el ejecutor, no
//! con una llamada directa.
//!
//! Antes de correr esto, en otra terminal:
//!   wasmtime -S cli -S tcp=y -S inherit-network=y \
//!     target/wasm32-wasip2/debug/ejecutor_pasos.wasm
//! Y luego:
//!   wasmtime -S cli -S tcp=y -S inherit-network=y \
//!     target/wasm32-wasip2/debug/basica_datos.wasm

use modelo::{DefinicionPaso, DefinicionSecuencia, Limite};
use motor::Motor;
use result_sink::SinkConsola;

fn main() {
    // El umbral de medir_voltaje vive en los **datos** (RF-29), no en el
    // código del paso: el paso mide 4.2 V y el motor evalúa el rango 4.5–5.5
    // (ADR-0008). En `ejemplos/basica.yaml` es lo mismo, expresado en YAML.
    let definicion = DefinicionSecuencia {
        nombre: "basica_datos".to_string(),
        pasos_setup: vec![DefinicionPaso::nuevo("conectar_equipo", 3)],
        pasos_main: vec![
            DefinicionPaso::con_limite("medir_voltaje", 1, Limite::Rango { min: 4.5, max: 5.5 }),
            DefinicionPaso::nuevo("verificar_led", 1),
        ],
        pasos_cleanup: vec![DefinicionPaso::nuevo("desconectar_equipo", 1)],
        ..Default::default()
    };

    let mut motor = match Motor::conecta("127.0.0.1", 9100) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("no se pudo conectar al ejecutor de pasos: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("conectado al ejecutor de pasos");

    let mut consola = SinkConsola::nuevo(std::io::stdout());
    match motor.ejecuta_secuencia(&definicion, &mut consola) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("la secuencia se interrumpió: {e}");
            std::process::exit(1);
        }
    }
}
