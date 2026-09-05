// `wasi:sockets` for a JavaScript host — deliberately unimplemented, for now.
//
// The shim's browser build cannot serve this: it still exposes the function
// style (`network` holds only `dropNetwork`) while the engine component asks
// for the resource style (`Network`, `TcpSocket`), so instantiation fails
// before any code runs. That is not a shim bug to work around; it is the
// platform fact ADR-0030 was written about — browsers have no TCP, and the real
// implementation must tunnel to the bridge over a WebSocket.
//
// Until the bridge lands, these exist so the component can instantiate and
// throw the moment anything actually reaches for the network. Which is never on
// the editor's current path: `--validate` opens no socket
// (crates/motor/src/bin/anvil.rs:288-305).
//
// They throw rather than return an error value on purpose. A stub that quietly
// answered "no route to host" would let the engine report a connection failure
// as if it had asked the world and been told no — the false red of ADR-0019's
// Rule 2. The host is what is missing here, not the network, and the two must
// not read the same.

const REASON =
  "wasi:sockets is not available in this host yet: the engine reaches executors " +
  "through the bridge over a WebSocket (ADR-0030), which is not implemented. " +
  "Nothing on the editor's current path should need it — --validate opens no socket.";

function unavailable() {
  throw new Error(REASON);
}

export class Network {
  constructor() {
    unavailable();
  }
}

export class TcpSocket {
  constructor() {
    unavailable();
  }
}

export const network = { Network };

export const instanceNetwork = {
  instanceNetwork: unavailable,
};

export const tcp = { TcpSocket };

export const tcpCreateSocket = {
  createTcpSocket: unavailable,
};

// Present so a component that imports them still instantiates; equally absent
// in practice.
export const udp = { UdpSocket: class UdpSocket { constructor() { unavailable(); } } };
export const udpCreateSocket = { createUdpSocket: unavailable };
export const ipNameLookup = { resolveAddresses: unavailable };
