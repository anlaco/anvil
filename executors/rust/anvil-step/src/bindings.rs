// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO
//! The WIT side: `anvil:step@0.4.0`, and the only part of this crate that a
//! step author never sees.
//!
//! The bindings are generated **here**, in the SDK, and not in the author's
//! crate: that is what removes the `wit/` directory and the checked-in
//! `bindings.rs` from their project. `pub_export_macro` is what makes it
//! possible — the `#[unsafe(no_mangle)]` symbols that turn a `cdylib` into a
//! component have to be emitted by the author's crate, so the macro that emits
//! them travels out of here and `export!` invokes it there.
//!
//! Only compiled for `wasm32`: everything else in this crate is plain Rust and
//! is tested natively, which is why a step's own unit tests need no WASM.

use crate::{Ctx, Named, Value};

wit_bindgen::generate!({
    path: "wit",
    world: "anvil-step",
    pub_export_macro: true,
    export_macro_name: "__anvil_export_bindings",
});

use exports::anvil::step::step as wit;

/// The component. `export!` hands this to the generated macro.
pub struct Component;

impl wit::Guest for Component {
    /// The single door in: turn the WIT's values into the SDK's, dispatch by
    /// name, and translate back. The knowing is all in the registry.
    fn run(name: String, attempt: i32, inputs: Vec<wit::Named>) -> wit::StepResult {
        let ctx = Ctx {
            attempt,
            step_name: name,
        };
        let inputs: Vec<Named> = inputs.iter().map(from_wit).collect();
        let o = crate::dispatch(&ctx, &inputs);
        wit::StepResult {
            status: o.status.to_string(),
            message: o.message,
            measured_value: o.measured_value,
            outputs: o.outputs.iter().map(to_wit).collect(),
        }
    }

    /// The catalog, built at compile time by `#[step]`.
    ///
    /// A duplicated name comes out as an **empty list**, which the bridge reads
    /// as "do not check me": a component that cannot say which function a name
    /// means must not have its catalog believed. Every one of its steps also
    /// comes back as `error` when invoked, so this is not a silence that hides
    /// anything (see `dispatch`).
    fn describe() -> Vec<wit::StepSpec> {
        if !crate::duplicates().is_empty() {
            return Vec::new();
        }
        crate::catalog()
            .iter()
            .map(|s| wit::StepSpec {
                name: s.spec.name.to_string(),
                inputs: s
                    .spec
                    .inputs
                    .iter()
                    .map(|p| wit::ParameterSpec {
                        name: p.name.to_string(),
                        type_: type_to_wit(p.r#type),
                        required: p.required,
                        // Rust has no defaults in a signature: an optional
                        // parameter is an `Option`, and what it falls back to
                        // is inside the step. Declaring it would be declaring
                        // something the SDK cannot read.
                        default: None,
                        doc: p.doc.to_string(),
                    })
                    .collect(),
                outputs: s
                    .spec
                    .outputs
                    .iter()
                    .map(|o| wit::OutputSpec {
                        name: o.name.to_string(),
                        type_: type_to_wit(o.r#type),
                        doc: o.doc.to_string(),
                    })
                    .collect(),
                doc: s.spec.doc.to_string(),
            })
            .collect()
    }
}

fn type_to_wit(t: crate::ValueType) -> wit::ValueType {
    match t {
        crate::ValueType::Unspecified => wit::ValueType::Unspecified,
        crate::ValueType::Number => wit::ValueType::Number,
        crate::ValueType::Text => wit::ValueType::Text,
        crate::ValueType::Boolean => wit::ValueType::Boolean,
    }
}

fn from_wit(n: &wit::Named) -> Named {
    Named {
        name: n.name.clone(),
        value: match &n.value {
            wit::Value::Number(x) => Value::Number(*x),
            wit::Value::Text(s) => Value::Text(s.clone()),
            wit::Value::Boolean(b) => Value::Boolean(*b),
        },
    }
}

fn to_wit(n: &Named) -> wit::Named {
    wit::Named {
        name: n.name.clone(),
        value: match &n.value {
            Value::Number(x) => wit::Value::Number(*x),
            Value::Text(s) => wit::Value::Text(s.clone()),
            Value::Boolean(b) => wit::Value::Boolean(*b),
        },
    }
}
