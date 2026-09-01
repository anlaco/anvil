// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO
//! Write a step for Anvil in Rust: annotate a function, compile to WASM.
//!
//! This is the whole authoring surface. You never write a `run` that dispatches
//! by name, never copy a `wit/` directory, and never keep a generated
//! `bindings.rs` in your project.
//!
//! ```ignore
//! use anvil_step::{step, Ctx, Outcome};
//!
//! /// Measures the DC voltage on a channel.
//! #[step(outputs(channel_used: f64))]
//! fn measure_voltage(ctx: Ctx, channel: f64, scale: Option<String>) -> Outcome {
//!     let volts = read_instrument(channel)?;
//!     Outcome::measured(volts).output("channel_used", channel)
//! }
//!
//! anvil_step::export!();
//! ```
//!
//! **The signature is the catalog.** Names, types and which inputs are required
//! come from the function's own parameters — nothing is written twice, so
//! nothing can drift, and Anvil can check a sequence *without running it*
//! (ADR-0021). What the signature cannot give, the attribute takes: `outputs`
//! (a return value has no names) and `name` (when the step's name in a sequence
//! is not a valid Rust identifier).
//!
//! A parameter is `f64`, `String` or `bool`, or an `Option` of one of them,
//! which is how it is declared optional. Anything else is a compile error: a
//! parameter that needs structure is a badly cut step (ADR-0020 §2).
//!
//! `ctx` is the executor talking to the step — the attempt number, the name the
//! sequence used — and a step gets one only if it asks for one. It is never
//! part of the described signature.
//!
//! **Objects that stay on the bench** —an instrument session, an open socket—
//! have no place here: a component is a function with no state between calls,
//! so it has nowhere to keep one, and the bridge rejects a reference before it
//! arrives (ADR-0022 §8). A step that needs one is served from a `grpc`
//! executor of its own process, such as the Python one.

pub use anvil_step_macros::step;

// Re-exported for `export!` and for what `#[step]` generates. Not part of the
// authoring surface: a step never names them.
#[doc(hidden)]
pub use inventory;
#[doc(hidden)]
pub use wit_bindgen;

#[cfg(target_arch = "wasm32")]
pub mod bindings;
mod input;
mod outcome;
mod registry;
mod spec;

pub use input::{Input, Scalar};
pub use outcome::{IntoOutcome, Outcome};
#[doc(hidden)]
pub use registry::outcome_of;
pub use registry::{catalog, dispatch, duplicates, Step};
pub use spec::{OutputSpec, ParameterSpec, StepSpec, ValueType};

/// A typed value crossing the boundary: the same three types as the `value` of
/// the WIT, the `oneof Value` of `paso.proto` and `expr::Value`. No lists and
/// no maps — a parameter that needs structure is a badly cut step
/// (ADR-0020 §2).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
    Boolean(bool),
}

impl Value {
    /// What this value is, for the message that says a type did not match.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "number",
            Value::Text(_) => "text",
            Value::Boolean(_) => "boolean",
        }
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Number(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.to_string())
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Boolean(v)
    }
}

/// A `Value` with its name. Named rather than positional: sequences are written
/// and reviewed by hand, and reordering two parameters must not change what the
/// bench measures (ADR-0020 §2).
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub name: String,
    pub value: Value,
}

impl Named {
    pub fn new(name: impl Into<String>, value: impl Into<Value>) -> Self {
        Named {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// What the executor knows about this invocation and the step does not.
///
/// A step receives it only if it declares a `ctx` parameter, and `ctx` is never
/// part of the described signature: it is the executor talking to the step, not
/// a value that comes out of the sequence.
///
/// It carries **only** what the WIT hands over. There is no `options` and no
/// object store here, unlike the Python executor: a component is a function
/// with no state between calls, so it has nowhere to keep a bench session
/// (ADR-0022 §8). Promising them would be lying.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// Attempt number, starting at 1 (RF-09). It is here and not in the
    /// signature because it is not something the sequence sets.
    pub attempt: i32,
    /// The name the sequence used, useful when one function serves several
    /// names.
    pub step_name: String,
}

/// Turns this crate into an Anvil step component. **One line, at the end of
/// your `lib.rs`.**
///
/// It takes no arguments: the steps are the ones `#[step]` registered, wherever
/// they live. Nothing to list, nothing to keep in sync — which is the point,
/// and also a constraint. `wit_bindgen::generate!` defines macros of its own,
/// and Rust does not allow defining a `macro_rules!` inside the expansion of a
/// macro that has metavariables, so a version of this taking a list of steps
/// could not exist even if we wanted one.
///
/// Outside `wasm32` it expands to nothing, so a step crate still `cargo test`s
/// natively.
#[cfg(target_arch = "wasm32")]
#[macro_export]
macro_rules! export {
    () => {
        // The generated macro takes a bare identifier, so the type is brought
        // into scope under a name nobody would write by hand.
        use $crate::bindings::Component as __AnvilStepComponent;
        $crate::bindings::__anvil_export_bindings!(__AnvilStepComponent with_types_in $crate::bindings);
    };
}

#[cfg(not(target_arch = "wasm32"))]
#[macro_export]
macro_rules! export {
    () => {};
}
