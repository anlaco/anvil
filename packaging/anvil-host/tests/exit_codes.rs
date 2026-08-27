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
        stdout.contains("=== exit_inconcluso: inconclusive ==="),
        "el 1 tiene que venir del veredicto, no de un error de carga. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[skipped] verdict"),
        "el paso se sigue reportando como lo que fue. stdout:\n{stdout}"
    );
}

#[test]
fn una_asigna_tras_un_error_no_borra_la_variable_que_lee_el_cleanup() {
    // Issue #27 / ADR-0019, Regla 2. El exit code aquí no distingue nada (el
    // `error` del paso inexistente ya sale 1 con o sin el arreglo): lo que se
    // mide es el reporte, donde el `pass_fail` de `cleanup` dice si
    // `locals.valor` conservó su 99.0 o se lo llevó por delante un `nothing`.
    let s = corre_con_reporte("packaging/anvil-host/tests/fixtures/asigna_tras_error.yaml");
    let stdout = String::from_utf8_lossy(&s.stdout);
    assert_eq!(
        codigo(&s),
        1,
        "el paso inexistente deja la secuencia en `error`. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[pass] check_valor: condición cumplida"),
        "la variable que el cleanup va a usar no se toca si el paso dio error. stdout:\n{stdout}"
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

// --- `--validate` (issues #17, #19, #20, #21, #22) ------------------------
//
// El manual promete que `--validate` «carga la secuencia, valida el schema,
// resuelve subsecuencias y detecta ciclos — sin ejecutar nada ni levantar el
// ejecutor». Cumplía la segunda mitad a medias y la primera de menos: cinco
// clases de secuencia rota salían aprobadas y morían luego en runtime, a
// mitad de la corrida. Hasta esta tanda no había ni un solo test E2E del flag.
//
// Son E2E y no unitarios del cargador por lo mismo que el resto del fichero:
// lo que se fija aquí es el **exit code** observable, que sólo existe
// atravesando el host.

/// Corre `anvil <secuencia> --validate`. Sin `--quiet`: varios de estos tests
/// leen stderr, que es donde va el diagnóstico.
fn valida(secuencia: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_anvil"))
        .current_dir(raiz_repo())
        .args([secuencia, "--validate"])
        .output()
        .expect("lanzar anvil")
}

#[test]
fn validate_de_una_secuencia_correcta_sale_con_cero() {
    // La línea base que faltaba: sin ella, cualquier test de abajo pasaría en
    // verde si `--validate` se rompiera del todo y rechazara siempre.
    let s = valida("packaging/anvil-host/tests/fixtures/paso.yaml");
    let err = String::from_utf8_lossy(&s.stderr);
    assert_eq!(codigo(&s), 0, "stderr:\n{err}");
    assert!(err.contains("válida"), "stderr:\n{err}");
}

#[test]
fn validate_rechaza_una_variable_no_declarada() {
    // Issue #19.
    let s = valida("packaging/anvil-host/tests/fixtures/validate_var_no_declarada.yaml");
    let err = String::from_utf8_lossy(&s.stderr);
    assert_eq!(codigo(&s), 1, "stderr:\n{err}");
    assert!(err.contains("no_existe"), "stderr:\n{err}");
}

#[test]
fn validate_rechaza_valor_medido_en_un_sequence_call() {
    // Issue #20.
    let s = valida("packaging/anvil-host/tests/fixtures/validate_valor_medido_en_call.yaml");
    let err = String::from_utf8_lossy(&s.stderr);
    assert_eq!(codigo(&s), 1, "stderr:\n{err}");
    assert!(err.contains("valor_medido"), "stderr:\n{err}");
}

#[test]
fn validate_rechaza_ejecutores_en_una_subsecuencia_externa() {
    // Issue #21. Este es el caso que salía **exit 0**: la sección se
    // descartaba y nadie decía nada.
    let s = valida("packaging/anvil-host/tests/fixtures/validate_sub_con_ejecutores.yaml");
    let err = String::from_utf8_lossy(&s.stderr);
    assert_eq!(codigo(&s), 1, "stderr:\n{err}");
    assert!(
        err.contains("raíz"),
        "el mensaje tiene que decir dónde se declaran. stderr:\n{err}"
    );
}

/// Issue #22, el test del arreglo.
///
/// La secuencia declara un `tipo: wasm` cuyo fichero **existe** pero no es un
/// componente. Con el guard puesto, el host no toca ese fichero bajo
/// `--validate` y la secuencia sale válida.
///
/// **Al revertir el guard este test tarda ~60 s en ponerse rojo**: el host
/// spawnea el puente, el puente muere al instanciar el componente, y
/// `esperar_wasm` agota sus sondeos antes de rendirse. Es el precio de no
/// depender de un `.wasm` construido en `target/`, que no existe en un clon
/// limpio.
#[test]
fn validate_no_levanta_el_puente_wasm() {
    let s = valida("packaging/anvil-host/tests/fixtures/validate_wasm_basura.yaml");
    let err = String::from_utf8_lossy(&s.stderr);
    assert!(
        !err.contains("anvil-puente-wasm"),
        "el puente no debe arrancar bajo --validate. stderr:\n{err}"
    );
    assert!(!err.contains("escuchando en"), "stderr:\n{err}");
    assert!(!err.contains("no empezó a escuchar"), "stderr:\n{err}");
    assert_eq!(codigo(&s), 0, "stderr:\n{err}");
}

// --- El ejecutor embebido y el pre-escaneo del host (issue #52) -----------

/// Issue #52, el test del arreglo.
///
/// El host pre-escanea el YAML por su cuenta (M5-ext.1) para recolectar los
/// `ejecutores:` declarados. Cuando ese parseo fallaba por esquema, el host
/// **además** daba por hecho que el guest tampoco iba a poder cargar la
/// secuencia y se saltaba el ejecutor de pasos embebido —sin decir nada.
///
/// La deducción sólo vale mientras host y guest compartan cargador. En cuanto
/// dejan de compartirlo (basta un build a medias: el host es un workspace
/// aparte y los guests van embebidos), el guest carga la secuencia, no hay
/// ejecutor escuchando, y el motor cae al `9100` por defecto —el host tampoco
/// le pasa `--port` en esa rama— y muere con un `connection-refused` que no
/// nombra ni la causa ni a nadie. Cuarenta segundos para no decir nada.
///
/// Lo que se fija aquí es que **el host no predice el veredicto del guest**:
/// si los argumentos dicen que se van a correr pasos, el ejecutor arranca,
/// aunque el pre-escaneo del host haya rechazado el fichero.
///
/// Al reintroducir el `&& !yaml_invalido` en `va_a_ejecutar` este test se pone
/// rojo: la línea del ejecutor desaparece de stderr.
#[test]
fn el_ejecutor_embebido_arranca_aunque_el_host_no_sepa_leer_el_yaml() {
    let s = corre_con_reporte("packaging/anvil-host/tests/fixtures/esquema_invalido.yaml");
    let err = String::from_utf8_lossy(&s.stderr);
    assert!(
        err.contains("ejecutor de pasos escuchando en"),
        "el ejecutor embebido tiene que arrancar igual: que el host no sepa \
         parsear el YAML no dice nada de si el guest podrá. stderr:\n{err}"
    );
    // Blinda la intención: el fixture tiene que ser rechazado por **esquema**.
    // Si degenerara en un fichero que carga bien, el test seguiría verde sin
    // haber ejercitado la rama que importa.
    assert_eq!(codigo(&s), 1, "stderr:\n{err}");
    assert!(
        err.contains("no se pudo cargar la secuencia"),
        "el fixture debe fallar al cargar, no al ejecutar. stderr:\n{err}"
    );
}

/// El contrapunto del anterior: el guard por argumentos sigue en pie. Un
/// `--validate` no levanta nada, y eso no lo puede aflojar el arreglo de #52
/// (issue #22 lo compró caro).
#[test]
fn validate_sigue_sin_levantar_el_ejecutor_embebido() {
    let s = valida("packaging/anvil-host/tests/fixtures/esquema_invalido.yaml");
    let err = String::from_utf8_lossy(&s.stderr);
    assert!(
        !err.contains("ejecutor de pasos escuchando en"),
        "--validate no levanta el ejecutor, pase lo que pase con el YAML. stderr:\n{err}"
    );
}

/// El contrapunto: no instanciar no es quedarse ciego. Que el `.wasm` exista
/// es una comprobación de fichero, la hace el cargador, y sigue corriendo.
#[test]
fn validate_sigue_comprobando_que_el_wasm_existe() {
    let s = valida("packaging/anvil-host/tests/fixtures/validate_wasm_inexistente.yaml");
    let err = String::from_utf8_lossy(&s.stderr);
    assert_eq!(codigo(&s), 1, "stderr:\n{err}");
    assert!(err.contains("no existe"), "stderr:\n{err}");
}
