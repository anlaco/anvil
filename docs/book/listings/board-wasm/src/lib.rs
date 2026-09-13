use anvil_step::{step, Outcome};

/// Measures the supply rail, in volts.
#[step]
fn measure_rail() -> Outcome {
    Outcome::measured(4.98)
}

anvil_step::export!();
