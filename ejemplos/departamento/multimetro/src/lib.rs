//! The bench's multimeter, as a module of the WASM department (ADR-0025).
//!
//! Its logical name is not written anywhere in here: it is the file stem of
//! the `.wasm` this crate builds (`multimetro`), and that is what a sequence
//! says in `multimetro/medir_voltaje`.

use anvil_step::{step, Outcome};

/// Measures DC voltage on a channel.
///
/// It shares its name with `plc`'s step on purpose: that is the collision the
/// qualified name exists to resolve.
#[step(name = "medir_voltaje", outputs(canal_usado: f64))]
fn measure_voltage(canal: Option<f64>) -> Outcome {
    let canal = canal.unwrap_or(1.0);
    Outcome::measured(4.2)
        .message(format!("multimeter, channel {canal}"))
        .output("canal_usado", canal)
}

anvil_step::export!();
