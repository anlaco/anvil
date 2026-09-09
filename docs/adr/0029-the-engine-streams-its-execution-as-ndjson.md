# ADR-0029: The engine streams its execution as NDJSON on stderr

- **Status:** Accepted. **Not implemented** by this ADR.
- **Date:** 2026-09-05
- **How it was decided:** in this repo, while designing the graphical editor.
  Management asked for a Run button that highlights the running step as it
  advances, the way TestStand's execution window does. Everything asserted
  about today's state is **verified by reading the code** in this session and
  cited with file and line.
- **Relates to:** ADR-0011, ADR-0015, ADR-0019, ADR-0021,
  [motor-de-ejecucion.md](../diseno/motor-de-ejecucion.md),
  [reportes.md](../diseno/reportes.md)
- **Scope:** decides **how execution progress leaves the engine**. It does
  **not** change the execution semantics, the `ResultSink` lifecycle, the
  verdict, `paso.proto` or the WIT; it adds **no import** to the engine guest's
  world; it does **not** let anything drive the engine — this is one-way, out
  only — and it does **not** design the editor.

## Context

The engine already publishes every event a live view needs. Verified on
2026-09-05 in `crates/modelo/src/result_sink.rs:16-22`, the `ResultSink`
lifecycle is:

```text
on_inicio_secuencia(def)
  on_inicio_paso(paso)
  on_resultado(resultado)
  on_fin_paso(paso)
on_fin_secuencia(resultado)
```

and the module's own doc says the streaming hooks *"se disparan igual y quedan
listos para sinks de log/UI en vivo futuros; los sinks de formato los
ignoran"* (`result_sink.rs:33-37`). So the events exist, fire at the right
moments, and are deliberately unused.

What is missing is a way out of the process. The three sinks that exist render
in `on_fin_secuencia` — console, JSON and CSV all wait for the aggregate
verdict, because the frozen header needs it (`result_sink.rs:28-33`). And the
engine is a `wasi:cli/run` guest that starts, runs and exits
(`crates/motor/src/bin/anvil.rs:171-403`): `--json` and `--csv` write files at
the end (`anvil.rs:311-318`). Nothing observes a run in flight.

This is the last piece the editor needs and the one with the widest blast
radius if it is got wrong, because there are three different things that host
the engine today and all three must keep working: the native host with
wasmtime embedded (ADR-0011), the `wasmtime` CLI kept for debugging
(ADR-0011 §Decisión), and — new — a JavaScript host in a browser tab.

## Decision

**A new sink writes one JSON object per line to stderr, enabled by `--events`.**

1. **NDJSON, one object per line.** Newline-delimited JSON: each event is a
   complete object on its own line, so a reader can act on each line as it
   arrives without waiting for a closing bracket. A JSON array could not be
   streamed without being a lie until its last byte.

2. **On stderr, not stdout.** stdout belongs to `SinkConsola`
   (`anvil.rs:359`), and `result_sink.rs:44-46` already states the rule:
   sinks log to stderr, *"nunca a stdout, que es territorio del sink de
   consola"*. Events on stderr compose with everything: they do not corrupt
   the human report, they need no `--quiet`, and both existing hosts already
   inherit stderr (`packaging/anvil-host/src/main.rs:89`).

3. **`--events` survives `--quiet`.** `--quiet` silences the console report and
   the stderr logs (`anvil.rs:115`, `anvil.rs:425-429`). It does not silence
   events: the two flags answer different questions, and a UI wanting events
   without console noise is the expected combination, not a contradiction.

4. **One field decides the shape: `event`.** Each line carries an `event` key
   naming which lifecycle hook produced it — `sequence_start`, `step_start`,
   `step_result`, `step_end`, `sequence_end` — and the fields that hook has.
   The payloads reuse the JSON sink's vocabulary verbatim
   (`crates/result_sink/src/json.rs:77-106`): same field names, same value
   encoding, same treatment of a `Reference`. Two renderings of the same fact
   must not drift.

5. **An unknown `event` is skipped, not fatal.** A reader that meets an event
   name it does not know ignores that line and keeps going. This is what lets
   events be added later without a version on the wire — and it is why this
   channel carries **no verdict of its own**: the verdict is the exit code and
   the report, exactly as today. A reader that misses a line must not be able
   to conclude anything false about the unit, which is Rule 2 of ADR-0019.

