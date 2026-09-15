// `wasi:sockets` for a JavaScript host, over the bridge.
//
// The engine reaches executors with gRPC on raw TCP (ADR-0006), and a browser
// has no TCP at all. So these calls are relayed to `anvil <sequence> --bridge`,
// which opens the real connections (ADR-0030). What makes that possible from
// synchronous WASI calls is `Atomics.wait`: this runs on the engine's worker
// and blocks it, while the network worker does the asynchronous half and wakes
// it. See ./channel.mjs.
//
// The streams handed back are built with the shim's own `_create`, not
// hand-rolled objects: the generated module checks `ret instanceof InputStream`
// and refuses anything else with "Resource error: Not a valid InputStream
// resource". The handler is ours; the wrapper has to be theirs.

import { inputStreamCreate, outputStreamCreate, pollableCreate } from "./io.mjs";
import { BlockingClient, EOS, ERROR, OK } from "./channel.mjs";

/** Set once by the engine worker, before the engine runs. */
let client = null;
let nextId = 1;

/** Sockets the guest has open, so they can be closed when it exits. */
const open = new Set();

/** Wires the shim to the network worker. Called from engine-worker.mjs. */
export function useBridge({ control, data, port }) {
  client = new BlockingClient({ control, data, port });
}

/** Whether a bridge is available at all. */
export function hasBridge() {
  return client !== null;
}

function unavailable() {
  throw new Error(
    "no bridge is connected, so the engine cannot reach any executor. Start one " +
      "with `anvil <sequence.yaml> --bridge` and open the editor with the URL it " +
      "prints. Nothing on the validate path needs this.",
  );
}

/** `{tag:'ipv4', val:{address:[..], port}}` → `"127.0.0.1:9100"`. */
function formatAddress(addr) {
  if (addr?.tag === "ipv4") {
    return `${addr.val.address.join(".")}:${addr.val.port}`;
  }
  if (addr?.tag === "ipv6") {
    const host = addr.val.address.map((g) => g.toString(16)).join(":");
    return `[${host}]:${addr.val.port}`;
  }
  throw new Error(`unsupported address family: ${addr?.tag}`);
}

/** Runs one request against the network worker, turning failures into throws. */
function ask(message, payload) {
  if (!client) unavailable();
  const { status, bytes } = client.request(message, payload);
  if (status === ERROR) {
    throw new Error(new TextDecoder().decode(bytes) || "the bridge failed");
  }
  return { status, bytes };
}

export class Network {}

export const network = { Network };

export const instanceNetwork = {
  instanceNetwork() {
    return new Network();
  },
};

export class TcpSocket {
  #id = nextId++;
  #connected = false;
  #closed = false;

  /**
   * WASI splits connecting in two so a guest can poll in between. Here the
   * whole thing happens in `startConnect`, blocking until the bridge says the
   * connection is open or says why it is not; `finishConnect` then just hands
   * over the streams. The engine cannot tell the difference — it calls them in
   * order — and pretending to be asynchronous would buy nothing, since the
   * thread is blocked either way.
   */
  startConnect(_network, remoteAddress) {
    if (this.#closed) throw new Error("socket is closed");
    ask({ op: "connect", id: this.#id, target: formatAddress(remoteAddress) });
    this.#connected = true;
    open.add(this);
  }

  finishConnect() {
    if (!this.#connected) throw new Error("finish-connect before start-connect");
    const id = this.#id;

    const input = inputStreamCreate({
      // Blocks until there are bytes or the peer closed. End of stream is
      // an empty result, not an error: the engine has to tell "done" from
      // "broken" (ADR-0019, Rule 2), and so does `wasi-grpc` above it.
      blockingRead(len) {
        const { status, bytes } = ask({ op: "read", id, max: Number(len) });
        if (status === EOS) return new Uint8Array(0);
        return bytes;
      },
      subscribe() {
        // Always ready, and the blocking read below is what actually waits.
        // A pollable that resolved only when data arrived would need this
        // thread to run a callback, which it cannot do while parked.
        return pollableCreate();
      },
      drop() {},
    });

    const output = outputStreamCreate({
      // The bridge buffers, so there is always room. Reporting a real figure
      // would mean asking it on every call for a number that only shrinks
      // under load the editor does not generate.
      checkWrite() {
        return 1n << 20n;
      },
      write(contents) {
        ask({ op: "write", id, length: contents.byteLength }, contents);
      },
      blockingWriteAndFlush(contents) {
        ask({ op: "write", id, length: contents.byteLength }, contents);
      },
      flush() {},
      blockingFlush() {},
      subscribe() {
        return pollableCreate();
      },
      drop() {},
    });

    return [input, output];
  }

  subscribe() {
    return pollableCreate();
  }

  [Symbol.dispose]() {
    this.drop();
  }

  drop() {
    if (this.#closed) return;
    this.#closed = true;
    open.delete(this);
    if (this.#connected && client) {
      try {
        ask({ op: "close", id: this.#id });
      } catch {
        // Closing something already gone is not worth reporting.
      }
    }
  }
}

/**
 * Closes whatever the guest left open, and is called when it exits.
 *
 * The engine states its verdict by exiting, and an exit runs no destructors: it
 * never drops its sockets, so nothing here ever sends the bridge a `close`.
 * Natively that costs nothing — the process ends and the kernel closes its
 * sockets. In the editor the guest is one short-lived instance inside a page
 * that stays up, so the relay on the far side of the bridge outlives it.
 *
 * That is worse than a leak. An executor that serves one connection at a time
 * — as the one built into anvil did until it was removed (ADR-0041) — is kept
 * busy for good by a connection a finished run left behind, and the next run's
 * connection waits in the accept queue for ever. That is the second Run that never returns (#61): the
 * engine blocks on a read the executor will never get to.
 */
export function closeOpenSockets() {
  // Copied first: `drop` removes from the set as it goes.
  for (const socket of [...open]) socket.drop();
}

export const tcp = { TcpSocket };

export const tcpCreateSocket = {
  createTcpSocket(_family) {
    if (!client) unavailable();
    return new TcpSocket();
  },
};

// Present so a component importing them still instantiates. The engine is a
// gRPC client and uses none of these; leaving them out would fail at
// instantiation rather than at the call, which is a worse place to find out.
export const udp = {
  UdpSocket: class UdpSocket {
    constructor() {
      unavailable();
    }
  },
};
export const udpCreateSocket = { createUdpSocket: unavailable };
export const ipNameLookup = { resolveAddresses: unavailable };

export { OK, EOS, ERROR };
