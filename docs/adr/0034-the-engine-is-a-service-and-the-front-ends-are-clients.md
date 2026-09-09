# ADR-0034: The engine is a service, and every front end is a client

- **Status:** Accepted. **Not implemented** by this ADR. Nothing here exists
  yet: the engine has no control interface, no pause and no cancellation —
  `grep -c 'fn abort\|fn cancel\|fn pause' crates/motor/src/lib.rs` returns 0,
  and `docs/diseno/motor-de-ejecucion.md:141,145` lists "Debugger visual" under
  *No incluye (post-MVP / out-of-scope)*.
- **Date:** 2026-09-09
- **How it was decided:** **by management**, in this repo, in a design
  conversation about where the engine should live once the editor grows a
  debugger. The question was raised as "IDE with an engine inside, or IDE that
  talks to an engine running natively", and settled on the second after four
  web research passes. Everything asserted here about **today's state of this
  repo** is **verified by reading the code in this session** and cited with file
  and line. Everything about **other products** (TestStand, OpenTAP, DAP,
  Jupyter, Chrome's Local Network Access) is **second-hand**: it comes from
  research agents with primary-source URLs, was **not** exercised here, and is
  marked as such at each point.
- **Relates to:** ADR-0006, ADR-0011, ADR-0019, ADR-0029, ADR-0030, ADR-0031,
  ADR-0033, [#62](https://github.com/anlaco/anvil/issues/62),
  [motor-de-ejecucion.md](../diseno/motor-de-ejecucion.md)
- **Scope:** decides **that the engine stops being only a command and gains a
  control interface of its own, that every front end reaches it through that
  interface, and where the engine is allowed to stop**. It does **not** design
  the protocol message by message — that is a later ADR with the wire in it; it
  does **not** change `paso.proto`, the WIT, the YAML sequence format or the
  execution semantics; it does **not** remove the engine from the browser tab,
  which stays and is required (§2); it does **not** decide the visual debugger's
  interface; it does **not** implement anything; and it decides **nothing**
  about parallelism inside a sequence beyond naming it as the thing that forces
  the vocabulary this ADR declines to invent (§6c).

## Context

### 1. Today the engine is a command, and that is why the editor has to contain it

The engine is a `wasi:cli/run` guest: it takes argv, writes to stdout and
stderr, and states its verdict by exiting — the exit path is explicit at
`crates/motor/src/bin/anvil.rs:207` and `:423`, both commenting on how
`exit(n≠0)` becomes `I32Exit(1)` when it crosses `wasi:cli/run`. The native host
embeds it, together with the step executor, as two guests compiled into the
binary (`packaging/anvil-host/src/main.rs:59-60`).

That shape has one consequence that drives this whole ADR: **the only way to use
the engine is to instantiate it and read what it prints.** So the editor does
exactly that. It hosts the guest itself and takes the run's progress off the
guest's stderr, line by line, through an `onStderrLine` callback
(`editor/src/engine.mjs:145,149`, wired to the interface at
`editor/src/app.mjs:586`). It works, and it is a workaround: stderr is shared
with the executors, so `editor/src/run-state.mjs:50` says in as many words that
"foreign text is expected, not exceptional" and the parser filters it.

`--events` (ADR-0029, ADR-0033) already made that channel structured —
`crates/motor/src/bin/anvil.rs:123` accepts the flag today, which is a change
from ADR-0033's own status line, written before it existed. But it is one
direction only. **The engine narrates; nothing can answer.**

### 2. The browser tab is not negotiable, and neither is the native host

Two requirements pull in opposite directions and both are real.

Someone must be able to open a page and try Anvil with nothing installed, and
that has to mean *running* a sequence, not just editing YAML — a demo where the
debugger does not run demonstrates nothing. There is no native process on that
machine, so the engine must be in the tab. It already is, as the same WebAssembly
core the CLI runs (ADR-0031), and the step executor is a guest too
(`main.rs:59-60`), so the simulated bench can join it there.

And with real hardware, the engine should be under wasmtime in a process the
operator controls, not under a browser tab. Today, with the engine in the tab,
five of the seven WASI shims it runs on are a third-party dependency —
`editor/src/wasi/{cli,clocks,filesystem,io,random}.mjs` are one-line re-exports
of `@bytecodealliance/preview2-shim` — and the other two, the socket shim and
its channel, are ours. Issue #61 was a defect of that host, not of the engine:
with no process, nobody closed what the guest left open.

So the engine runs in two places for good reasons, and the question this ADR
answers is not *where* but *how anything talks to it*.

### 3. What a debugger needs, and what exists

The stated purpose of the graphical IDE is debugging a sequence the way a
specialised test executive does: see state, pause, continue, force a step.
None of that exists in the engine, in either location. What the editor calls
abort today is killing the worker thread, and `editor/src/engine-pool.mjs:149`
already says why that will not survive contact with a bench: *"Terminating the
thread is the only abort available: the engine has no cancellation of its own"*,
and doing it mid-sequence leaves the bench as it was, with no `cleanup` run.

There is also a naming debt already contracted. `pause_on_fail` exists in the
sequence format and does **not** pause: it is a `break`
(`crates/motor/src/lib.rs:759-761`). It carries TestStand's name without
TestStand's behaviour, and the day a real pause exists the two meanings
collide.

### 4. What the field did — second-hand, with sources

None of the following was exercised in this session.

- **NI TestStand keeps the engine in-process.** It is a COM/ActiveX automation
  server each application loads into its own process; the extension model is
  that NI ships the operator interface's source for you to copy. Its gRPC
  remote API exists as an "Early Access" example that translates the COM object
  graph one class at a time, with `Get_`/`Set_` per property and object handles
  the client must free by hand; it has not been touched since October 2023.
  ([ni/grpc-teststand-api](https://github.com/ni/grpc-teststand-api),
  [TestStand API](https://www.ni.com/en/support/documentation/supplemental/08/programming-with-the-teststand-api.html))
- **Keysight's OpenTAP has both, and added the service second.** The core is a
  library loaded in-process; on top of it they built the **OpenTAP Runner**, a
  daemon that exposes sessions over NATS with JSON payloads, spawning each run
  as a separate process, with clients in C#, TypeScript and Python. Its
  documentation gives as the reason to use it: remote interaction, clients in a
  language other than C#, and cloud. ([doc.opentap.io/runner](https://doc.opentap.io/runner/))
  The Runner's debug surface is the closest thing to a specification of what is
  needed here: breakpoints by step id, a `Breaking` state with a break event,
  resume, step, jump-to-step, abort — plus **operator input requests travelling
  over the protocol**, and a **session watchdog**.
  In-process OpenTAP, by contrast, has no pause API at all; the maintainers'
  answer is to sleep the thread from an event handler, and they state the limit
  plainly: you can only stop between steps.
  ([forum](https://forum.opentap.io/t/how-to-pause-a-testplan-and-resume-the-testplan/960))
- **Nobody stops inside a step.** TestStand's own documentation says that even
  Abort waits: if a call to a step module is active, the execution waits for
  that module to return.
- **DAP is a façade everywhere it is used outside plain code.** Jupyter wraps
  DAP inside its own messaging protocol and had to add custom requests for
  cells, which have no source file (JEP 47); the VDM toolchain uses DAP for
  debugging and invented a second protocol for everything else. DAP's
  `Capabilities` is a **closed set** with no slot for declaring an extension,
  and the request to carry custom data on a breakpoint was closed as not
  planned ([DAP #445](https://github.com/microsoft/debug-adapter-protocol/issues/445));
  DAP also has no routing for several concurrent targets through one adapter
  ([DAP #79](https://github.com/microsoft/debug-adapter-protocol/issues/79)).
- **"Write DAP once, get every editor" does not hold.** RobotCode — same shape
  of domain, better funded — ships debugging in VS Code only and says so in its
  own Neovim documentation; its predecessor is unmaintained. Geany does not
  speak DAP at all. What is cheap is the VS Code side: a native binary can be
  the debug adapter directly over stdin/stdout, with a manifest-only extension.
- **A public page may no longer reach `127.0.0.1`.** Chrome 142 shipped Local
  Network Access: a public-origin request to loopback raises a permission
  prompt, and denial fails silently — Dell had to publish a support note when
  SupportAssist broke from dell.com. **Loopback-to-loopback is exempt by
  specification.** WebSockets were not gated in the initial launch but are
  announced to follow.
  ([Chrome](https://developer.chrome.com/blog/local-network-access),
  [WICG spec](https://wicg.github.io/local-network-access/))

## Decision

**The engine gains a control interface of its own, and every front end — the
graphical IDE, a VS Code plugin, anything later — is a client of it.**

Seven parts, and only these are decided here.

**a. The interface is Anvil's own, in Anvil's vocabulary.** Steps, phases,
measurements, limits, verdicts, retries, instrument state. Not DAP's threads,
stack frames and scopes. The reasons are in §4: DAP cannot be extended in a way
a foreign client would see, and everything that makes Anvil a test sequencer
rather than a code debugger has no vocabulary in it.

**b. It is commands and events, never a remote object graph.** The event
direction already exists and keeps its shape: the NDJSON of ADR-0029 and
ADR-0033, with the execution identity of ADR-0033 unchanged. What is added is
the direction back. A property-by-property object API is rejected explicitly
(§Alternatives).

**c. DAP is a façade over it, not the native tongue.** Written later, as a
translator, and aimed at VS Code specifically rather than at "every editor".
Step breakpoints map onto DAP's function breakpoints, which require only a name.

**d. The engine may only stop at step boundaries.** A pause requested elsewhere
is honoured at the next boundary, and the time until then is the duration of the
step in flight. That latency is **part of the interface and must be visible**:
"pause requested" and "paused" are different states and the front end shows
both. Stopping inside a step would mean stopping with an instrument half
configured, which is the failure this project exists to prevent (ADR-0019).

**e. Cleanup is shielded.** A stop request that arrives while `cleanup` is
running does not interrupt it. Terminating and aborting are two different verbs
with two different promises, and the difference is whether `cleanup` runs.

**f. One client holds control; the others observe.** Events are broadcast to
every connected client; the commands that move the execution or touch the bench
belong to a single holder at a time. This is not inherited from any protocol we
looked at — LSP assumes one client, DAP one session, and VS Code's Live Share
lets any guest step. For a sequencer, two clients able to start a run against
the same power supply is a physical safety defect, not a concurrency one. And
the engine keeps a watchdog: if the controlling client disappears, what happens
to energised hardware is a decision the engine makes, not an accident.

**g. When there is a bench, the engine serves the web IDE itself, from
loopback.**

> **Narrowed by [ADR-0035](0035-the-clients-of-the-engine-and-how-anvil-reaches-a-bench.md)
> (2026-09-10):** loopback is written here as the rule; it is the **default**.
> An operator interface on a tablet, and an engineer debugging against the bench
> from their own machine, both cross the local network — so ADR-0035 §f keeps
> the engine on loopback unless told otherwise at start-up, and makes exposing
> *operation* and exposing *control* two separate switches, both off by
> default. What does not change: when both ends are on one machine, this is the
> shape, and it is what avoids Chrome's Local Network Access prompt.
 A page served from `http://127.0.0.1:<port>` talking to the engine
on the same host is loopback-to-loopback and exempt from Chrome's Local Network
Access; it is also a secure context, so no mixed content and no certificates.
GitHub Pages stays the home of the pure-WASM demo, which touches nothing local.

**The library does not go away.** `anvil sequence.yaml` stays a command that
runs and exits (ADR-0011): the service is an additional way in, not a
replacement. OpenTAP has both, and the only third-party editor in that
ecosystem exists *because* the engine was loadable in-process.

## Alternatives rejected

- **Leave it as it is: engine in the tab, bridge as a cable.** Costs nothing and
  is what runs today. Rejected as the destination, not as the present: it gives
  the debugger nothing to stand on, and it puts the engine on a JavaScript host
  when there is voltage on the bench. It remains what ships until the service
  exists.
- **Make DAP the engine's native protocol.** Rejected on §4: closed capability
  set, no declarable extensions, single session against a multi-UUT future, and
  no vocabulary for a measurement or a verdict. Every precedent examined ended
  up with a protocol of its own and DAP on top.
- **Expose the engine as a remote object graph** — a service per class, a getter
  and a setter per property, handles with client-managed lifetime. This is
  precisely NI's gRPC API, and its state (untouched since 2023, "subject to
  change without notice") is the argument. It turns every field read into a
  round trip and hands the client a distributed garbage-collection problem.
- **gRPC-Web or Connect for the browser leg.** gRPC-Web needs a proxy and has no
  bidirectional streaming; both put status in HTTP trailers, and this project
  already knows what that costs — `wasi-grpc` v0.1 not understanding
  trailers-only was the real cause of #58. A local binary running a proxy to
  talk to itself is cost with no return.
- **Serve the web IDE from GitHub Pages and have it reach a local engine.**
  Rejected on §4: it is one permission prompt per user, worded as "access other
  apps and services on this device", in an industrial setting whose default
  answer is no.
- **Keep the engine in the tab even with hardware, and reach the bench through
  the bridge.** This is today's shape and the honest alternative to (g). It
  keeps one execution path instead of two and puts the run's state next to the
  UI, which is a real advantage for a debugger. Rejected because the host
  underneath is a browser tab and five third-party shims, because a transpiled
  core has no threads, and because closing a tab is a gesture a person makes by
  accident.

## Consequences

**a. There is a new public surface, and it is the expensive kind.** Once a VS
Code plugin or a third party speaks it, it cannot be moved. The mitigation is
the one LSP uses and this ADR adopts: a handshake that negotiates capabilities,
where an unknown capability is ignored rather than fatal. There is a concrete
warning from the field: OpenTAP Runner users lost hours because the protocol was
not specified to the byte and the reference client became the specification.

**b. ADR-0031 is narrowed, and knowingly.** "One engine, two front ends" stays
true of the artefact — it is the same core module in both places, and no second
implementation is permitted. What changes is that the two hosts will not offer
the same **capabilities**: parallelism inside a sequence needs threads the
transpiled core does not have, so the browser will be a subset. That divergence
is deliberate and is declared in the handshake, not discovered by the user.

**c. ADR-0030 changes what the bridge is for.** Today the bridge exists to give
the tab's engine a network, and it is a byte relay that understands nothing of
what it carries. In the shape decided here, with the engine native, it becomes
the door to the engine — which is a better job for something that already
holds a token and is pinned to loopback. Nothing in ADR-0030 is contradicted:
the boundary still lives at the bridge whenever the engine is hosted outside
`packaging/anvil-host`, which is still the browser case.

**d. Issue #62 stops being a fragility and becomes a blocker.** The step
executor serves one connection at a time. Two steps running in parallel against
one executor is exactly the case that hangs there with no diagnostic.

**e. Pausing needs a vocabulary this ADR does not invent.** Once a run can stop
and be resumed, and once steps can run in parallel, the sequence must be able to
say that two steps share an instrument. TestStand offers named locks over an
arbitrary resource plus batch synchronisation; OpenTAP offers a lock step with a
name and no timeout. In neither does a step *declare* which instrument it uses
for the engine to infer exclusion. That is a design opportunity and a separate
ADR.

**f. `pause_on_fail` is now a name with a deadline.** It does not pause
(`lib.rs:759-761`). Today, in alpha and with no users, renaming it costs a
commit. Once there are sequences in the field it costs a migration.

**g. Rule 3 of ADR-0019 extends to the debugger.** Forcing a step's result,
skipping one, or editing a variable mid-run **alters the criterion**, and
therefore has to reach the report. A run that was interfered with and does not
say so is a run whose measurements cannot be reconstructed — the same defect
class as a run that reports nothing.

**h. A long-lived local service is a liability that has to be designed, not
packaged.** Validating both `Host` and `Origin`, authenticating even on
loopback, an ephemeral port with a connection file rather than a fixed one, and
a clean uninstall. The 2025-26 crop of MCP DNS-rebinding advisories is this
exact architecture failing in four languages, and Zoom's 2019 local server —
which survived uninstalling the client — is what the worst case looks like.
