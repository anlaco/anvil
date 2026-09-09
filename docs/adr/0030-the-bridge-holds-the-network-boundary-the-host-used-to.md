# ADR-0030: When the engine runs in a browser, the bridge holds the boundary the host used to

- **Status:** Accepted. **Not implemented** by this ADR.
- **Date:** 2026-09-05
- **How it was decided:** in this repo, while designing the graphical editor.
  Management chose to run the engine inside the browser tab, having been told
  in the same session that browsers have no TCP sockets and that this requires
  a local proxy; the choice was made with that on the record. Everything
  asserted about today's state is **verified by reading the code** in this
  session and cited with file and line. The claims about browser platform
  behaviour — no TCP API, and WebSocket's exemption from the same-origin
  policy — are **not contrasted with primary sources** in this session and are
  marked below.
- **Relates to:** ADR-0006, ADR-0011, ADR-0013, ADR-0019, ADR-0027, ADR-0029
- **Scope:** decides **who enforces the network boundary** when the engine is
  hosted somewhere that is not `packaging/anvil-host`. It does **not** change
  `wasi-grpc`, `paso.proto`, the WIT or the engine; it does **not** replace the
  native host, which stays the way Anvil is distributed and run (ADR-0011); it
  does **not** decide the editor's interface; and it decides **nothing** about
  what happens to energised hardware when the connection drops, which is a
  separate and still-open question.

## Context

The engine talks to executors over `wasi-grpc` — HTTP/2 on raw `wasi:sockets`
(ADR-0006). Today those sockets are wasmtime's, and wasmtime is where the
boundary lives. Verified on 2026-09-05:

- `packaging/anvil-host/src/main.rs:87-92` — `wasi_loopback()` grants the
  network and then installs `socket_addr_check(|addr| addr.ip().is_loopback())`.
  Nothing but `127.0.0.0/8` and `::1` gets out.
- `packaging/anvil-host/src/main.rs:96-106` — `wasi_loopback_con_declaradas()`
  relaxes that by exactly the non-loopback IPs the sequence declared.
- `packaging/anvil-host/src/main.rs:245-250` — the comment states the property
  the whole design rests on: *"nothing leaves loopback without being
  declared"*.

That check is enforced by **the host**, which is trusted code, on **the
guest**, which runs whatever a sequence says. Host and guest are separated by
wasmtime.

Running the engine in a browser tab removes wasmtime from that path. The
engine's `wasi:sockets` are then satisfied by a JavaScript shim, and the shim
tunnels to a local bridge process that opens the real TCP connections —
because *(not contrasted)* browsers expose no raw TCP API at all; WebSocket,
WebTransport and `fetch` are the whole menu, and none of them is a socket.

Two things follow, and the second one is the reason this ADR exists.

**The guard moves into the same context as the guarded.** A shim in the page
is not a sandbox boundary: it is code sitting beside the code it is supposed
to constrain. Whatever compromises the page compromises the check.

**The bridge is an open port on the developer's machine.** And *(not
contrasted)* WebSocket is not subject to the same-origin policy the way
`fetch` is: a browser will happily open `ws://127.0.0.1:PORT` from **any**
page the user has loaded, sending an `Origin` header that only the server can
act on. So any website open in another tab can attempt to speak to the bridge.
In a product whose whole job is to operate power supplies and RF generators,
an unauthenticated local port that reaches the bench is not a theoretical
finding.

## Decision

**The bridge enforces the boundary the host used to, and authenticates every
connection. The shim's check does not count.**

> **Extended by [ADR-0034](0034-the-engine-is-a-service-and-the-front-ends-are-clients.md)
> (2026-09-09):** nothing decided here is contradicted — whenever the engine is
> hosted outside `packaging/anvil-host`, which is still the browser case, the
> boundary lives at the bridge. What changes is the bridge's **job**. Here it is
> a byte relay that understands nothing of what it carries, existing so the
> tab's engine has a network. Once the engine can run natively as a service, the
> bridge is also the door to the engine — a better fit for something that
> already holds a token and is pinned to loopback. Note also that ADR-0034 §g
> puts the web IDE on `http://127.0.0.1` served by the engine itself, so the
> page and the bridge share an origin family they did not share here.

1. **The address check moves to the bridge, unchanged in meaning.** The bridge
   allows loopback plus exactly the non-loopback IPs declared in the
   sequence's `executors:`, reusing `ips_no_loopback_declaradas`
   (`packaging/anvil-host/src/main.rs:251`). The property of ADR-0011 survives
   verbatim: nothing leaves loopback without being declared. What changes is
   who says no.

