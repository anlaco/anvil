//! `anvil describe`: the catalog, out as data (ADR-0044).
//!
//! This is the half that cannot be tested anywhere else. The serialiser has
//! unit tests in `crates/motor/src/describe.rs` against catalogs written by
//! hand; what only the real binary can show is that the **host starts the
//! `type: wasm` executors for a subcommand that runs no step**. Get that wrong
//! and nothing fails — the document comes out well-formed, with an empty
//! `executors`, which reads as "this bench serves nothing". That is the exact
//! false green ADR-0019 Rule 2 is about, and it is why this file exists.
//!
//! Like `exit_codes.rs` and `executor_binary.rs`, run it in release: each
//! invocation starts wasmtime and compiles the guest.

use std::path::{Path, PathBuf};
use std::process::Output;

/// The repo root. The binary only preopens its CWD, so the tests run from
/// here and pass **relative paths**.
fn raiz_repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// The example department, as `make example` leaves it. Skips when it has not
/// been built — a skip never claims a pass.
fn hay_departamento() -> bool {
    let dist = raiz_repo().join("ejemplos/departamento/dist");
    dist.join(format!("anvil-exec-wasm{}", std::env::consts::EXE_SUFFIX))
        .exists()
        && dist.join("multimetro.wasm").exists()
}

fn describe(secuencia: &str) -> Output {
    describe_con(secuencia, &["--quiet"])
}

fn describe_con(secuencia: &str, extra: &[&str]) -> Output {
    let mut args = vec!["describe", secuencia];
    args.extend_from_slice(extra);
    std::process::Command::new(env!("CARGO_BIN_EXE_anvil"))
        .current_dir(raiz_repo())
        .args(&args)
        .output()
        .expect("run anvil describe")
}

fn json(salida: &Output) -> serde_json::Value {
    let texto = String::from_utf8_lossy(&salida.stdout);
    serde_json::from_str(&texto).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}).\nstdout:\n{texto}\nstderr:\n{}",
            String::from_utf8_lossy(&salida.stderr)
        )
    })
}

#[test]
fn el_catalogo_del_departamento_sale_por_stdout_como_json() {
    if !hay_departamento() {
        eprintln!("skipped: ejemplos/departamento has not been assembled (make example)");
        return;
    }
    let s = describe("ejemplos/demo_departamento.yseq");
    assert_eq!(s.status.code(), Some(0), "describe should exit 0");

    let doc = json(&s);
    assert_eq!(doc["describe_version"], 1);

    let ejecutores = doc["executors"].as_object().expect("executors is an object");
    // The one the sequence declares, and it is not empty: an empty catalog
    // here would mean the host never started the `type: wasm` executor, and
    // the document would still be well-formed.
    assert_eq!(ejecutores.len(), 1, "one declared executor: {ejecutores:?}");
    let e = ejecutores.values().next().unwrap();
    assert_eq!(e["describes"], true);

    let pasos = e["steps"].as_array().expect("steps is an array");
    assert!(!pasos.is_empty(), "the demo bench serves steps");

    // Qualified by logical module name (ADR-0025 §2, ADR-0026 §1): the name a
    // sequence must write, prefix and all.
    let nombres: Vec<&str> = pasos.iter().filter_map(|p| p["name"].as_str()).collect();
    assert!(
        nombres.contains(&"multimetro/medir_voltaje"),
        "expected a qualified name, got {nombres:?}"
    );

    // And the signature, which is the whole point: this is what an editor
    // draws its parameter table from instead of guessing from the YAML.
    let medir = pasos
        .iter()
        .find(|p| p["name"] == "multimetro/medir_voltaje")
        .expect("multimetro/medir_voltaje");
    assert!(!medir["doc"].as_str().unwrap_or_default().is_empty());
    let canal = medir["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "canal")
        .expect("the 'canal' input");
    // Types cross as their names, never as the enum's integers (ADR-0044 §5).
    assert_eq!(canal["type"], "number");
}

#[test]
fn describe_no_ejecuta_un_paso() {
    if !hay_departamento() {
        eprintln!("skipped: ejemplos/departamento has not been assembled (make example)");
        return;
    }
    // `Describe` asks, it does not measure — and **without `--quiet`**, which
    // is the whole of what makes this an assertion. The console report's
    // frozen header goes to stdout (`crates/result_sink/src/consola.rs`), so
    // a `describe` that fell through to a run would put `=== … ===` there and
    // the document would stop being JSON. Asked quietly, the report is
    // silenced and this test passes against an engine that ran the sequence:
    // the first version of it did exactly that.
    let s = describe_con("ejemplos/demo_departamento.yseq", &[]);
    let stdout = String::from_utf8_lossy(&s.stdout);
    assert!(
        !stdout.contains("==="),
        "the console report's header must not appear:\n{stdout}"
    );
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_ok(),
        "stdout should be the catalog and nothing else:\n{stdout}"
    );
}

#[test]
fn un_ejecutor_inalcanzable_no_se_omite_del_documento() {
    // A sequence declaring a `grpc` executor nobody is serving. The run
    // refuses to connect, which is the honest answer — what must not happen is
    // a zero exit with an empty `executors`, because that reads as "serves
    // nothing" (ADR-0019, Rule 2; ADR-0028 is about exactly this confusion).
    let s = describe("packaging/anvil-host/tests/fixtures/describe_caido.yseq");
    assert_ne!(
        s.status.code(),
        Some(0),
        "an executor that cannot be reached is not a success. stdout:\n{}",
        String::from_utf8_lossy(&s.stdout)
    );
    let stderr = String::from_utf8_lossy(&s.stderr);
    assert!(
        stderr.contains("describe"),
        "the failure should name what was being done:\n{stderr}"
    );
}
