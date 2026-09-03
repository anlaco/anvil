//! The executor binary a sequence declares (ADR-0027).
//!
//! `path:` on a `type: wasm` executor is **the executor's own binary**, not a
//! `.wasm` and not a folder of modules: Anvil spawns exactly that file, and
//! where its modules live is the executor's business — it finds them next to
//! itself. That is what keeps a build path out of a sequence.
//!
//! This replaces the ADR-0023 lookup these tests used to exercise, where the
//! bridge was always the one sitting next to `anvil` and a sequence could not
//! say otherwise.
//!
//! Both directions are covered:
//!
//! - with a real department (the binary plus its `.wasm` beside it), a
//!   sequence runs through it end to end;
//! - with a `path` that is not a file, the run stops naming it, and says what
//!   a `path` is now — the mistake the change makes easy is pointing it at the
//!   `.wasm`, and that answer has to be in the message.
//!
//! Like `exit_codes.rs`, run this in release: each invocation starts wasmtime
//! and compiles the guest (~0.9 s with the release host, ~23 s in debug).

use std::path::{Path, PathBuf};
use std::process::Output;

/// The repo root. The binary only preopens its CWD, so the tests run from
/// here and pass **relative paths** (an absolute path would not cross the
/// WASI sandbox).
fn raiz_repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// The example department: the bridge binary with its two modules beside it,
/// as `make example` leaves it. Skips when it has not been built — a skip
/// never claims a pass.
fn departamento_demo() -> Option<PathBuf> {
    let ruta = raiz_repo().join("ejemplos/departamento/dist/anvil-exec-wasm");
    let modulo = raiz_repo().join("ejemplos/departamento/dist/multimetro.wasm");
    (ruta.exists() && modulo.exists()).then_some(ruta)
}

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

/// A department declared by its binary runs, and its modules are found by the
/// executor itself — nothing in the sequence says where they are.
#[test]
fn un_departamento_declarado_por_su_binario_corre() {
    let Some(_) = departamento_demo() else {
        eprintln!(
            "skipped: ejemplos/departamento has not been assembled (make example). \
             The failure case below still runs."
        );
        return;
    };
    let s = corre("ejemplos/demo_departamento.yaml");
    assert_eq!(
        codigo(&s),
        0,
        "the department sequence should pass. stderr:\n{}",
        String::from_utf8_lossy(&s.stderr)
    );
}

/// Pointing `path` at a `.wasm` — the natural mistake coming from before
/// ADR-0027 — must stop the run and say what a `path` is now.
///
/// Seen to fail by dropping the `is_file`/extension guidance from
/// `instanciar_wasm`: anvil tries to `exec` the `.wasm`, and the error is
/// "Exec format error", which sends the reader to look at their toolchain.
#[test]
fn apuntar_path_a_un_wasm_lo_dice() {
    if departamento_demo().is_none() {
        eprintln!("skipped: ejemplos/departamento has not been assembled (make example)");
        return;
    }
    let dir = raiz_repo().join("packaging/anvil-host/tests/fixtures");
    let yaml = dir.join("wasm_path_es_un_wasm.yaml");
    let s = corre(
        yaml.strip_prefix(raiz_repo())
            .expect("relative")
            .to_str()
            .expect("utf8"),
    );
    let stderr = String::from_utf8_lossy(&s.stderr);
    assert_eq!(
        codigo(&s),
        1,
        "a `path` that is not an executor must stop the run. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("anvil-exec-wasm") || stderr.contains("binario del ejecutor"),
        "the error must say what a 'path' is now. stderr:\n{stderr}"
    );
}
