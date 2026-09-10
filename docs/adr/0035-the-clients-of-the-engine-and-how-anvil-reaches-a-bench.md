# ADR-0035: The clients of the engine, and how Anvil reaches a bench

- **Status:** Accepted. **Not implemented** by this ADR. None of it exists: the
  engine has no control interface (ADR-0034 is also unimplemented), there is no
  operator interface of any kind, and `packaging/package.sh` registers no file
  type — it assembles a tarball of two static musl binaries, the example
  sequences and a department (`packaging/package.sh:1-8`).
- **Date:** 2026-09-10
- **How it was decided:** **by management**, in the design conversation that
  continued straight from ADR-0034. That ADR settled that the engine becomes a
  service; this one settles **who its clients are, what each of them can do, and
  how Anvil physically gets to a bench** — questions that only surfaced once the
  service existed on paper, one of which (a tablet away from the bench)
  contradicts a decision ADR-0034 had just made (§7a). Everything asserted about
  **this repo** is **verified by reading the code in this session** and cited
  with file and line. Everything about **other products** is **second-hand**,
  taken from research with primary-source URLs, **not exercised here**, and
  marked at each point.
- **Relates to:** ADR-0002, ADR-0005, ADR-0011, ADR-0019, ADR-0029, ADR-0031,
  ADR-0033, ADR-0034, [#62](https://github.com/anlaco/anvil/issues/62),
  [#63](https://github.com/anlaco/anvil/issues/63),
  [#66](https://github.com/anlaco/anvil/issues/66),
  [#67](https://github.com/anlaco/anvil/issues/67)
- **Scope:** decides **the two client profiles and what separates them, what the
  operator interface is and where it lives, where each client runs, how Anvil
  reaches a bench, and what wraps the IDE**. It does **not** design the protocol
  — that is [#63](https://github.com/anlaco/anvil/issues/63) and its own ADR; it
  does **not** change the engine, `paso.proto`, the WIT or the sequence format;
  it does **not** decide the IDE's visual design; and it **explicitly defers**
  two things rather than deciding them: parallelism inside a sequence (§6e) and
  an embedded browser for the IDE (§6d).

## Context

ADR-0034 left the engine as a service with one kind of client implied: a front
end that debugs. Pressing on it produced three facts it had not accounted for.

**A bench and a desk are not the same room.** The person who writes a sequence
and the person who runs it are different people with different needs, and the
second one may be holding a tablet several metres from the machine. TestStand
splits exactly here — Sequence Editor on one side, Operator Interface on the
other — and ships the operator interface's source for customers to copy
(second-hand: [NI](https://www.ni.com/en/support/documentation/supplemental/08/teststand-user-interface-development-best-practices.html)).

**A tablet is not loopback.** ADR-0034 §g put the web IDE on `http://127.0.0.1`
precisely because loopback-to-loopback is exempt from Chrome's Local Network
Access. A tablet talking to the bench machine crosses the local network, which
is what that restriction is about, and loses the secure context along with it.

**Anvil reaches a bench on a memory stick.** Not through a package manager, not
through an installer with a network behind it. The engine is already a single
statically linked binary of about 35 MB (`packaging/anvil-host/target/release/anvil`,
35 392 664 bytes) that embeds both guests
(`packaging/anvil-host/src/main.rs:59-60`). Whatever the bench needs has to be
inside it.

Two things already in the repo turned out to matter more than expected.
`editor/src/run-state.mjs` is a pure state machine over the event stream, kept
out of the DOM on purpose, and it already stores the full declared `plan`
(`run-state.mjs:32,75`, from `sequence_start`, ADR-0033 §4b) — so a client can
render a whole sequence knowing nothing about it in advance. And the editor
currently reads and writes files through the browser
(`editor/src/app.mjs:675-676,716-720`), which is why it needs a fallback to
downloading on anything that is not Chromium.

## Decision

**a. The protocol has two profiles, not two protocols: control and operation.**
*Control* is the IDE's: breakpoints, pause, step, inspect, force a result.
*Operation* is the bench's: start, stop, watch, and answer the engine when it
asks the operator something. They are authorised separately, because starting a
run means energising hardware and that is not the same permission as watching
one.

**b. The operator interface is generic, and it renders itself.** One web
application, written and maintained here, that works for any sequence because it
learns the sequence from `sequence_start`'s `plan` when it connects. It is a
product, not a template to be copied.

**c. What is published for someone who wants their own is the protocol, not our
source.** TestStand hands out the operator interface's code because it has no
protocol to hand out; the result is that every customer maintains a derivative
and NI maintains the template they all came from. With a documented contract,
a third party writes a client and copies nothing.

**d. The operator interface ships inside the engine's binary**, the way both
guests already do. Arriving on a memory stick means one file, not "copy this
folder and do not leave anything behind".

**e. The engine owns the files; the browser never touches disk.** A sequence
reaches the engine as an argument or over the protocol, and the IDE asks the
engine to open and save. This removes the File System Access dependency at
`app.mjs:675-720` and makes the IDE work the same on any browser.

**f. Loopback is the default, not the rule, and there are two switches.** The
engine listens on loopback unless told otherwise when it starts. Exposing
*operation* to the network (the tablet) and exposing *control* to the network
(an engineer debugging against the bench from their laptop) are two separate
decisions, and both are off by default.

**g. The bench gets the engine and the operator interface. The IDE is never
installed there.** Debugging against real hardware happens from the developer's
machine over the control profile, which is what the OpenTAP Runner does with a
remote session URL (second-hand: [doc.opentap.io/runner](https://doc.opentap.io/runner/)).
This replaces "install the IDE on the bench during development and remove it
before locking the system down" — the system that gets validated should be the
system that ran.

**h. One session, one process.** Each run is a separate engine process. This is
how several units under test run at once **without the engine gaining threads**:
it stays the single-threaded `wasi:cli/run` command it is today, and the service
orchestrates. It also contains a failure: a sequence that brings the engine down
takes its own unit with it and nothing else.

**i. The IDE is wrapped by the browser, not by a framework.** The engine serves
the page and opens it in the browser's application mode; the browser can install
it, which gives a window with no address bar, an icon and a menu entry.
Double-clicking a sequence is a **packaging** matter — a `.desktop` file with
its `MimeType` on Linux, registry entries on Windows, a thin `.app` bundle on
macOS — not a reason to adopt a UI framework.
> **Narrowed by [ADR-0036](0036-the-sequence-editor-is-wrapped-by-tauri-not-the-browser.md)
> for Windows**: browser-application-mode assumes a capable, already-installed
> browser on a machine this project does not control, which does not hold for
> "someone downloaded the editor" the way it holds for a developer's own
> computer. The Windows Sequence Editor is wrapped by Tauri instead.

## Alternatives rejected

- **Embed Chromium in the IDE (Electron or CEF).** Deferred, not condemned: it
  is what gives a consistent window everywhere. Rejected **for now** because it
  is 150–200 MB per platform and a standing commitment to ship Chromium's
  security patches — Electron releases every few weeks for that reason — and
  because §i already delivers a clean window, an icon and a double-click. The
  cost of deferring is near zero: the IDE is a client of the protocol, so
  wrapping it differently later changes the wrapper, not the architecture.
- **Tauri.** Lighter than Electron because it uses the system webview, and in
  Rust like the rest. Deferred on the same reasoning, plus an artefact to build,
  sign and update per platform and three different webviews to support.
  > **Reversed for Windows by
  > [ADR-0036](0036-the-sequence-editor-is-wrapped-by-tauri-not-the-browser.md)**:
  > the reasoning here still holds and is accepted as a real cost, but it is
  > outweighed by a reliability requirement §i did not account for.
- **One executable containing Chromium and wasmtime.** Rejected outright, not
  deferred. It fuses the IDE and the engine back into a single artefact, which
  is what ADR-0034 separated, and it forces a production bench to carry a
  browser it never opens — 200 MB of attack surface for nothing.
- **The operator interface compiled together with the sequence.** Rejected: it
  makes code travel with data, so whoever sends a sequence executes code on the
  bench machine. It is also the TestStand model, where every customer ends up
  maintaining a program.
- **The operator interface declared inside the sequence**, rendered by a generic
  client — which would fit ADR-0002 and ADR-0005 well. Rejected because a
  declarative interface always falls short somewhere and the escape hatch is how
  code gets back in. (b) gets the same benefit differently: generic because it
  reads the plan, not because the sequence describes a screen.
- **Keeping the control profile on loopback only**, which was the simpler shape
  proposed earlier in the same conversation. Rejected because debugging against
  the real bench from a laptop is worth more than the simplification — with the
  consequence, stated plainly, that the profile that can energise hardware is
  the one that goes over the network.

## Consequences

**a. ADR-0034 §g is narrowed and is annotated there.** Loopback was written as
the rule; it is the default. Both profiles can be exposed deliberately.

**b. The part that has to be secured properly is the control profile.** It can
start a run, force a step's result and stop one. It is also the one that will
cross a factory network, where HTTPS has no pretty answer — a local network has
no valid certificate and the alternatives are installing a CA on every client or
shipping a private key. **Unresolved here**, and it should not be discovered
during deployment.

**c. `run-state.mjs` becomes shared code.** It is the operator interface's engine
as much as the editor's, and it is already written to be testable without a
browser. Where it lives has to change so both can use it, without forking it —
a second implementation of "which step is running" is exactly what ADR-0031
forbids.

**d. Packaging grows two jobs**: registering the file type per platform, and the
single-instance problem that comes with it. Double-clicking a second sequence
while one engine is running must open it in the one that is running, not start a
second engine on a second port. That makes "open this file" a protocol message.

**e. The default port must be stable, which pulls against
[#67](https://github.com/anlaco/anvil/issues/67).** An installed web application
is identified by its origin, and the origin includes the port; an ephemeral port
makes every launch a different application and loses whatever it stored. A fixed
default with a fallback, and the connection file for discovery, is the shape
that has to be reconciled there.

**f. [#62](https://github.com/anlaco/anvil/issues/62) is reached from a new
direction.** Several engine processes (§h) against one step executor is the same
one-connection-at-a-time limit, arrived at without any parallelism inside a
sequence.

**g. Parallelism inside a sequence is deferred, explicitly.** Multi-UUT by
processes covers what exists today. The expensive half — threads inside the
engine, hierarchical cancellation to keep cleanup guaranteed, and a vocabulary
for instruments shared between steps
([#66](https://github.com/anlaco/anvil/issues/66)) — waits for a case that
needs it.

**h. Traceability is an open problem this ADR creates.** If sequences arrive on
a memory stick, nothing yet says which version of the sequence and which version
of the engine produced a given report. Rule 3 of ADR-0019 makes that a defect:
the version of what ran is the first thing that alters the criterion.
