//! The bridge lookup (ADR-0023): `anvil` no longer embeds the bridge — it
//! looks for `anvil-puente-wasm` **next to its own executable** and spawns it
//! from there. These tests exercise exactly that lookup, in both directions:
//!
//! - with the file present, a sequence with a `type: wasm` executor runs
//!   through it end to end;
//! - with the file absent, the run stops with an error that names the path
//!   that was looked at — never "your executor is old": a missing file is
//!   not a contract mismatch, and the engine's contract echo (ADR-0020 §4b)
//!   is the one that names both numbers when that is the case.
//!
//! To fake the missing file without touching the real target directory, the
//! test copies the `anvil` binary alone into a temp directory and runs it
//! from there: no bridge beside it, so the lookup fails. The copy is what
//! keeps the test hermetic — it cannot race another test that needs the real
//! pair in place.
//!
//! Like `exit_codes.rs`, run this in release: each invocation starts
//! wasmtime and compiles the guest (~0.9 s with the release host, ~23 s in
//! debug). CI uses `--release` for the same reason.

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

/// The demo component the `demo_wasm` sequence serves. Since the Rust step SDK
/// carries the WIT and the bindings (ADR-0024) it builds with the plain
/// toolchain, so `make build` builds it and it is there — `cargo component` is
/// no longer needed and no longer missing on a clean checkout. The skip stays
/// for a bare `cargo test` that did not go through the Makefile: a skip never
/// claims a pass.
fn componente_demo() -> Option<PathBuf> {
    let ruta = raiz_repo().join("ejemplos/hola-paso/target/wasm32-wasip2/debug/hola_paso.wasm");
    ruta.exists().then_some(ruta)
}

fn corre_desde(binario: &Path, secuencia: &str) -> Output {
    std::process::Command::new(binario)
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

/// With the bridge where it belongs, the demo sequence runs its `.wasm` steps
/// through it. Skips (with a notice) when the demo component has not been
/// compiled: the *lookup* itself is still covered by the missing-file test.
#[test]
fn con_el_puente_junto_al_binario_la_secuencia_wasm_corre() {
    let Some(_componente) = componente_demo() else {
        eprintln!(
            "skipped: ejemplos/hola-paso has not been compiled (make example). \
             The bridge lookup's missing-file case still runs below."
        );
        return;
    };
    let s = corre_desde(
        Path::new(env!("CARGO_BIN_EXE_anvil")),
        "ejemplos/demo_wasm.yaml",
    );
    assert_eq!(
        codigo(&s),
        0,
        "the demo sequence should pass with the bridge beside the binary. stderr:\n{}",
        String::from_utf8_lossy(&s.stderr)
    );
}

/// The lookup's failure mode, with the named path: the binary runs from a
/// directory that has no bridge, a `type: wasm` executor is declared, and the
/// error must name the path that was looked at — not a connection error, and
/// never a claim about contract versions.
#[test]
fn sin_el_puente_falla_nominando_la_ruta_buscada() {
    // No point without the component: the loader would fail-fast on the
    // missing `.wasm` before reaching the bridge lookup, and the test would
    // pass for the wrong reason.
    if componente_demo().is_none() {
        eprintln!("skipped: ejemplos/hola-paso has not been compiled (make example)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("anvil-bridge-lookup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let anvil_solo = dir.join("anvil");
    std::fs::copy(env!("CARGO_BIN_EXE_anvil"), &anvil_solo).expect("copy the anvil binary");

    let s = corre_desde(&anvil_solo, "ejemplos/demo_wasm.yaml");
    let stderr = String::from_utf8_lossy(&s.stderr);
    assert_eq!(
        codigo(&s),
        1,
        "a missing bridge must stop the run before touching anything. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("anvil-puente-wasm"),
        "the error must name the bridge. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains(dir.join("anvil-puente-wasm").display().to_string().as_str()),
        "the error must name the path it looked at ({}). stderr:\n{stderr}",
        dir.join("anvil-puente-wasm").display()
    );
    // On an assert failure this stays behind in /tmp; the OS cleans that up.
    let _ = std::fs::remove_file(&anvil_solo);
    let _ = std::fs::remove_dir(&dir);
}
