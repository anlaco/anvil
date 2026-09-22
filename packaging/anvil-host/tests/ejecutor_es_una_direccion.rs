//! An executor is an address, and nothing brings one up (ADR-0046).
//!
//! This replaces `executor_binary.rs`, which tested `type: wasm` and its
//! `path:` — the executor's own binary, spawned by the host. Both are gone,
//! and with them the tests that pinned their diagnostics.
//!
//! What is worth pinning now is the pair that makes the change survivable:
//!
//! - a sequence runs against a bench **someone else started**, which is the
//!   only way it works now and the way it always worked in production;
//! - a sequence written the old way says **what to write instead**. That one
//!   is not a nicety: it is what every sequence in existence says, and the
//!   loader rejecting it with "unknown field 'path'" would leave a person
//!   with no route out.
//!
//! Run in release, like its neighbours: each invocation starts wasmtime and
//! compiles the guest.

use std::process::Output;

mod banco;
use banco::{arranca, dist, raiz_repo, sin_banco};

fn corre(secuencia: &str) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_anvil"))
        .current_dir(raiz_repo())
        .args([secuencia, "--quiet"])
        .output()
        .expect("run anvil")
}

fn codigo(salida: &Output) -> i32 {
    salida.status.code().unwrap_or_else(|| {
        panic!(
            "anvil ended by signal, with no code. stderr:\n{}",
            String::from_utf8_lossy(&salida.stderr)
        )
    })
}

/// A department someone started serves a sequence that only knows its address.
#[test]
fn una_secuencia_corre_contra_un_banco_que_ya_estaba() {
    let Some(_banco) = arranca() else {
        eprintln!(
            "skipped: ejemplos/departamento has not been assembled (make example). \
             The failure case below still runs."
        );
        return;
    };
    let s = corre("ejemplos/demo_departamento.yseq");
    assert_eq!(
        codigo(&s),
        0,
        "the department sequence should pass against a running bench. stderr:\n{}",
        String::from_utf8_lossy(&s.stderr)
    );
}

/// And with nobody there, it says which executor and at what address — not
/// that something is missing from the file.
#[test]
fn sin_banco_dice_a_quien_no_alcanzo() {
    if dist().is_none() {
        eprintln!("skipped: ejemplos/departamento has not been assembled (make example)");
        return;
    }
    // Nobody may have a bench up while this runs, or it passes by connecting.
    let _lock = sin_banco();
    let s = corre("ejemplos/demo_departamento.yseq");
    let stderr = String::from_utf8_lossy(&s.stderr);
    assert_eq!(codigo(&s), 1, "nothing is listening. stderr:\n{stderr}");
    assert!(
        stderr.contains("instrumentos") && stderr.contains("9101"),
        "the failure must name the executor and where it was expected:\n{stderr}"
    );
}

/// A sequence written against a WASM department — which is what every
/// sequence before 0.9 says — must be told what to write, through the real
/// binary and not only in a unit test.
#[test]
fn el_type_wasm_de_siempre_dice_que_escribir() {
    let s = corre("packaging/anvil-host/tests/fixtures/type_wasm_antiguo.yseq");
    let stderr = String::from_utf8_lossy(&s.stderr);
    assert_eq!(codigo(&s), 1);
    for parte in ["type: grpc", "anvil-exec-wasm --modules", "dev:"] {
        assert!(
            stderr.contains(parte),
            "the way out must include '{parte}':\n{stderr}"
        );
    }
}
