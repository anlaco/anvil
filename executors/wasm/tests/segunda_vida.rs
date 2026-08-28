//! The bridge's second life (ADR-0023, issue #57): the binary a human
//! launches by hand, with its own CLI. Only the host-spawned life was ever
//! exercised — the bridge can now be copied anywhere and started with
//!
//! ```sh
//! anvil-puente-wasm --wasm <path.wasm> [--port <n>] [--bind <ip>]
//! ```
//!
//! and the tests here launch it exactly that way. They use
//! `CARGO_BIN_EXE_*`, the binary this workspace itself just built, so no
//! artifact from outside the crate is needed. One of the three cases needs a
//! real component (`ejemplos/hola-paso`, built with `cargo component`): when
//! it is missing it says so and skips — a skip never claims a pass, the way
//! `make test-executors` skips without python3.

use std::io::Read;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

const SONDEOS: u32 = 1000; // 10 s of 10 ms polls; a native binary binds fast.

/// The demo component, if it has been compiled. `cargo component` is not part
/// of this repo's build, so this is `None` on a clean checkout and in CI.
fn componente_demo() -> Option<std::path::PathBuf> {
    let ruta = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ejemplos/hola-paso/target/wasm32-wasip1/debug/hola_paso.wasm")
        .canonicalize()
        .ok()?;
    ruta.exists().then_some(ruta)
}

/// Reserves an ephemeral loopback port the way the host does (bind 0, read
/// back, release), so concurrent tests never pick the same one.
fn puerto_libre() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind :0");
    let puerto = listener.local_addr().expect("local addr").port();
    drop(listener);
    puerto
}

/// Without `--wasm` the bridge has nothing to serve: it refuses instead of
/// listening on an empty component. This is the CLI's front door.
#[test]
fn sin_wasm_no_arranca() {
    let out = Command::new(env!("CARGO_BIN_EXE_anvil-puente-wasm"))
        .output()
        .expect("launch the bridge");
    let code = out.status.code().expect("exit code");
    assert_ne!(
        code, 0,
        "no --wasm is a usage error, not a listening server. stdout:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// A file that is not WebAssembly gets the bridge's own diagnosis, not
/// wasmtime's generic parse error — this is DIAG-5's whole point, and the
/// second life must say it too.
#[test]
fn fichero_que_no_es_wasm_se_diagnostica() {
    let dir = std::env::temp_dir().join(format!("puente-segunda-vida-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let basura = dir.join("no-es-wasm.bin");
    std::fs::write(&basura, b"esto no empieza por la cabecera \\0asm").expect("write junk");

    let out = Command::new(env!("CARGO_BIN_EXE_anvil-puente-wasm"))
        .args(["--wasm"])
        .arg(&basura)
        .output()
        .expect("launch the bridge");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_ne!(
        out.status.code().unwrap_or(0),
        0,
        "it must fail. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("no es un fichero WebAssembly")
            || stderr.contains("not a WebAssembly file"),
        "the not-a-WASM diagnosis is expected. stderr:\n{stderr}"
    );
    // On an assert failure this stays behind in /tmp; the OS cleans that up.
    let _ = std::fs::remove_dir_all(&dir);
}

/// With a real component, a hand-launched bridge does what the host relies
/// on: it loads the component, listens, and exits **on its own** when its
/// stdin closes (EOF) — the lifecycle the host's pipe depends on.
#[test]
fn escucha_y_sale_por_eof() {
    let Some(componente) = componente_demo() else {
        eprintln!(
            "skipped: ejemplos/hola-paso has not been compiled (cargo component build \
             --manifest-path ejemplos/hola-paso/Cargo.toml)"
        );
        return;
    };
    let puerto = puerto_libre();

    let mut hijo = Command::new(env!("CARGO_BIN_EXE_anvil-puente-wasm"))
        .args(["--wasm"])
        .arg(&componente)
        .args(["--port", &puerto.to_string()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("launch the bridge");
    // The bridge holds its own stdin read; closing our end sends the EOF.
    let stdin = hijo.stdin.take().expect("piped stdin");

    // Readiness: same probe the host uses — a connect it discards.
    let addr = format!("127.0.0.1:{puerto}");
    let mut listo = false;
    for _ in 0..6000 {
        if TcpStream::connect(&addr).is_ok() {
            listo = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(listo, "the bridge must listen on the port it was given");

    // EOF on stdin: the bridge must exit by itself, with code 0.
    drop(stdin);
    let mut status = None;
    for _ in 0..30 {
        match hijo.try_wait().expect("wait for the bridge") {
            Some(s) => {
                status = Some(s);
                break;
            }
            None => std::thread::sleep(Duration::from_millis(500)),
        }
    }
    let status = status.expect("the bridge must exit on its own after stdin closes");
    assert!(
        status.success(),
        "EOF must be an orderly exit (0), not a crash. stderr follows:\n{}",
        {
            let mut s = String::new();
            let _ = hijo.stderr.take().map(|mut e| e.read_to_string(&mut s));
            s
        }
    );
}