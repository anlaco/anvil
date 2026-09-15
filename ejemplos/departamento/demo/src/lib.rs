//! The demonstration bench: simulated steps that let every example, test and
//! CI job run with nothing but the package (ADR-0041).
//!
//! A module of the example department (ADR-0025), so a sequence calls these as
//! `demo/<step>` on an executor of `type: wasm` whose `path` is the
//! department's `anvil-exec-wasm`. Nothing here touches hardware or the network
//! — a component has neither — and every step is deterministic, because tests
//! assert on what they return.

use anvil_step::{step, Ctx, Outcome};

/// Connects to the instrument; fails the first attempt and passes from the
/// second, which is what exercises the engine's retries.
#[step]
fn connect(ctx: Ctx) -> Outcome {
    if ctx.attempt < 2 {
        Outcome::failed(format!(
            "the instrument did not answer (attempt {})",
            ctx.attempt
        ))
    } else {
        Outcome::passed(format!("instrument connected (attempt {})", ctx.attempt))
    }
}

/// Measures voltage and returns the reading; the engine judges it against the
/// sequence's limit (ADR-0008).
///
/// Channel 1 reads 4.2 V and each further channel 0.1 V more, plus `offset`:
/// the formula is arbitrary, the point is that the reading depends on the
/// inputs. `temperature` is context for the report and takes no part in the
/// verdict.
#[step(outputs(channel_used: f64, temperature: f64))]
fn measure_voltage(channel: Option<f64>, offset: Option<f64>) -> Outcome {
    let channel = channel.unwrap_or(1.0);
    let volts = 4.2 + (channel - 1.0) * 0.1 + offset.unwrap_or(0.0);
    Outcome::measured(volts)
        .message(format!("measured: {volts} V (channel {channel})"))
        .output("channel_used", channel)
        .output("temperature", 21.5)
}

/// Checks the led is lit; passes, with no measurement.
#[step]
fn check_led() -> Outcome {
    Outcome::passed("led on")
}

/// Opens a relay; passes, with no measurement.
#[step]
fn open_relay() -> Outcome {
    Outcome::passed("relay open")
}

/// Closes the connection with the instrument.
#[step]
fn disconnect() -> Outcome {
    Outcome::passed("instrument disconnected")
}

/// An instrument that is not there: always `error`, never `fail` — a bench
/// problem says nothing about the unit (ADR-0019, Rule 2).
#[step]
fn instrument_offline() -> Outcome {
    Outcome::error("the instrument did not answer")
}

anvil_step::export!();

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(attempt: i32) -> Ctx {
        Ctx {
            attempt,
            step_name: "connect".into(),
        }
    }

    #[test]
    fn connect_fails_the_first_attempt_and_passes_the_second() {
        assert_eq!(connect(ctx(1)).status, "fail");
        assert_eq!(connect(ctx(2)).status, "pass");
    }

    #[test]
    fn measure_voltage_reads_4_2_on_channel_1_and_follows_its_inputs() {
        let base = measure_voltage(None, None);
        assert_eq!(base.status, "pass");
        assert_eq!(base.measured_value, Some(4.2));

        let v = measure_voltage(Some(3.0), Some(0.05))
            .measured_value
            .unwrap();
        assert!((v - 4.45).abs() < 1e-9, "{v}");
    }

    #[test]
    fn measure_voltage_returns_its_two_named_outputs() {
        let names: Vec<_> = measure_voltage(Some(2.0), None)
            .outputs
            .iter()
            .map(|o| o.name.clone())
            .collect();
        assert_eq!(names, ["channel_used", "temperature"]);
    }

    #[test]
    fn instrument_offline_is_an_error_not_a_fail() {
        let o = instrument_offline();
        assert_eq!(o.status, "error");
        assert_eq!(o.measured_value, None);
    }
}
