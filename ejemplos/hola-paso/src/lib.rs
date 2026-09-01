//! The "hello world" step component (ADR-0015, ADR-0024): three steps in Rust,
//! compiled to a `.wasm` and run by Anvil. The reference for the guide
//! "write a step in Rust, compile it and run it with Anvil".
//!
//! The component knows nothing about gRPC or protobuf: it is handed the step's
//! name, the attempt number and its already-evaluated parameters, and gives
//! back a result. What speaks gRPC with the engine is the bridge
//! (`anvil-puente-wasm`), which loads this component and calls it.
//!
//! It knows nothing about **contract versions** either (ADR-0015): the echo the
//! engine checks is answered by the bridge on its behalf. What does reach it is
//! that the WIT is versioned and travels stuck to the artifact — **the rule is
//! to recompile**, there is no compatibility shim (ADR-0020 §4d). A `.wasm`
//! built against `anvil:step@0.3.0` does not instantiate.
//!
//! Build:
//!   cargo build --target wasm32-wasip2 --manifest-path ejemplos/hola-paso/Cargo.toml
//!   # → ejemplos/hola-paso/target/wasm32-wasip2/debug/hola_paso.wasm

use anvil_step::{step, Ctx, Outcome};

/// Measures the voltage on a channel.
///
/// The measurement comes back on its own: **the threshold is not the step's
/// business**, the engine judges it against the `limit` the sequence declares
/// (ADR-0008). `canal_usado` is a named output, so the example shows that side
/// of the contract too — `assign` reads it as `result.outputs.canal_usado`.
#[step(name = "medir_voltaje", outputs(canal_usado: f64))]
fn measure_voltage(canal: Option<f64>) -> Outcome {
    let canal = canal.unwrap_or(1.0);
    Outcome::measured(4.2)
        .message(format!("measured on channel {canal}"))
        .output("canal_usado", canal)
}

/// Connects to the instrument; fails once, then passes.
///
/// The same shape as `pasos_demo::conectar` in the embedded executor: a
/// transient failure on attempt 1 that passes from attempt 2 — which is how the
/// attempt number reaching the step is exercised (RF-09). It takes a `ctx`
/// because it asks for one; the step above does not.
#[step(name = "conectar_equipo")]
fn connect(ctx: Ctx) -> Outcome {
    if ctx.attempt == 1 {
        // The unit, as far as this fake step is concerned: `fail`. A bench
        // problem would be `error` (ADR-0019, Rule 2).
        return Outcome::failed("lost the handshake (transient)");
    }
    Outcome::passed("connected")
}

/// Greets whoever the sequence names.
///
/// The original hello-world, now with its parameter extracted by the SDK. If
/// the sequence says `a_quien: 3`, the step is never called: the mismatch comes
/// back as `error` naming the parameter, instead of a number quietly turned
/// into text.
#[step]
fn hola(ctx: Ctx, a_quien: Option<String>) -> Outcome {
    let who = a_quien.unwrap_or_else(|| ctx.step_name.clone());
    Outcome::passed(format!("hola {who} (attempt {})", ctx.attempt))
}

anvil_step::export!();