6. **Best-effort, like every sink.** `ResultSink` returns no `Result` on
   purpose (`result_sink.rs:39-46`): a sink that cannot write does not break
   the run. A blocked or closed stderr must never stop a sequence with a unit
   on the bench.

> **Extended by [ADR-0033](0033-identity-on-the-event-stream-is-the-execution.md)
> (2026-09-09):** points 4, 5 and 6 are completed there, and **two** sentences of
> the Scope above are contradicted. In short: the `event` key alone does not say
> *which* step a line is about, and the answer is that **identity is the
> execution, not the step** — `run_id`, `step_run_id` and `parent_run_id`, minted
> by the engine, with the name and the position demoted to attributes. Point 5's
> permission to skip lines and point 6's best-effort delivery are only honourable
> if loss is *detectable*, so the envelope gains a monotonic `seq`, allocated
> before each write is attempted — and, because a gap needs a line on both sides
> and an aborted run never reaches `on_fin_secuencia`, a `plan` on
> `sequence_start` to say what was declared. Point 5's lack of a version field is
> patched where it does not stretch: a `final` flag and an `events_version`. The
> forgery that a shared fd 2 allows is fixed at its source rather than by a check
> on the wire.
>
> Contradicted: **"does not change the `ResultSink` lifecycle"** — the hook
> signatures change, though the hook order does not; and **"adds no import to the
> engine guest's world"** — it adds `wasi:random/random`, a `wasi:cli`-world
> standard, so the property that made this ADR reject a *private* import (the
> guest stays runnable by any host) is intact, but the promise as worded is not.
>
> Also corrected there: point 6's "a blocked or closed stderr must never stop a
> sequence" **cannot be delivered from the guest** — in the pinned
> `wasmtime-wasi`, the stdio stream's `check_write` returns a constant permit and
> `write` blocks. ADR-0033 §4a states the real exposure instead.

7. **The channel is one-way.** Nothing on it can pause, resume, abort or
   otherwise steer the engine. Driving the engine is a separate contract, is
   what breakpoint debugging will need, and is deliberately not decided here.

## Alternatives discarded

**An import in the engine guest's world (`anvil:editor/events`).** The
original shape in the editor plan: the host implements an interface and the
sink calls it. It was rejected on discovering what it costs. The engine has no
WIT of its own today — the only `.wit` files in the repo are `anvil-step.wit`
for the step (`executors/wasm/wit/`, `executors/rust/anvil-step/wit/`) — and
the engine is a plain WASI component. Giving it a private import makes it
unrunnable by any host that does not implement it: `wasmtime run` stops
working, and `packaging/anvil-host` has to grow an implementation it has no
use for. A line on stderr costs none of that and is readable by all three
hosts as they already are.

**A file, like `--json` and `--csv`.** Symmetric with what exists, and it does
work in the browser, where the "file" is a shim the host can watch. But it
makes every other reader poll a growing file, and it turns "watch a run" into
a filesystem problem for the two hosts that have a perfectly good stderr.

**A socket the editor connects to.** Most flexible, and the natural home for
the control channel later. But it makes the engine a server, needs a port and
a lifetime and a shutdown, and inverts the engine's one-shot nature for a
feature that only needs to push. Not worth it for one-way output.

**Reuse stdout with `--quiet`.** Cheapest to write, and wrong: it makes two
flags load-bearing for one behaviour, and any future stdout writer silently
corrupts the stream. The module already ruled stdout out.

## Consequences

- **A live view becomes possible without touching execution semantics.** No
  phase, retry or aggregation rule changes; nothing about how a verdict is
  reached moves. The engine says out loud what it was already saying to itself.
- **The three hosts keep working unchanged.** The native host, the `wasmtime`
  CLI and a JavaScript host all inherit stderr; none needs to know this exists
  until something reads it.
- **`--events` is debuggable by hand**, which is how a channel stays honest:
  `anvil ejemplos/basica.yaml --events 2>&1 >/dev/null` is a readable stream a
  person can watch.
- **Two renderings of the same fact now exist** — the events and the final
  report. They must be built from the same code path in `json.rs` rather than
  written twice, or they will drift, and a drift here means the live view and
  the report disagree about a measurement. That is the maintenance cost this
  ADR incurs and it should be paid at implementation time, not later.
- **`docs/diseno/reportes.md` is marked a proposal** and describes a lifecycle
  this partly realises; it should be revisited when this is implemented.
- **The control channel is still owed.** Breakpoints, pause and abort need the
  engine to be driven, not just observed, and nothing here provides that.
