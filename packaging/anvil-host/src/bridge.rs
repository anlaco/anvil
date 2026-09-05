//! The bridge: what lets the engine reach executors when it runs in a browser.
//!
//! A browser has no TCP sockets, so the engine hosted in a tab cannot speak
//! gRPC to anything. This process is the missing half: it accepts one WebSocket
//! from the editor, opens the real TCP connections on its behalf and relays
//! bytes both ways. It is `anvil-host` minus the engine — the engine having
//! moved to the browser (ADR-0030).
//!
//! **It also holds the network boundary that wasmtime holds in the native
//! path.** There, `socket_addr_check` refuses anything that is not loopback or
//! declared in the sequence's `executors:`, and the property is that nothing
//! leaves loopback without being declared. With the engine in a tab, wasmtime
//! is not in the path any more and a shim in the page is not a boundary — it is
//! code sitting beside the code it would constrain. So the check lives here,
//! unchanged in meaning.
//!
//! And it authenticates. A WebSocket is not subject to the same-origin policy
//! the way `fetch` is: any page the user has open can try to connect to
//! `ws://127.0.0.1:PORT`. On a machine whose port reaches a power supply, an
//! unauthenticated local socket is not acceptable, so every connection must
//! present the token this process mints at start-up and prints.
//!
//! ## Wire format
//!
//! One WebSocket carries every connection, multiplexed. Binary frames only:
//!
//! ```text
//! byte 0     kind
//! bytes 1-4  connection id, big-endian u32, minted by the editor
//! bytes 5..  payload
//! ```
//!
//! | kind | name   | direction | payload                     |
//! |------|--------|-----------|-----------------------------|
//! | 0x01 | Open   | editor →  | "host:port", UTF-8          |
//! | 0x02 | Opened | → editor  | none                        |
//! | 0x03 | Failed | → editor  | reason, UTF-8               |
//! | 0x04 | Data   | both      | bytes                       |
//! | 0x05 | Close  | both      | none                        |
//! | 0x06 | Hello  | → editor  | JSON, once on connect       |
//!
//! `Hello` is the first frame the bridge sends, and it carries the arguments
//! the engine would have been given had it run here — the ephemeral port of the
//! embedded executor, and an `--executor name=host:port` for each `type: wasm`
//! one. The native host injects these into the guest's argv
//! (`main.rs:548-576`); with the engine in a browser there is no argv to inject
//! into, so they travel over the wire instead. Without them the engine falls
//! back to port 9100 and cannot reach anything, which is exactly what happened
//! the first time this ran end to end.
//!
//! `Failed` carries a reason because the editor shows it to a person: a
//! refused address and an unreachable one are different problems, and a shim
//! that reported both as "connection refused" would let a *policy* decision
//! read as the bench being switched off (ADR-0019, Rule 2).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::{Message, WebSocket};

/// Frame kinds. See the module docs.
const OPEN: u8 = 0x01;
const OPENED: u8 = 0x02;
const FAILED: u8 = 0x03;
const DATA: u8 = 0x04;
const CLOSE: u8 = 0x05;
const HELLO: u8 = 0x06;

/// How long a read on the WebSocket waits before yielding the lock.
///
/// The socket is shared between the frame reader and every connection's relay
/// thread, so the reader cannot simply block on it forever holding the lock.
/// Short enough to stay responsive, long enough not to spin.
const POLL: Duration = Duration::from_millis(5);

/// What the bridge will let the engine connect to.
///
/// The same rule the native host applies with `socket_addr_check`: loopback,
/// plus exactly the non-loopback addresses the sequence declared. Built from
/// `ips_no_loopback_declaradas`, so the two hosts cannot drift apart in what
/// they permit.
pub struct Policy {
    declared: std::collections::HashSet<IpAddr>,
}

impl Policy {
    pub fn new(declared: std::collections::HashSet<IpAddr>) -> Self {
        Self { declared }
    }

    /// Whether the engine may reach this address, and why not when it may not.
    fn check(&self, addr: &SocketAddr) -> Result<(), String> {
        if addr.ip().is_loopback() || self.declared.contains(&addr.ip()) {
            return Ok(());
        }
        Err(format!(
            "{} is not loopback and is not declared in the sequence's `executors:`. \
             Nothing leaves loopback without being declared (ADR-0011, ADR-0030).",
            addr.ip()
        ))
    }
}

/// A connection the bridge is relaying, as seen by the writer side.
struct Relay {
    tcp: TcpStream,
}

/// The shared WebSocket, and the flag that tells relay threads to stop.
struct Shared {
    socket: Mutex<WebSocket<TcpStream>>,
    closed: AtomicBool,
}

