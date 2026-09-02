//! The bench's PLC, as a module of the WASM department (ADR-0025).
//!
//! Same step name as `multimetro`, a different instrument and a different
//! measurement: `plc/medir_voltaje` and `multimetro/medir_voltaje` are two
//! steps, and neither shadows the other.

use anvil_step::{step, Outcome};

/// Reads the 24 V rail the PLC feeds.
#[step(name = "medir_voltaje")]
fn measure_voltage() -> Outcome {
    Outcome::measured(24.0).message("plc, 24 V rail")
}

anvil_step::export!();
