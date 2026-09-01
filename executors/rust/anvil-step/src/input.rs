// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO
//! Turning the sequence's parameters into the function's arguments.
//!
//! This is what `#[step]` leans on so it does not have to recognise types
//! itself: it writes `<f64 as Input>::TYPE` and lets the trait answer. A
//! parameter of a type that does not implement `Input` is a **compile error**,
//! which is the whole advantage of doing this in Rust — in Python an
//! unannotated parameter can only be described as *unspecified* and left
//! unchecked.

use crate::{Named, Value, ValueType};

/// One of the three types that cross the boundary.
pub trait Scalar: Sized {
    const TYPE: ValueType;
    fn from_value(v: &Value) -> Option<Self>;
}

impl Scalar for f64 {
    const TYPE: ValueType = ValueType::Number;
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Number(x) => Some(*x),
            _ => None,
        }
    }
}

impl Scalar for String {
    const TYPE: ValueType = ValueType::Text;
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Text(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl Scalar for bool {
    const TYPE: ValueType = ValueType::Boolean;
    fn from_value(v: &Value) -> Option<Self> {
        match v {
            Value::Boolean(b) => Some(*b),
            _ => None,
        }
    }
}

/// What a step's parameter may be: `f64`, `String`, `bool`, or an `Option` of
/// one of them — an `Option` is how a parameter is declared optional.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a step parameter",
    label = "not a type a sequence can send",
    note = "a step parameter is f64, String or bool — or Option<T> of one of them, which makes it optional. A parameter that needs structure is a badly cut step (ADR-0020 §2)."
)]
pub trait Input: Sized {
    const TYPE: ValueType;
    const REQUIRED: bool;

    /// Pulls this parameter out of the inputs. Called **after**
    /// `StepSpec::check` has passed, so `Err` here means the check and this
    /// disagree — a bug in the SDK, reported as `error` rather than a panic
    /// that would abort the component.
    fn extract(name: &str, inputs: &[Named]) -> Result<Self, String>;
}

impl<T: Scalar> Input for Option<T> {
    const TYPE: ValueType = T::TYPE;
    const REQUIRED: bool = false;

    fn extract(name: &str, inputs: &[Named]) -> Result<Self, String> {
        match inputs.iter().find(|n| n.name == name) {
            None => Ok(None),
            Some(n) => T::from_value(&n.value)
                .map(Some)
                .ok_or_else(|| mismatch(name)),
        }
    }
}

/// The three required cases. Written out one by one and not as a blanket
/// `impl<T: Scalar> Input for T`: that one overlaps with the `Option<T>` above
/// as far as coherence can tell, because nothing proves `Option<T>` is not
/// itself a `Scalar`.
macro_rules! input_for_scalar {
    ($t:ty) => {
        impl Input for $t {
            const TYPE: ValueType = <$t as Scalar>::TYPE;
            const REQUIRED: bool = true;

            fn extract(name: &str, inputs: &[Named]) -> Result<Self, String> {
                let n = inputs
                    .iter()
                    .find(|n| n.name == name)
                    .ok_or_else(|| missing(name))?;
                <$t as Scalar>::from_value(&n.value).ok_or_else(|| mismatch(name))
            }
        }
    };
}

input_for_scalar!(f64);
input_for_scalar!(String);
input_for_scalar!(bool);

fn missing(name: &str) -> String {
    format!("the parameter '{name}' did not arrive")
}

fn mismatch(name: &str) -> String {
    format!("the parameter '{name}' arrived with another type")
}