impl Shared {
    /// Sends one frame. Errors are swallowed on purpose: a relay thread that
    /// cannot write has nothing useful to do about it, and the reader will
    /// notice the socket is gone.
    fn send(&self, kind: u8, id: u32, payload: &[u8]) {
        let mut frame = Vec::with_capacity(5 + payload.len());
        frame.push(kind);
        frame.extend_from_slice(&id.to_be_bytes());
        frame.extend_from_slice(payload);
        if let Ok(mut s) = self.socket.lock() {
            let _ = s.send(Message::Binary(frame.into()));
        }
    }
}

/// Runs the bridge until the editor disconnects.
///
/// `listener` is already bound; `token` is what a connection must present.
pub fn serve(
    listener: TcpListener,
    token: &str,
    policy: Policy,
    engine_args: &[String],
) -> std::io::Result<()> {
    let policy = Arc::new(policy);

    for stream in listener.incoming() {
        let stream = stream?;
        // One editor at a time. A second connection while one is live is far
        // more likely to be another page trying its luck than a second editor,
        // and the token check below is what decides.
        if let Err(e) = session(stream, token, Arc::clone(&policy), engine_args) {
            eprintln!("bridge: session ended: {e}");
        }
    }
    Ok(())
}

// `ErrorResponse` is tungstenite's type and its size is not ours to change:
// the handshake callback's signature is fixed by the library. Same reason the
// WASM executor allows it on `describe` (executors/wasm/src/main.rs).
#[allow(clippy::result_large_err)]
fn session(
    stream: TcpStream,
    token: &str,
    policy: Arc<Policy>,
    engine_args: &[String],
) -> Result<(), String> {
    let peer = stream.peer_addr().map_err(|e| e.to_string())?;
    if !peer.ip().is_loopback() {
        return Err(format!("refused a connection from {}", peer.ip()));
    }

    // The token is checked **inside** the handshake, and a bad one answers
    // HTTP 403 instead of completing it.
    //
    // Checking after `accept_hdr` returned looked like it worked — the log said
    // "refused" — but the client's `onopen` had already fired, because the 101
    // was on the wire before the check ran. The connection was then closed and
    // nothing could be done through it, so it was not exploitable; it was
    // worse than that, it was a refusal that did not look like one, one
    // refactor away from being real. Found by connecting without a token and
    // watching it be accepted.
    // `Cell` because the closure borrows it mutably for the whole call, and the
    // error mapping afterwards needs to read it.
    let refused = std::cell::Cell::new(false);
    let socket = tungstenite::accept_hdr(
        stream,
        |req: &Request, res: Response| -> Result<Response, ErrorResponse> {
            // The token travels in the query rather than a header: a browser
            // cannot set headers on a WebSocket handshake. It is a loopback URL
            // and never leaves the machine.
            let presented = req.uri().query().and_then(|q| {
                q.split('&')
                    .find_map(|kv| kv.strip_prefix("token=").map(str::to_owned))
            });

            // Length first, then every byte — no early exit on the first
            // mismatch. Not a serious defence against local timing analysis,
            // but it costs nothing to not hand one out.
            let ok = presented
                .as_deref()
                .map(|t| {
                    t.len() == token.len()
                        && t.bytes()
                            .zip(token.bytes())
                            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                            == 0
                })
                .unwrap_or(false);

            if ok {
                return Ok(res);
            }

            refused.set(true);
            let mut err = ErrorResponse::new(Some(
                "this port belongs to an Anvil bridge and needs the token it printed at start-up"
                    .into(),
            ));
            *err.status_mut() = tungstenite::http::StatusCode::FORBIDDEN;
            Err(err)
        },
    )
    .map_err(|e| {
        if refused.get() {
            "refused a connection with a wrong or missing token".to_string()
        } else {
            format!("handshake failed: {e}")
        }
    })?;

    eprintln!("bridge: editor connected from {peer}");
    relay_session(socket, policy, engine_args)
}

fn relay_session(
    socket: WebSocket<TcpStream>,
    policy: Arc<Policy>,
    engine_args: &[String],
) -> Result<(), String> {
    socket
        .get_ref()
        .set_read_timeout(Some(POLL))
        .map_err(|e| e.to_string())?;

    let shared = Arc::new(Shared {
        socket: Mutex::new(socket),
        closed: AtomicBool::new(false),
    });
    let mut relays: HashMap<u32, Relay> = HashMap::new();

    // Hand over the engine's arguments before anything else: the editor needs
    // them to start the engine at all.
    shared.send(HELLO, 0, json_args(engine_args).as_bytes());

    loop {
        if shared.closed.load(Ordering::Relaxed) {
            break;
        }

        let message = {
            let mut s = shared.socket.lock().map_err(|_| "socket poisoned")?;
            match s.read() {
                Ok(m) => Some(m),
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    None
                }
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    break
                }
                Err(e) => return Err(format!("websocket read failed: {e}")),
            }
        };

        let Some(message) = message else { continue };
        let Message::Binary(frame) = message else {
            // Ping/pong and close are handled by tungstenite; text frames are
            // not part of this protocol and are ignored rather than guessed at.
            continue;
        };
        if frame.len() < 5 {
            continue;
        }

        let kind = frame[0];
        let id = u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]);
        let payload = &frame[5..];

        match kind {
            OPEN => open(&shared, &policy, &mut relays, id, payload),
            DATA => {
                if let Some(relay) = relays.get_mut(&id) {
                    if relay.tcp.write_all(payload).is_err() {
                        relays.remove(&id);
                        shared.send(CLOSE, id, &[]);
                    }
                }
            }
            CLOSE => {
                if let Some(relay) = relays.remove(&id) {
                    let _ = relay.tcp.shutdown(std::net::Shutdown::Both);
                }
            }
            _ => {}
        }
    }

    shared.closed.store(true, Ordering::Relaxed);
    for (_, relay) in relays {
        let _ = relay.tcp.shutdown(std::net::Shutdown::Both);
    }
    eprintln!("bridge: editor disconnected");
    Ok(())
}

