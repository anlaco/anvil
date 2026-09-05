//! The bridge's door: who gets in, and what they are allowed to reach.
//!
//! **Why these launch the binary instead of calling a function.** What is being
//! tested is the handshake — whether a connection is refused *before* the
//! WebSocket is established — and that only exists once a real client speaks
//! HTTP to a real listener. A unit test of the token comparison would have
//! passed happily while the bug these were written for was live: the check ran
//! after `accept_hdr` had already sent the 101, so the log said "refused" while
//! the client's `onopen` had fired. It looked correct from the server side and
//! was wrong from the only side that matters.
//!
//! These are cheap: the bridge does not instantiate the engine, so unlike
//! `exit_codes.rs` they do not need a release build to be quick. They do start
//! the embedded executor, which is why they allow a few seconds to come up.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root")
}

fn binary() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target");
    p.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    p.push("anvil");
    p
}

/// A running bridge, killed when the test ends.
struct Bridge {
    child: Child,
    url: String,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Starts `anvil --bridge` and reads the URL it prints.
///
/// Reading the URL from stdout rather than guessing a port is deliberate: the
/// port is ephemeral and the token is minted per run, so anything that assumed
/// either would be testing a fiction.
fn start_bridge() -> Bridge {
    let mut child = Command::new(binary())
        .arg("ejemplos/basica.yaml")
        .arg("--bridge")
        .current_dir(repo_root())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("could not start the bridge; build it with `cargo build --manifest-path packaging/anvil-host/Cargo.toml`");

    let stdout = child.stdout.take().expect("stdout");
    let mut url = None;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(at) = line.find("ws://") {
            url = Some(line[at..].trim().to_string());
            break;
        }
    }

    Bridge {
        child,
        url: url.expect("the bridge did not print its URL"),
    }
}

fn without_token(url: &str) -> String {
    url.split("?token=").next().unwrap_or(url).to_string()
}

/// Attempts a WebSocket connection. `Ok` means the handshake completed.
fn connect(url: &str) -> Result<(), String> {
    tungstenite::connect(url)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[test]
fn the_right_token_gets_in_and_every_other_case_does_not() {
    let bridge = start_bridge();
    std::thread::sleep(Duration::from_millis(200));

    let base = without_token(&bridge.url);

    // The bug this exists for: each of these used to complete the handshake and
    // only then be closed, so a client saw a successful connection.
    assert!(
        connect(&base).is_err(),
        "a connection with no token must be refused during the handshake"
    );
    assert!(
        connect(&format!("{base}?token=")).is_err(),
        "an empty token must be refused"
    );
    assert!(
        connect(&format!("{base}?token=0000000000000000000000000000000f")).is_err(),
        "a wrong token must be refused"
    );

    assert!(
        connect(&bridge.url).is_ok(),
        "the token the bridge printed must be accepted"
    );
}

#[test]
fn a_refused_token_is_answered_with_403() {
    // The status matters because it is what tells a person which port they hit.
    // A closed socket says nothing; a 403 with a sentence says what to do.
    let bridge = start_bridge();
    std::thread::sleep(Duration::from_millis(200));

    let err = connect(&without_token(&bridge.url)).unwrap_err();
    assert!(
        err.contains("403") || err.to_lowercase().contains("forbidden"),
        "expected a 403, got: {err}"
    );
}

#[test]
fn each_bridge_mints_its_own_token() {
    // A token that were predictable — or worse, constant — would make the
    // check decoration. Two bridges must not share one.
    let a = start_bridge();
    let b = start_bridge();
    assert_ne!(
        a.url.split("?token=").nth(1),
        b.url.split("?token=").nth(1),
        "two bridges minted the same token"
    );
}
