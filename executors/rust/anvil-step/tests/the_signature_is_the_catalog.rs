// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO
//! What `#[step]` publishes, and what it refuses to publish.
//!
//! Native tests: none of this needs WASM, which is also the point — a step's
//! own unit tests call the function directly, with no executor in the way.

use anvil_step::{catalog, dispatch, step, Ctx, Named, Outcome, ValueType};

/// Measures the voltage on a channel.
///
/// A second line that must not reach the catalog.
#[step(outputs(channel_used: f64, serial: String))]
fn measure_voltage(ctx: Ctx, channel: f64, scale: Option<String>) -> Outcome {
    Outcome::measured(4.2)
        .message(format!("attempt {} scale {:?}", ctx.attempt, scale))
        .output("channel_used", channel)
}

/// Checks the LED is lit.
#[step]
fn check_led() -> Outcome {
    Outcome::passed("led lit")
}

#[step(name = "medir-con-guiones")]
fn hyphenated(flag: bool) -> Outcome {
    Outcome::passed(format!("{flag}"))
}

/// Returns a bare number.
#[step]
fn bare_number() -> f64 {
    7.5
}

/// Returns a bare bool.
#[step]
fn bare_bool() -> bool {
    false
}

/// Returns a Result whose Err is a bench problem.
#[step]
fn breaks() -> Result<Outcome, String> {
    Err("the instrument did not answer".to_string())
}

fn spec(name: &str) -> anvil_step::StepSpec {
    catalog()
        .iter()
        .find(|s| s.spec.name == name)
        .unwrap_or_else(|| panic!("'{name}' is not in the catalog"))
        .spec
}

fn ctx(name: &str, attempt: i32) -> Ctx {
    Ctx {
        attempt,
        step_name: name.to_string(),
    }
}

#[test]
fn names_types_and_required_come_from_the_signature() {
    let s = spec("measure_voltage");
    let names: Vec<&str> = s.inputs.iter().map(|p| p.name).collect();
    // `ctx` is the executor talking to the step, never a described parameter.
    assert_eq!(names, vec!["channel", "scale"]);
    assert_eq!(s.inputs[0].r#type, ValueType::Number);
    assert!(s.inputs[0].required);
    // An `Option` is how a parameter is declared optional.
    assert_eq!(s.inputs[1].r#type, ValueType::Text);
    assert!(!s.inputs[1].required);
}

#[test]
fn outputs_and_doc_come_from_the_attribute_and_the_doc_comment() {
    let s = spec("measure_voltage");
    let outs: Vec<(&str, ValueType)> = s.outputs.iter().map(|o| (o.name, o.r#type)).collect();
    assert_eq!(
        outs,
        vec![
            ("channel_used", ValueType::Number),
            ("serial", ValueType::Text)
        ]
    );
    assert_eq!(s.doc, "Measures the voltage on a channel.");
}

#[test]
fn a_step_can_be_named_something_rust_cannot_spell() {
    assert_eq!(spec("medir-con-guiones").name, "medir-con-guiones");
    // And the function keeps its own name, callable from its unit tests.
    assert_eq!(hyphenated(true).status, "pass");
}

#[test]
fn the_catalog_is_sorted_so_two_runs_read_the_same() {
    let names: Vec<&str> = catalog().iter().map(|s| s.spec.name).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn a_step_gets_a_ctx_only_if_it_asks_for_one() {
    let o = dispatch(&ctx("measure_voltage", 3), &[Named::new("channel", 1.0f64)]);
    assert!(o.message.contains("attempt 3"), "{}", o.message);
    assert_eq!(dispatch(&ctx("check_led", 1), &[]).message, "led lit");
}

#[test]
fn an_output_the_step_returns_travels_with_the_result() {
    let o = dispatch(&ctx("measure_voltage", 1), &[Named::new("channel", 2.0f64)]);
    assert_eq!(o.outputs, vec![Named::new("channel_used", 2.0f64)]);
}

#[test]
fn a_missing_required_parameter_is_error_and_never_a_default() {
    let o = dispatch(&ctx("measure_voltage", 1), &[]);
    assert_eq!(o.status, "error");
    // The message the catalog gives, not the extractor's last-resort one: what
    // is being checked is that the step enforces its own signature.
    assert!(
        o.message
            .contains("needs the parameter 'channel' and the sequence did not send it"),
        "{}",
        o.message
    );
    assert_eq!(o.measured_value, None);
}

#[test]
fn a_parameter_of_the_wrong_type_is_error_and_never_a_conversion() {
    let o = dispatch(
        &ctx("measure_voltage", 1),
        &[Named::new("channel", "three")],
    );
    assert_eq!(o.status, "error");
    assert!(
        o.message.contains("is number and a text arrived"),
        "{}",
        o.message
    );
}

#[test]
fn an_optional_parameter_the_sequence_omits_is_the_steps_business() {
    let o = dispatch(&ctx("measure_voltage", 1), &[Named::new("channel", 1.0f64)]);
    assert_eq!(o.status, "pass");
    assert!(o.message.contains("scale None"), "{}", o.message);
}

#[test]
fn a_bare_number_is_a_measurement_and_a_bare_bool_is_pass_fail() {
    assert_eq!(
        dispatch(&ctx("bare_number", 1), &[]).measured_value,
        Some(7.5)
    );
    let o = dispatch(&ctx("bare_bool", 1), &[]);
    assert_eq!(o.status, "fail");
    // Not "measured 0.0, passed": a pass/fail step has no measurement.
    assert_eq!(o.measured_value, None);
}

#[test]
fn an_err_is_error_never_fail() {
    let o = dispatch(&ctx("breaks", 1), &[]);
    assert_eq!(
        o.status, "error",
        "a broken bench says nothing about the unit"
    );
    assert!(o.message.contains("did not answer"), "{}", o.message);
}