2. **The shim checks too, and it is worth nothing.** Defence in depth is
   welcome, but the shim's check may never be the only one, and no reasoning
   about safety may rest on it. It is in the attacker's address space.

3. **Every connection is authenticated with a per-run secret.** The bridge
   mints a token when it starts, prints it, and refuses any WebSocket that
   does not present it. The editor gets it the way the person running both
   already gets everything else — from the terminal that started the bridge.
   An unauthenticated local port that reaches instruments is not acceptable
   even for one afternoon.

4. **`Origin` is checked and is not the defence.** The bridge rejects
   unexpected origins because it is free, and does not rely on it: a header is
   a request to be polite, not a boundary. The token is the boundary.

5. **The bridge binds loopback only.** No `--bind 0.0.0.0` equivalent. The
   remote-bench case belongs to the executors, which already have it
   (`executors/wasm/src/main.rs:818`), not to this.

6. **The page must be cross-origin isolated, and that is not optional.**
   Added on 2026-09-05, after building it. The engine's network calls are
   *synchronous* — `blockingRead` returns bytes, `Pollable.block()` returns
   nothing — and the only way to block a thread in JavaScript is
   `Atomics.wait` on a `SharedArrayBuffer`, which browsers withhold unless the
   page is cross-origin isolated. So the editor is served with
   `Cross-Origin-Opener-Policy: same-origin` and
   `Cross-Origin-Embedder-Policy: require-corp`, verified in Chrome: without
   them `SharedArrayBuffer` does not exist at all, and with them a worker
   blocks and is woken by another thread as intended.

   The price is that every cross-origin resource then needs CORP/CORS headers,
   and the editor cannot be embedded in a page that is not itself isolated.
   The editor embeds everything it uses, so it pays nothing today; what it
   gives up is being embeddable in someone else's application later.

7. **The bridge refuses to be a general-purpose tunnel.** It carries gRPC to
   declared executors and nothing else. It is not a SOCKS proxy that happens
   to be written in Rust, and the temptation to make it one — "just let it
   connect anywhere, the editor knows what it is doing" — is the whole finding
   above, rewritten as a feature.

8. **The native host stays the reference.** `packaging/anvil-host` remains how
   Anvil is distributed and run (ADR-0011), and its boundary stays where it is.
   This ADR governs an additional host, not a replacement. A capability the
   browser path cannot honour is a reason to keep work in the native path, not
   a reason to weaken the native path to match.

## Alternatives discarded

**Give `wasi-grpc` a `Transport` trait and a WebSocket implementation.**
Architecturally the clean answer, and ADR-0011 §Recortes already fingers the
gap: `Cliente`/`Servidor` are built only from `TcpSocket`/`InputStream`/
`OutputStream`. Rejected for now on scope — `wasi-grpc` is a separate repo with
its own Apache licence and its own consumers, and this would change its public
surface for a reason internal to Anvil's editor. Faking `wasi:sockets` in the
shim keeps the change on our side of the fence and leaves the engine's code
untouched. Worth revisiting if the shim proves unmaintainable.

**Serve the engine from the native binary instead (`anvil --serve`).** Removes
this ADR entirely: the boundary stays in wasmtime where it already works, and
the browser only paints. It was put to management with this trade-off stated
and not chosen. It remains the fallback if the shim does not hold up.

**No token, loopback is enough.** The assumption that "it's only on localhost"
is exactly what the WebSocket exemption defeats. Rejected on the strength of
what is on the other end of the wire.

## Consequences

- **A security-relevant component now exists that did not before**, and it is
  the kind that ages badly if nobody owns it. The bridge deserves its own tests
  for the negative cases — wrong token, undeclared IP, unexpected origin — and
  those tests must be seen to fail before they are believed, per the house rule.
- **Two hosts now enforce the same property in two places.** The address check
  exists in `packaging/anvil-host` and again in the bridge. They must share
  `ips_no_loopback_declaradas` rather than reimplement it, or they will drift,
  and a drift here means one host allows what the other forbids.
- **Guarantees are lost and should not be quietly assumed.** A WebSocket is not
  a TCP socket: half-close semantics, timeouts and backpressure all differ, and
  `wasi-grpc` has no deadlines to begin with
  (`crates/ejecutor_pasos/src/main.rs:122-124`). A step that hangs will hang
  differently here.
- **The failure modes of a browser are now Anvil's failure modes**: a closed
  tab, a sleeping laptop, a revoked file permission. None of them exists in the
  native path, and what they mean for a bench mid-run is **not decided by this
  ADR** and must be before anything runs unattended.
- **The report must say which host ran the sequence.** Under Rule 3 of
  ADR-0019, what alters the conditions of a measurement is written down — and
  which host held the boundary is exactly that.
