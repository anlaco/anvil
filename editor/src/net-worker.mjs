// The network half of the socket shim: owns the WebSocket to the bridge, and
// answers the engine's blocking requests.
//
// It speaks the bridge's frame format (packaging/anvil-host/src/bridge.rs):
// one socket carrying every connection, multiplexed by a 32-bit id.
//
//   byte 0     kind
//   bytes 1-4  connection id, big-endian
//   bytes 5..  payload
//
// Everything here is asynchronous, which is the point: this is the side that
// can wait for the network. The engine's worker is parked in `Atomics.wait`
// while this runs, and `ChannelResponder.answer` is what wakes it.

import { ChannelResponder, DATA_BYTES, EOS, OK } from "./wasi/channel.mjs";

const OPEN = 0x01;
const OPENED = 0x02;
const FAILED = 0x03;
const DATA = 0x04;
const CLOSE = 0x05;
const HELLO = 0x06;

/** One relayed connection, as this side sees it. */
class Connection {
  /** Bytes that arrived and have not been read by the engine yet. */
  pending = [];
  /** True once the far end closed. */
  ended = false;
  /** Set while the engine is blocked waiting for bytes on this connection. */
  waiting = null;
  /** Set while the engine is blocked waiting for the connect to resolve. */
  connecting = null;
}

let socket = null;
let responder = null;
/** The shared data buffer: how a write's payload arrives from the engine. */
let sharedData = null;
/** Arguments the bridge says the engine needs; arrive in the HELLO frame. */
let engineArgs = [];
const connections = new Map();

function frame(kind, id, payload = new Uint8Array()) {
  const f = new Uint8Array(5 + payload.byteLength);
  f[0] = kind;
  new DataView(f.buffer).setUint32(1, id, false);
  f.set(payload, 5);
  return f;
}

/** Total bytes buffered for a connection. */
function buffered(conn) {
  return conn.pending.reduce((n, c) => n + c.byteLength, 0);
}

/** Takes up to `max` bytes out of a connection's buffer. */
function take(conn, max) {
  const want = Math.min(max, buffered(conn), DATA_BYTES);
  const out = new Uint8Array(want);
  let at = 0;
  while (at < want) {
    const head = conn.pending[0];
    const n = Math.min(head.byteLength, want - at);
    out.set(head.subarray(0, n), at);
    at += n;
    if (n === head.byteLength) conn.pending.shift();
    else conn.pending[0] = head.subarray(n);
  }
  return out;
}

/** Answers a pending read if it can now be satisfied. */
function serveRead(conn) {
  if (!conn.waiting) return;
  if (buffered(conn) > 0) {
    const { max } = conn.waiting;
    conn.waiting = null;
    responder.answer(OK, take(conn, max));
    return;
  }
  if (conn.ended) {
    conn.waiting = null;
    // End of stream is not an error: it is how a peer says it is done, and the
    // engine has to be able to tell the two apart.
    responder.answer(EOS, null);
  }
}

function onFrame(bytes) {
  if (bytes.byteLength < 5) return;
  const kind = bytes[0];
  const id = new DataView(bytes.buffer, bytes.byteOffset).getUint32(1, false);
  const payload = bytes.subarray(5);

  // The bridge's opening frame, which belongs to no connection: the arguments
  // the engine would have been given as argv in the native path.
  if (kind === HELLO) {
    try {
      engineArgs = JSON.parse(new TextDecoder().decode(payload)).args ?? [];
    } catch {
      engineArgs = [];
    }
    return;
  }

  const conn = connections.get(id);
  if (!conn) return;

  switch (kind) {
    case OPENED:
      if (conn.connecting) {
        conn.connecting = null;
        responder.answer(OK, null);
      }
      break;
    case FAILED:
      connections.delete(id);
      if (conn.connecting) {
        conn.connecting = null;
        // The bridge's reason travels through verbatim. A refusal by policy and
        // an unreachable instrument are different problems, and collapsing them
        // into "connection refused" would let a decision read as a dead bench
        // (ADR-0019, Rule 2).
        responder.fail(new TextDecoder().decode(payload));
      }
      break;
    case DATA:
      conn.pending.push(payload.slice());
      serveRead(conn);
      break;
    case CLOSE:
      conn.ended = true;
      serveRead(conn);
      break;
    default:
      break;
  }
}

