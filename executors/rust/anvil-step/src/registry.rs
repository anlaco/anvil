// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO
//! The registry `#[step]` writes to and `export!` serves.
//!
//! There is no dispatch table here and no `if` on step names: `run` and
//! `describe` read the same registry, which is what stops the two from
//! disagreeing. The author never lists their steps — declaring one is
//! registering it, in whatever module it lives (ADR-0021 §1).

use crate::{Ctx, IntoOutcome, Named, Outcome, StepSpec};

/// A registered step: the signature to publish and the function to call.
pub struct Step {
    pub spec: StepSpec,
    /// The wrapper `#[step]` writes: it pulls the arguments out of `inputs` and
    /// calls the author's function. Never the function itself, whose signature
    /// is its own.
    pub call: fn(&Ctx, &[Named]) -> Outcome,
}

inventory::collect!(Step);

/// Every registered step, sorted by name — the catalog, in a stable order so
/// two runs of `--validate` read the same.
pub fn catalog() -> Vec<&'static Step> {
    sorted(inventory::iter::<Step>.into_iter().collect())
}

/// The names registered more than once.
///
/// Two steps answering to one name is not something the component can resolve:
/// picking either would run **a different measurement than the sequence asked
/// for** and report `pass`. So it is not resolved — see [`dispatch`].
pub fn duplicates() -> Vec<&'static str> {
    duplicates_in(&catalog())
}

/// Runs the step `ctx.step_name` names.
///
/// Three ways this does not end in the author's function, and all three are
/// `error` — never `fail`, which would be blaming the unit for the bench
/// (ADR-0019, Rule 2):
///
/// - a name this component does not serve, answered with the catalog so the
///   typo is visible;
/// - a duplicate name, which makes the whole component untrustworthy;
/// - inputs that do not fit the signature ([`StepSpec::check`]).
pub fn dispatch(ctx: &Ctx, inputs: &[Named]) -> Outcome {
    dispatch_in(&catalog(), ctx, inputs)
}

fn sorted(mut steps: Vec<&'static Step>) -> Vec<&'static Step> {
    steps.sort_by_key(|s| s.spec.name);
    steps
}

fn duplicates_in(steps: &[&Step]) -> Vec<&'static str> {
    let mut dup: Vec<&'static str> = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        if steps[i + 1..].iter().any(|o| o.spec.name == s.spec.name) && !dup.contains(&s.spec.name)
        {
            dup.push(s.spec.name);
        }
    }
    dup
}

fn dispatch_in(steps: &[&Step], ctx: &Ctx, inputs: &[Named]) -> Outcome {
    let dup = duplicates_in(steps);
    if !dup.is_empty() {
        // Every step, not just the duplicated one: a component that cannot say
        // which function a name means cannot be trusted about any of them, and
        // finding that out at step 47 is worse than not starting.
        return Outcome::error(format!(
            "this component registers more than one step called: {}. Two steps \
             answering to one name would run a different measurement than the \
             sequence asked for",
            dup.join(", ")
        ));
    }
    let Some(step) = steps.iter().find(|s| s.spec.name == ctx.step_name) else {
        let known: Vec<&str> = steps.iter().map(|s| s.spec.name).collect();
        let known = if known.is_empty() {
            "none".to_string()
        } else {
            known.join(", ")
        };
        return Outcome::error(format!(
            "this component does not serve a step called '{}' (it serves: {})",
            ctx.step_name, known
        ));
    };
    if let Some(bad) = step.spec.check(inputs) {
        return Outcome::error(bad);
    }
    (step.call)(ctx, inputs)
}

/// What `#[step]` calls to turn whatever the author's function returned into an
/// `Outcome`. Here and not in the macro so the conversion is one place.
#[doc(hidden)]
pub fn outcome_of(v: impl IntoOutcome) -> Outcome {
    v.into_outcome()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParameterSpec, Value, ValueType};

    fn medir(_ctx: &Ctx, _inputs: &[Named]) -> Outcome {
        Outcome::measured(4.2)
    }

    const CANAL: &[ParameterSpec] = &[ParameterSpec {
        name: "canal",
        r#type: ValueType::Number,
        required: true,
        doc: "",
    }];

    fn step(name: &'static str, inputs: &'static [ParameterSpec]) -> Step {
        Step {
            spec: StepSpec {
                name,
                inputs,
                outputs: &[],
                doc: "",
            },
            call: medir,
        }
    }

    fn ctx(name: &str) -> Ctx {
        Ctx {
            attempt: 1,
            step_name: name.to_string(),
        }
    }

    #[test]
    fn an_unknown_name_is_error_and_names_the_catalog() {
        let a = step("medir_voltaje", &[]);
        let b = step("conectar_equipo", &[]);
        let steps = vec![&a, &b];
        let o = dispatch_in(&steps, &ctx("medir_voltage"), &[]);
        assert_eq!(o.status, crate::outcome::ERROR);
        assert!(o.message.contains("medir_voltage"), "{}", o.message);
        assert!(o.message.contains("conectar_equipo"), "{}", o.message);
    }

    #[test]
    fn a_duplicated_name_stops_every_step_of_the_component() {
        let a = step("medir_voltaje", &[]);
        let b = step("medir_voltaje", &[]);
        let c = step("conectar_equipo", &[]);
        let steps = vec![&a, &b, &c];
        // Not even the one that is not duplicated runs.
        let o = dispatch_in(&steps, &ctx("conectar_equipo"), &[]);
        assert_eq!(o.status, crate::outcome::ERROR);
        assert!(o.message.contains("medir_voltaje"), "{}", o.message);
    }

    #[test]
    fn inputs_that_do_not_fit_the_signature_are_error_not_a_measurement() {
        let a = step("medir_voltaje", CANAL);
        let steps = vec![&a];
        let o = dispatch_in(
            &steps,
            &ctx("medir_voltaje"),
            &[Named::new("canal", "tres".to_string())],
        );
        assert_eq!(o.status, crate::outcome::ERROR);
        assert_eq!(o.measured_value, None, "no silent measurement");
    }

    #[test]
    fn a_step_that_fits_runs() {
        let a = step("medir_voltaje", CANAL);
        let steps = vec![&a];
        let o = dispatch_in(
            &steps,
            &ctx("medir_voltaje"),
            &[Named::new("canal", Value::Number(1.0))],
        );
        assert_eq!(o.status, crate::outcome::PASS);
        assert_eq!(o.measured_value, Some(4.2));
    }
}
