// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO
//! What a step gives back.

use crate::{Named, Value};

/// The step met its criterion.
pub const PASS: &str = "pass";
/// The unit does not comply. Information about the DUT.
pub const FAIL: &str = "fail";
/// Could not be judged. Information about the bench or the step.
pub const ERROR: &str = "error";

/// What a step gives back.
///
/// Build it with the constructors below rather than by hand: `status` is a
/// closed vocabulary, and a string Anvil cannot read turns the step into
/// `error` on the other side, with a message naming what you returned
/// (ADR-0019, Rule 2). `"skipped"` is not here on purpose — only the engine
/// produces it.
///
/// **The threshold is not the step's business**: return the measurement and let
/// the engine judge it against the sequence's `limit` (ADR-0008).
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub status: &'static str,
    pub message: String,
    pub measured_value: Option<f64>,
    /// Named values the step returns **besides** the measurement. They take no
    /// part in the verdict; `assign` reads them as `result.outputs.<name>`
    /// (ADR-0020 §3).
    pub outputs: Vec<Named>,
}

impl Outcome {
    fn of(status: &'static str, message: impl Into<String>) -> Self {
        Outcome {
            status,
            message: message.into(),
            measured_value: None,
            outputs: Vec::new(),
        }
    }

    /// The step met its criterion.
    pub fn passed(message: impl Into<String>) -> Self {
        Outcome::of(PASS, message)
    }

    /// The unit does not comply. **Information about the DUT** — not about the
    /// bench, and not about your step (ADR-0019, Rule 2).
    pub fn failed(message: impl Into<String>) -> Self {
        Outcome::of(FAIL, message)
    }

    /// The step could not judge: the instrument did not answer, the parameter
    /// made no sense, the bench is not where it should be. Never `failed` for
    /// these — a broken bench says nothing about the unit.
    pub fn error(message: impl Into<String>) -> Self {
        Outcome::of(ERROR, message)
    }

    /// A measurement. The engine judges it against the sequence's `limit`.
    pub fn measured(value: f64) -> Self {
        Outcome {
            status: PASS,
            message: String::new(),
            measured_value: Some(value),
            outputs: Vec::new(),
        }
    }

    /// Free text for the report: what the step did, or why it could not.
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// One named output, the ones `assign` reads as `result.outputs.<name>`.
    /// Declare them in the attribute too (`#[step(outputs(name: f64))]`) so
    /// Anvil can check that expression without running the sequence.
    pub fn output(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.outputs.push(Named::new(name, value));
        self
    }
}

/// Whatever a step returned, as an `Outcome`.
///
/// A step may return an `Outcome` or, when there is nothing to add:
///
/// - `f64` → a measurement,
/// - `bool` → pass/fail, the simplest step there is,
/// - `()` → passed with no measurement,
/// - `Result<T, E>` → `Err` is **`error`**, never `fail`.
///
/// That last one is how a step reports a bench problem without ceremony, and it
/// matters more here than in Python: a component compiled to WASM aborts on
/// `panic!`, so there is no exception for the SDK to catch. `?` is the way.
pub trait IntoOutcome {
    fn into_outcome(self) -> Outcome;
}

impl IntoOutcome for Outcome {
    fn into_outcome(self) -> Outcome {
        self
    }
}

impl IntoOutcome for f64 {
    fn into_outcome(self) -> Outcome {
        Outcome::measured(self)
    }
}

impl IntoOutcome for bool {
    fn into_outcome(self) -> Outcome {
        if self {
            Outcome::passed("")
        } else {
            Outcome::failed("the step returned false")
        }
    }
}

impl IntoOutcome for () {
    fn into_outcome(self) -> Outcome {
        Outcome::passed("")
    }
}

impl<T: IntoOutcome, E: std::fmt::Display> IntoOutcome for Result<T, E> {
    /// `Err` is **`error`**, never `fail`: a step that blew up says nothing
    /// about the unit under test, and reporting it as a failed unit is the
    /// false red that mirrors ADR-0019's false green.
    fn into_outcome(self) -> Outcome {
        match self {
            Ok(v) => v.into_outcome(),
            Err(e) => Outcome::error(format!("the step could not run: {e}")),
        }
    }
}
