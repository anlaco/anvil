//! El contrato de exit codes del binario `anvil` (issue #16).
//!
//! | Código | Significa |
//! |---|---|
//! | `0` | la secuencia corrió y el veredicto agregado es `paso` |
//! | `1` | cualquier otra cosa: `fallo`, `error`, `inconcluso` (ADR-0019), error de carga, error de uso |
//!
//! **Por qué estos tests lanzan el binario y no llaman a una función.** El
//! veredicto lo decide el guest (`crates/motor/src/bin/anvil.rs`), que corre
//! en `wasm32-wasip2` bajo wasmtime. El std de esa plataforma aplana cualquier
//! `process::exit(n≠0)` a `I32Exit(1)` al cruzar `wasi:cli/run`, así que el
//! contrato sólo es observable atravesando el host de verdad. Un test que
//! ejercitara el motor compilado nativo pasaría en verde sin probar nada de lo
//! que importa aquí.
//!
//! **Corre esto en release.** Cada invocación arranca wasmtime y compila el
//! guest: ~0,9 s con el host de release, ~23 s con el de debug (wasmtime
//! compila el guest sin optimizar). Con cuatro casos eso es la diferencia
//! entre 4 s y minuto y medio:
//!
//! ```sh
//! make release
//! cargo test --release --manifest-path packaging/anvil-host/Cargo.toml
//! ```
//!
//! Si `cargo test` a secas parece colgado, es esto: no está colgado, está en
//! debug. CI usa `--release` por el mismo motivo.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// La raíz del repo. El binario sólo preabre su CWD (`main.rs`, `wasi.
/// preopened_dir(&cwd, ".")`), así que los tests corren desde aquí y pasan
/// **rutas relativas**: una ruta absoluta no atraviesa el sandbox WASI y
/// muere con «no se pudo leer el fichero (os error 44)», que es un error de
/// carga y saldría 1 por el motivo equivocado — el test pasaría en verde sin
/// haber ejecutado la secuencia.
fn raiz_repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("raíz del repo")
}

/// Corre `anvil` con `--quiet` (el reporte de consola no aporta al exit code y
/// ensucia la salida del runner) y devuelve la salida completa.
fn corre(secuencia: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_anvil"))
        .current_dir(raiz_repo())
        // Sin `--port`: cada proceso reserva uno efímero (issue #15), así que
        // los tests conviven en paralelo. Fijarlo los haría chocar entre sí.
        .args([secuencia, "--quiet"])
        .output()
        .expect("lanzar anvil")
}

/// Ídem, pero **sin `--quiet`**: para los casos en que el exit code por sí solo
/// no distingue el veredicto de un error de carga (los dos salen 1), y hay que
/// leer el reporte para saber que se midió lo que se creía medir.
fn corre_con_reporte(secuencia: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_anvil"))
        .current_dir(raiz_repo())
        .arg(secuencia)
        .output()
        .expect("lanzar anvil")
}

/// `.status.code()` es `None` si al proceso lo mató una señal; eso no es «el
/// contrato dice otra cosa», es que el test no llegó a medir nada.
fn codigo(salida: &Output) -> i32 {
    salida.status.code().unwrap_or_else(|| {
        panic!(
            "anvil terminó por señal, sin código. stderr:\n{}",
            String::from_utf8_lossy(&salida.stderr)
        )
    })
}

#[test]
fn una_secuencia_que_pasa_sale_con_cero() {
    let s = corre("packaging/anvil-host/tests/fixtures/paso.yaml");
    assert_eq!(
        codigo(&s),
        0,
        "un veredicto `paso` es el único caso que sale 0. stderr:\n{}",
        String::from_utf8_lossy(&s.stderr)
    );
}

#[test]
fn una_secuencia_que_falla_sale_con_uno() {
    // `ejemplos/veredicto.yaml` documenta en su cabecera que acaba en `fallo`
    // (ADR-0018, veredicto compuesto). Es el caso que motivó el issue #16:
    // corrida exitosa, resultado negativo — y antes salía 0.
    let s = corre("ejemplos/veredicto.yaml");
    assert_eq!(
        codigo(&s),
        1,
        "un `fallo` no puede salir 0: es lo que rompía `anvil x.yaml && desplegar`. stderr:\n{}",
        String::from_utf8_lossy(&s.stderr)
    );
}

#[test]
fn una_secuencia_con_error_de_ejecucion_sale_con_uno() {
    let s = corre("packaging/anvil-host/tests/fixtures/error_runtime.yaml");
    let stderr = String::from_utf8_lossy(&s.stderr);
    assert_eq!(codigo(&s), 1, "un `error` sale 1. stderr:\n{stderr}");
    // Blinda la intención: tiene que ser un error de EJECUCIÓN. Si el fixture
    // degenerara en un error de carga (p. ej. si `expr` empezara a plegar
    // constantes al validar), seguiría saliendo 1 y el test seguiría verde
    // midiendo otra cosa.
    assert!(
        !stderr.contains("no se pudo cargar la secuencia"),
        "el fixture debe fallar al ejecutar, no al cargar. stderr:\n{stderr}"
    );
}

#[test]
fn una_secuencia_cuyo_veredicto_no_se_evalua_sale_con_uno() {
    // Issue #31 / ADR-0019, Regla 1. Es el caso más traicionero de la tabla:
    // ningún paso en rojo, el `pass_fail` que hace de veredicto saltado por
    // precondición, y antes la secuencia salía `paso` con exit 0 — un pipeline
    // aprobando una unidad que nadie midió.
    //
    // Sin `--quiet` a propósito: un exit 1 a secas no distingue `inconcluso`
    // de «el fichero no existe», que también sale 1. Sin la aserción sobre el
    // reporte, este test daría verde midiendo el camino equivocado.
    let s = corre_con_reporte("packaging/anvil-host/tests/fixtures/inconcluso.yaml");
    let stdout = String::from_utf8_lossy(&s.stdout);
    assert_eq!(
        codigo(&s),
        1,
        "un veredicto sin evaluar no puede salir 0. stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&s.stderr)
    );
    assert!(
        stdout.contains("=== exit_inconcluso: inconcluso ==="),
        "el 1 tiene que venir del veredicto, no de un error de carga. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[saltado] verdict"),
        "el paso se sigue reportando como lo que fue. stdout:\n{stdout}"
    );
}

#[test]
fn una_secuencia_que_no_carga_sale_con_uno() {
    let s = corre("no-existe-de-verdad.yaml");
    assert_eq!(
        codigo(&s),
        1,
        "un YAML inexistente sale 1. stderr:\n{}",
        String::from_utf8_lossy(&s.stderr)
    );
}

#[test]
fn un_error_de_uso_se_aplana_a_uno_bajo_el_host() {
    // `anvil.rs` hace `exit(2)` para el error de uso (convención Unix), pero
    // el std de `wasm32-wasip2` lo aplana a 1 al cruzar `wasi:cli/run`. Este
    // test fija esa realidad: es lo que fuerza que el contrato publicado sea
    // binario en vez de 0/1/2. Si algún día dejara de aplanarse, saltaría aquí
    // y tocaría revisar el contrato documentado, no silenciar el test.
    let s = Command::new(env!("CARGO_BIN_EXE_anvil"))
        .current_dir(raiz_repo())
        .arg("--flag-inventado")
        .output()
        .expect("lanzar anvil");
    assert_eq!(
        codigo(&s),
        1,
        "el exit(2) del guest se ve como 1 a través del host. stderr:\n{}",
        String::from_utf8_lossy(&s.stderr)
    );
}
