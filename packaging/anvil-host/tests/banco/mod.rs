//! Bringing the demo bench up, for the tests that need one (ADR-0046).
//!
//! Nothing starts an executor for you any more: a sequence says where one is
//! listening and whoever runs it puts it there. That is the product's
//! behaviour, so it is also the tests' — they start the bench, run against it
//! and take it down, exactly as a person does.
//!
//! A fixed port, not an ephemeral one, because the example sequences name it:
//! `127.0.0.1:9101` is what `ejemplos/*.yseq` declare. A test that picked its
//! own port would have to rewrite the sequence, and then it would not be
//! testing the sequence that ships.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
/// One bench, one port, so two tests cannot have one at the same time.
///
/// A **file** lock and not a `Mutex`, and that distinction cost a flaky run:
/// `cargo test` gives each integration file its own **process**, so a
/// process-local mutex serialises `describe.rs` with itself and not with
/// `exit_codes.rs`. Both wanted 9101 and which one failed depended on
/// scheduling.
///
/// `create_new` is the exclusive part — it fails if the file is there — and a
/// stale lock from a killed test is cleared by age rather than left to block
/// the suite for ever.
pub struct Cerrojo(PathBuf);

impl Drop for Cerrojo {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn cerrojo() -> Cerrojo {
    let ruta = std::env::temp_dir().join("anvil-banco-9101.lock");
    for _ in 0..3000 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&ruta)
        {
            Ok(_) => return Cerrojo(ruta),
            Err(_) => {
                // A lock nobody owns any more: a test killed mid-run would
                // otherwise stop every later run on this machine.
                if let Ok(m) = std::fs::metadata(&ruta) {
                    if m.modified()
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|d| d.as_secs() > 120)
                    {
                        let _ = std::fs::remove_file(&ruta);
                        continue;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }
    panic!("another test has held the bench lock for a minute; something is stuck");
}

/// The port the example sequences declare.
pub const PUERTO: u16 = 9101;

/// The repo root. The binary only preopens its CWD, so tests run from here.
pub fn raiz_repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// The demo department, as `make example` leaves it, or None when it has not
/// been built — a skip never claims a pass.
pub fn dist() -> Option<PathBuf> {
    let d = raiz_repo().join("ejemplos/departamento/dist");
    let exe = d.join(format!("anvil-exec-wasm{}", std::env::consts::EXE_SUFFIX));
    (exe.exists() && d.join("multimetro.wasm").exists()).then_some(d)
}

/// A bench that shuts itself down when the test drops it, and holds the lock
/// until it does.
pub struct Banco(Child, #[allow(dead_code)] Cerrojo);

impl Drop for Banco {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Takes the lock **without** starting anything, for a test that needs to be
/// sure nothing is listening.
///
/// It is the same lock `arranca` takes, and it is the whole point: a test
/// asserting «nobody is there» is meaningless while another thread has a bench
/// up on the same port. The first version of this file did not have it, and
/// that test failed by connecting successfully.
pub fn sin_banco() -> Cerrojo {
    cerrojo()
}

/// Starts the demo bench on [`PUERTO`] and waits for it to listen.
///
/// Serial: one port, so two tests that both want a bench cannot run at once.
/// `cargo test` gives each integration file its own process but shares threads
/// inside it, which is why the callers take a lock rather than trusting luck.
pub fn arranca() -> Option<Banco> {
    let guard = cerrojo();
    let d = dist()?;

    // Holding the lock is not the same as the port being free: the previous
    // bench was killed a moment ago and the kernel has not finished with its
    // socket. Spawning into that gives a process that fails to bind, exits,
    // and is then waited on for six seconds — which is how this file was
    // flaky before the wait was here.
    for _ in 0..500 {
        if std::net::TcpStream::connect(("127.0.0.1", PUERTO)).is_err() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let exe = d.join(format!("anvil-exec-wasm{}", std::env::consts::EXE_SUFFIX));
    let child = Command::new(exe)
        .args([
            "--modules",
            &d.to_string_lossy(),
            "--port",
            &PUERTO.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the demo bench");
    let banco = Banco(child, guard);

    let mut banco = banco;
    for _ in 0..600 {
        if std::net::TcpStream::connect(("127.0.0.1", PUERTO)).is_ok() {
            return Some(banco);
        }
        // A bench that exited says so now instead of after six seconds of
        // connecting to nothing.
        if let Ok(Some(estado)) = banco.0.try_wait() {
            panic!("the demo bench exited before listening: {estado}");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the demo bench did not start listening on 127.0.0.1:{PUERTO}");
}
