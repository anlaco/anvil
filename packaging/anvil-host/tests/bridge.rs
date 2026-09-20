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
//! the demo bench's executor `basica.yseq` declares, which is why they allow a
//! few seconds to come up.

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
        .arg("ejemplos/basica.yseq")
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

// ---------------------------------------------------------------- latency

/// Frame kinds, mirrored from `src/bridge.rs` — the tests speak the same
/// protocol the editor does.
const OPEN: u8 = 0x01;
const OPENED: u8 = 0x02;
const DATA: u8 = 0x04;

fn frame(kind: u8, id: u32, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(5 + payload.len());
    f.push(kind);
    f.extend_from_slice(&id.to_be_bytes());
    f.extend_from_slice(payload);
    f
}

/// A loopback echo server, so the measurement is of the bridge and nothing
/// else: whatever arrives goes straight back.
fn echo_server() -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind echo");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                loop {
                    match std::io::Read::read(&mut stream, &mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            if std::io::Write::write_all(&mut stream, &buf[..n]).is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        }
    });
    addr
}

/// Round-trip latency through the relay, measured many times.
///
/// **Why the maximum and not the mean.** The defect this was written for was
/// starvation, not slowness: the frame reader held the socket's mutex in a
/// tight loop and the connection's relay thread, holding bytes the editor was
/// blocked waiting for, could not get in. Most exchanges were fast and a few
/// took seconds — measured from the editor, 20 runs of `ejemplos/basica.yseq`
/// ranged from 99 ms to 32 s with a 6.8 s median. A mean stays green through
/// that. What is being asserted is that no single exchange stalls, because the
/// engine has no per-step deadline and `wasi-grpc` has none either: the only
/// thing under a stalled exchange is the editor's 120 s last resort.
#[test]
fn no_exchange_through_the_relay_stalls() {
    let echo = echo_server();
    let bridge = start_bridge();

    let (mut ws, _) = tungstenite::connect(&bridge.url).expect("connect to the bridge");
    if let tungstenite::stream::MaybeTlsStream::Plain(tcp) = ws.get_ref() {
        // A stall must fail the assertion, not hang the suite.
        tcp.set_read_timeout(Some(Duration::from_secs(30))).ok();
    }

    // Open a relayed connection to the echo server and wait for OPENED,
    // stepping over the HELLO the bridge sends first.
    ws.send(tungstenite::Message::Binary(
        frame(OPEN, 1, echo.to_string().as_bytes()).into(),
    ))
    .expect("send OPEN");
    loop {
        match ws.read().expect("read") {
            tungstenite::Message::Binary(b) if b[0] == OPENED => break,
            tungstenite::Message::Binary(b) if b[0] == 0x03 => {
                panic!(
                    "the bridge refused the echo server: {}",
                    String::from_utf8_lossy(&b[5..])
                )
            }
            _ => continue,
        }
    }

    const EXCHANGES: usize = 200;
    let mut worst = Duration::ZERO;
    let mut total = Duration::ZERO;
    for i in 0..EXCHANGES {
        let payload = format!("ping {i}");
        let started = std::time::Instant::now();
        ws.send(tungstenite::Message::Binary(
            frame(DATA, 1, payload.as_bytes()).into(),
        ))
        .expect("send DATA");
        loop {
            if let tungstenite::Message::Binary(b) = ws.read().expect("read") {
                if b[0] == DATA {
                    break;
                }
            }
        }
        let took = started.elapsed();
        total += took;
        worst = worst.max(took);
    }

    let mean = total / EXCHANGES as u32;
    // Printed always, not only on failure: the number this guards is a
    // distribution, and seeing it drift is the early warning.
    eprintln!("relay round trip over {EXCHANGES}: mean {mean:?}, worst {worst:?}");
    // Generous on purpose: an echo over loopback is sub-millisecond, so this
    // fails on a stall and not on a slow machine.
    assert!(
        worst < Duration::from_millis(250),
        "an exchange stalled for {worst:?} (mean {mean:?}) over {EXCHANGES} round trips \
         — the relay is not handing the socket over"
    );
}