function connect(url) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    ws.onopen = () => resolve(ws);
    // A refused handshake — a wrong token answers 403 — surfaces here as a
    // bare error event with nothing in it, so the message says what to check
    // rather than repeating "undefined".
    ws.onerror = () =>
      reject(
        new Error(
          "could not connect to the bridge. Check that `anvil <sequence> --bridge` " +
            "is running and that the URL carries the token it printed.",
        ),
      );
    ws.onmessage = (e) => onFrame(new Uint8Array(e.data));
    ws.onclose = (event) => {
      // Everything in flight dies with the socket; anything blocked has to be
      // released or the engine's thread never comes back.
      for (const [, conn] of connections) {
        conn.ended = true;
        if (conn.connecting || conn.waiting) {
          conn.connecting = null;
          conn.waiting = null;
          responder.fail("the bridge closed the connection");
        }
      }
      connections.clear();
      socket = null;
      // And the page is told, because it is the page that offers Run. An editor
      // that keeps offering it once the bridge is gone is offering something it
      // cannot do, which is the same class of defect as a run that reports
      // nothing (ADR-0019, Rule 3).
      self.postMessage({
        lost: true,
        reason: event.wasClean
          ? "the bridge closed the connection"
          : "the connection to the bridge was lost",
      });
    };
  });
}

// Requests arrive on a direct channel from the engine's worker, not through
// the page: the engine blocks the moment it posts one, and routing that through
// a third thread would add a hop that can be busy painting.
self.onmessage = async ({ data }) => {
  if (data.op !== "start") return;

  responder = new ChannelResponder({ control: data.control, data: data.data });
  sharedData = new Uint8Array(data.data);
  data.port.onmessage = (e) => handle(e.data);
  try {
    socket = await connect(data.url);
    // The bridge sends HELLO immediately, but "immediately" is still a network
    // round trip. Waiting a beat here means `connectBridge` resolves with the
    // arguments already in hand, instead of the editor racing them.
    await new Promise((r) => setTimeout(r, 150));
    self.postMessage({ started: true, engineArgs });
  } catch (e) {
    self.postMessage({ started: false, error: e.message });
  }
};

function handle(data) {
  if (!responder) return;
  if (!socket) {
    responder.fail("not connected to a bridge");
    return;
  }

  switch (data.op) {
    case "connect": {
      const conn = new Connection();
      conn.connecting = true;
      connections.set(data.id, conn);
      socket.send(frame(OPEN, data.id, new TextEncoder().encode(data.target)));
      break;
    }
    case "read": {
      const conn = connections.get(data.id);
      if (!conn) return responder.fail(`no connection ${data.id}`);
      conn.waiting = { max: data.max };
      serveRead(conn);
      break;
    }
    case "write": {
      const conn = connections.get(data.id);
      if (!conn) return responder.fail(`no connection ${data.id}`);
      // The payload is already in shared memory — `BlockingClient.request`
      // copies it there before posting — so it is read from there, not from the
      // message. Copied out because `send` is asynchronous and the engine
      // reuses the buffer for its next request the moment it wakes.
      socket.send(frame(DATA, data.id, sharedData.slice(0, data.length)));
      // Answered immediately: the bridge does not acknowledge writes, and the
      // engine's `check-write` model expects to be told how much it may send
      // rather than when it landed.
      responder.answer(OK, null);
      break;
    }
    case "close": {
      const conn = connections.get(data.id);
      connections.delete(data.id);
      if (conn) socket.send(frame(CLOSE, data.id));
      responder.answer(OK, null);
      break;
    }
    default:
      responder.fail(`unknown operation '${data.op}'`);
  }
}