fn open(
    shared: &Arc<Shared>,
    policy: &Arc<Policy>,
    relays: &mut HashMap<u32, Relay>,
    id: u32,
    payload: &[u8],
) {
    let target = match std::str::from_utf8(payload) {
        Ok(t) => t,
        Err(_) => return shared.send(FAILED, id, b"target address was not valid UTF-8"),
    };

    let addr: SocketAddr = match target.parse() {
        Ok(a) => a,
        Err(e) => {
            return shared.send(
                FAILED,
                id,
                format!("bad address '{target}': {e}").as_bytes(),
            )
        }
    };

    // The boundary. This is the check wasmtime performs in the native path.
    if let Err(why) = policy.check(&addr) {
        eprintln!("bridge: refused {addr}: {why}");
        return shared.send(FAILED, id, why.as_bytes());
    }

    let tcp = match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
        Ok(t) => t,
        Err(e) => {
            return shared.send(
                FAILED,
                id,
                format!("could not reach {addr}: {e}").as_bytes(),
            )
        }
    };

    let reader = match tcp.try_clone() {
        Ok(r) => r,
        Err(e) => {
            return shared.send(
                FAILED,
                id,
                format!("could not split {addr}: {e}").as_bytes(),
            )
        }
    };

    relays.insert(id, Relay { tcp });
    shared.send(OPENED, id, &[]);

    // One thread per connection, reading TCP and pushing frames back. Threads,
    // not async, for the same reason the rest of this binary uses them: the
    // work is blocking and there are a handful of connections, not thousands.
    let shared = Arc::clone(shared);
    std::thread::spawn(move || pump(shared, id, reader));
}

fn pump(shared: Arc<Shared>, id: u32, mut tcp: TcpStream) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        if shared.closed.load(Ordering::Relaxed) {
            return;
        }
        match tcp.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => shared.send(DATA, id, &buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    shared.send(CLOSE, id, &[]);
}

/// The engine's arguments as a JSON array.
///
/// Hand-rolled rather than pulling in a serialiser: it is a list of strings,
/// and `crates/result_sink/src/json.rs` assembles its documents the same way
/// for the same reason.
fn json_args(args: &[String]) -> String {
    let mut out = String::from("{\"args\":[");
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        for c in a.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
    }
    out.push_str("]}");
    out
}

/// Mints the per-session token.
///
/// Not from a cryptographic source, and that is a real limitation: it is
/// derived from the clock and the process id, which someone on the same machine
/// could narrow down. It is a barrier against a web page in another tab
/// wandering into the port, which is the threat ADR-0030 names, and not against
/// a local attacker who already runs code here.
pub fn mint_token() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mixed = now
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(pid << 64);
    format!("{mixed:032x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn loopback_is_allowed() {
        let p = Policy::new(HashSet::new());
        assert!(p.check(&"127.0.0.1:9100".parse().unwrap()).is_ok());
        assert!(p.check(&"[::1]:9100".parse().unwrap()).is_ok());
    }

    #[test]
    fn undeclared_non_loopback_is_refused_with_a_reason() {
        let p = Policy::new(HashSet::new());
        let err = p.check(&"192.168.1.50:5025".parse().unwrap()).unwrap_err();
        // The reason is shown to a person, so it has to name the address and
        // say what would make it allowed.
        assert!(err.contains("192.168.1.50"), "{err}");
        assert!(err.contains("executors"), "{err}");
    }

    #[test]
    fn a_declared_address_is_allowed() {
        let mut declared = HashSet::new();
        declared.insert("192.168.1.50".parse::<IpAddr>().unwrap());
        let p = Policy::new(declared);
        assert!(p.check(&"192.168.1.50:5025".parse().unwrap()).is_ok());
        // Declaring one address does not open the rest of the subnet.
        assert!(p.check(&"192.168.1.51:5025".parse().unwrap()).is_err());
    }

    #[test]
    fn tokens_differ_between_mints() {
        assert_ne!(mint_token(), mint_token());
    }
}
