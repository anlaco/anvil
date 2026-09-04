//! Source of `../traps_and_recovers.wasm`, the fixture behind
//! `a_trap_answers_error_and_the_next_call_still_works` (issue #58): two
//! steps built with the real `anvil-step` SDK, one that panics and one that
//! passes, so the test can call the panicking one and then the passing one
//! against the same loaded component — the exact shape of the issue's own
//! repro (a two-step sequence, `revienta` then `bien`).
//!
//! Not part of any Cargo workspace (`[workspace]` in `Cargo.toml`), the same
//! trick `ejemplos/hola-paso` and `firma_no_casa-fuente` use to stay
//! standalone.
//!
//! To regenerate `../traps_and_recovers.wasm`:
//! ```sh
//! cd executors/wasm/tests/fixtures/traps_and_recovers-fuente
//! cargo build --target wasm32-wasip2 --release
//! cp target/wasm32-wasip2/release/traps_and_recovers.wasm ../traps_and_recovers.wasm
//! ```

use anvil_step::{step, Outcome};

/// Panics on purpose — the WASM trap issue #58 is about.
#[step]
fn revienta() -> Outcome {
    panic!("boom")
}

/// Passes — called after `revienta` to prove the component still answers.
#[step]
fn bien() -> Outcome {
    Outcome::passed("ok")
}

anvil_step::export!();
