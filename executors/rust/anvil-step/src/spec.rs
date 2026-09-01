// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO
//! The catalog: what a step is called and what it takes.
//!
//! **The signature is the catalog.** Names, types and which inputs are required
//! come from the function's own parameters — nothing is written twice, so
//! nothing can drift. What the signature cannot give, the attribute takes:
//! `outputs` (a return value has no names) and `name` (when the step's name in
//! a sequence is not a valid Rust identifier).

use crate::{Named, Value};

/// What a parameter is, for the catalog. Same vocabulary as the WIT's
/// `value-type` and `paso.proto`'s `ValueType`, minus `reference`: a component
/// cannot hold an object between calls (ADR-0022 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    /// The executor does not say. Anvil reads it as *unchecked* and never
    /// guesses it is a number (ADR-0019, Rule 2). Rust does not produce it —
    /// a parameter always has a type — but the WIT can carry it.
    Unspecified,
    Number,
    Text,
    Boolean,
}

impl ValueType {
    pub fn name(&self) -> &'static str {
        match self {
            ValueType::Unspecified => "unspecified",
            ValueType::Number => "number",
            ValueType::Text => "text",
            ValueType::Boolean => "boolean",
        }
    }

    /// Whether a value that arrived is of this type. `Unspecified` accepts
    /// anything: what is not declared is not checked, it is not guessed.
    pub fn accepts(&self, value: &Value) -> bool {
        match self {
            ValueType::Unspecified => true,
            ValueType::Number => matches!(value, Value::Number(_)),
            ValueType::Text => matches!(value, Value::Text(_)),
            ValueType::Boolean => matches!(value, Value::Boolean(_)),
        }
    }
}

/// One input of a step, as the catalog describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterSpec {
    pub name: &'static str,
    pub r#type: ValueType,
    /// Whether the sequence has to send it. An `Option<T>` in the signature is
    /// what makes it optional.
    pub required: bool,
    pub doc: &'static str,
}

/// One named output — what `assign` reads as `result.outputs.<name>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSpec {
    pub name: &'static str,
    pub r#type: ValueType,
    pub doc: &'static str,
}

/// One step of the catalog.
#[derive(Debug, Clone, Copy)]
pub struct StepSpec {
    pub name: &'static str,
    pub inputs: &'static [ParameterSpec],
    pub outputs: &'static [OutputSpec],
    /// The first line of the step's doc comment.
    pub doc: &'static str,
}

impl StepSpec {
    /// Why these inputs do not fit the signature, or `None` if they do.
    ///
    /// **The step enforces its own catalog.** If it did not, the catalog would
    /// be a promise nobody keeps: a parameter the step does not know would be
    /// dropped, and the step would measure something else and say `pass` — the
    /// same false green the contract echo exists to prevent (ADR-0020 §4b).
    ///
    /// A mismatch is always `error`, never `fail` and never a default.
    pub fn check(&self, inputs: &[Named]) -> Option<String> {
        for got in inputs {
            if !self.inputs.iter().any(|p| p.name == got.name) {
                let known: Vec<&str> = self.inputs.iter().map(|p| p.name).collect();
                let known = if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                };
                return Some(format!(
                    "the step '{}' does not take a parameter called '{}' (it takes: {})",
                    self.name, got.name, known
                ));
            }
        }
        for p in self.inputs {
            match inputs.iter().find(|got| got.name == p.name) {
                None if p.required => {
                    return Some(format!(
                        "the step '{}' needs the parameter '{}' and the sequence did not send it",
                        self.name, p.name
                    ))
                }
                None => continue,
                Some(got) if !p.r#type.accepts(&got.value) => {
                    return Some(format!(
                        "the parameter '{}' of '{}' is {} and a {} arrived",
                        p.name,
                        self.name,
                        p.r#type.name(),
                        got.value.type_name()
                    ))
                }
                Some(_) => continue,
            }
        }
        None
    }
}
