//! Source of `../firma_no_casa.wasm`, the fixture behind
//! `a_signature_mismatch_names_both_sides` (issue #24). It builds directly
//! against `wit-bindgen`, bypassing the `anvil-step` SDK on purpose: the SDK
//! only ever emits the correct signature, and BUG-06 (the beta bug this
//! guards) came from someone hand-rolling the WIT binding themselves.
//!
//! `wit/anvil-step.wit` here is a copy of the real one with a single
//! deliberate edit: `run`'s `attempt` is `u32` instead of `s32` — the exact
//! type BUG-06 got wrong. Everything else, `describe` included, matches.
//!
//! Not part of any Cargo workspace (`[workspace]` in `Cargo.toml`), the same
//! trick `ejemplos/hola-paso` uses to stay a standalone project.
//!
//! To regenerate `../firma_no_casa.wasm`:
//! ```sh
//! cd executors/wasm/tests/fixtures/firma_no_casa-fuente
//! cargo build --target wasm32-wasip2 --release
//! cp target/wasm32-wasip2/release/paso_roto.wasm ../firma_no_casa.wasm
//! ```

wit_bindgen::generate!({
    path: "wit",
    world: "anvil-step",
});

struct Component;

use exports::anvil::step::step as wit;

impl wit::Guest for Component {
    fn run(_name: String, attempt: u32, _inputs: Vec<wit::Named>) -> wit::StepResult {
        wit::StepResult {
            status: "pass".to_string(),
            message: format!("attempt {attempt}"),
            measured_value: None,
            outputs: Vec::new(),
        }
    }

    fn describe() -> Vec<wit::StepSpec> {
        Vec::new()
    }
}

export!(Component);
