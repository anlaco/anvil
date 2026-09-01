# ADR-0024: The signature is the catalog in Rust too, and that needs a second door in the WIT

- **Status:** Accepted
- **Date:** 2026-09-01
- **How it was decided:** in this repo, on a commission from management that
  asked for one thing — *"writing WASM code for Anvil in Rust should be as
  simple as putting a few decorators on functions, the way it already is in
  Python"* — and, when the two halves of the Python decorator were laid out,
  chose the scope: authoring **and** catalog, WIT break included. Everything
  asserted about today's state is **verified by running** the code in this repo
  and cited with file and line.
- **Relates to:** ADR-0003, ADR-0012, ADR-0015, ADR-0019, ADR-0020, ADR-0021,
  ADR-0022, ADR-0023, issue #39,
  [guia-inicio-rapido.md](../guia-inicio-rapido.md)
- **Scope:** decides **and implements** the Rust step-authoring SDK
  (`executors/rust/`) and the `describe` function of `anvil:step@0.4.0`. It does
  not give the component object references, does not give it `options`, does not
  touch `paso.proto`, the engine or the embedded executor, and does not decide
  what to do about a component that traps (§Consequences records the finding).

## Context

[ADR-0021](0021-el-ejecutor-describe-su-catalogo.md) gave the Python executor a
surface with two halves, and they are easy to mistake for one:

- **Authoring.** `@step` over a function. The SDK reads the signature, maps the
  parameters by name, checks the types, turns any exception into `error`
  (`executors/python/anvil_step/__init__.py:389-440`).
- **Catalog.** The same signature is published through `Describe`, so Anvil
  checks a sequence *before touching the unit*
  (`executors/python/server.py:235-251`).

Rust had neither. Writing a step meant installing `cargo-component`, copying
`wit/anvil-step.wit` into your project, keeping a 507-line generated
`bindings.rs` in your repo, and writing `impl Guest for Component` with a single
`run` that **looks for its own parameters by hand** — the reference example did
`parametros.iter().find(|p| p.name == "a_quien")` and matched on `Value`
(`ejemplos/hola-paso/src/lib.rs:36-53`, before this ADR).

There was no dispatch by name at all: the example answered the same thing for
`medir_voltaje` and for `conectar_equipo`, the two steps `ejemplos/demo_wasm.yaml`
sends it. And there could be no catalog, because `anvil:step@0.3.0` exported a
single function; the bridge said so out loud and answered `describes = false`
(`executors/wasm/src/main.rs:240-242`, before this ADR). Verified by running it
on 2026-09-01, the sequence printed:

```
aviso: 3 step(s) unchecked on 'rust_sdk': it does not describe its catalog
```

So the two halves are not one decision: the first is a library, the second is a
change to the contract. Doing only the first would have shipped an SDK that
knows every step's name and type at compile time and has **nowhere to say them**.

## Decision

### 1 — The authoring surface is `#[step]`, `Outcome` and `export!()`

```rust
use anvil_step::{step, Ctx, Outcome};

/// Measures the voltage on a channel.
#[step(outputs(channel_used: f64))]
fn measure_voltage(ctx: Ctx, channel: f64, scale: Option<String>) -> Outcome {
    Outcome::measured(read(channel, scale)).output("channel_used", channel)
}

anvil_step::export!();
```

The signature is the catalog, as in Python. What the signature cannot give, the
attribute takes: `outputs` (a return value has no names) and `name` (when the
step's name in a sequence is not a valid Rust identifier).

A parameter is `f64`, `String`, `bool` or an `Option` of one of them — the
`Option` is what makes it optional. Anything else is a **compile error**, which
is where Rust does better than Python: there, an unannotated parameter can only
be described as *unspecified* and left unchecked (ADR-0019, Rule 2); here the
step does not build.

`ctx` is injected only if the step asks for it, and is never described: it is the
executor talking to the step. It carries `attempt` and `step_name` and nothing
else — no `options` and no object store, because a component has nowhere to keep
a bench session (ADR-0022 §8). Promising them would be lying.

### 2 — `export!()` takes no arguments, and could not take them

The steps are the ones `#[step]` registered, wherever they live — collected
through `inventory`, whose constructors run when the component is instantiated.
Nothing to list and nothing to keep in sync.

It is also forced. `wit_bindgen::generate!` defines `macro_rules!` of its own,
and Rust refuses to define a `macro_rules!` inside the expansion of a macro that
has metavariables (`error: expected identifier, found metavariable`, verified
2026-09-01). A version of `export!` taking a list of steps could not exist.

### 3 — The bindings live in the SDK, and a component builds with the plain toolchain

`wit_bindgen::generate!` runs inside `anvil-step` with `pub_export_macro`, so the
`#[unsafe(no_mangle)]` symbols that turn a `cdylib` into a component are still
emitted by the author's crate while the WIT and the generated code stay here.
That removes the `wit/` directory and the checked-in `bindings.rs` from the
author's project.

And the target is **`wasm32-wasip2`**, not `cargo component` over `wasip1`:
`cargo build --target wasm32-wasip2` produces a component the bridge loads and
runs (verified 2026-09-01, `ejemplos/demo_wasm.yaml`). `cargo-component` is no
longer needed to write a step for Anvil.

The price is a second copy of the WIT, in `executors/rust/anvil-step/wit/`. The
bridge's copy remains the source of truth and the test
`the_sdk_wit_is_the_bridge_wit` compares them, which is the net ADR-0020 §4e asks
for.

### 4 — `anvil:step@0.4.0` grows a `describe`

```wit
describe: func() -> list<step-spec>;
```

with `step-spec`, `parameter-spec`, `output-spec` and `value-type` mirroring the
catalog messages of `paso.proto` (`crates/modelo/paso.proto:106-144`), minus
`reference`: a component cannot hold an object, and the bridge already rejects a
reference before it arrives (`executors/wasm/src/main.rs:209-214`).

`paso.proto` is **not touched** and `CONTRACT` stays at 4: `Describe` already
existed in the service and the bridge already answered it, only empty. There is
no old peer whose silence could alter a verdict (ADR-0020 §4c). The break is the
WIT's, and the WIT is paid for by recompiling (ADR-0020 §4d) — which is what
0.4.0 is: the first time that bill has come due.

### 5 — An empty catalog is read as "do not check me"

In gRPC the `describes` boolean tells "I serve nothing" apart from "I do not
describe myself", because an executor can legitimately serve zero steps
(ADR-0021 §4). A component that serves zero steps has nothing to do, so the safe
reading is the only useful one: the bridge turns an empty list into
`describes = false`.

That is also what the SDK returns when two steps are registered under one name.
It does not pick one — picking would run **a different measurement than the
sequence asked for** and report `pass` — so `describe` goes empty and every step
of that component comes back as `error` naming the duplicate.

## Alternatives rejected

- **Authoring without the catalog.** The cheap half. It would have left the SDK
  holding every name and type at compile time with no door to publish them
  through, and left `--validate --with-executors` blind to WASM steps for as long
  as the WIT stayed at 0.3.0. The break costs one recompile of one example today
  and grows more expensive every month.
- **Reading the catalog off the artifact** instead of asking the component. The
  WIT embedded in the `.wasm` says *"there is a `run` that takes a name and a
  list"* — true and useless, the same reason ADR-0021 §1 rejected gRPC
  reflection.
- **A `describe` that returns the catalog as JSON text.** One WIT type instead of
  four, and a parser on the other side plus a format nobody checks. The records
  cost nothing and the bridge translates them field by field.
- **Recognising parameter types in the proc-macro** (matching on `f64`, `String`,
  …) instead of a trait. It would put the list of admissible types in the macro,
  where extending it means editing a `match` and where the error message for
  everything else is written by hand. `Input` with
  `#[diagnostic::on_unimplemented]` gives the message and keeps the list in one
  place.
- **`panic!` as the way to report a bench problem**, with the bridge turning the
  trap into `error`. It cannot work today (§Consequences) and it would still be
  the wrong shape: `Result` is how Rust says "this did not work".

## Consequences

- Writing a step in Rust is a function and one dependency. `ejemplos/hola-paso`
  went from 69 lines plus 507 of generated bindings plus a copied WIT, to 3 steps
  in ~30 lines of actual code.
- WASM steps stop being *unchecked*. Verified 2026-09-01: a sequence sending
  `a_quienn` to a step that takes `a_quien` now stops at
  `--validate --with-executors` with exit 1 and
  `step 'hola' (mi_paso_wasm): it takes no input called 'a_quienn' (it takes: a_quien)`,
  where before it loaded without a complaint.
- **Every existing component must be rebuilt.** In this repo that is
  `ejemplos/hola-paso`; outside it, anything built against `anvil:step@0.3.0`.
  wasmtime refuses to instantiate the mismatch, so nobody finds out the wrong way.
- `make build` now builds the reference component, and so does CI. It could not
  before, because it needed `cargo component` — the example was a **manual**
  acceptance criterion (`docs/planes/m5-ext.md:118-119`) and nothing verified it
  automatically.
- **Two findings about the bridge, both older than this work:**
  - A `println!` in a step killed the bridge's worker with *"Cannot start a
    runtime from within a runtime"* and cut the sequence: the blocking wasmtime
    call ran on the tokio runtime's driver thread, and the WASI sync bindings
    block on the runtime from inside to serve the component's stdout. Fixed here
    with `block_in_place`, because writing a debug print is the first thing
    anyone does and the failure lands mid-run on a real unit (RF-12). Verified
    before and after.
  - A `panic!` in a step still cuts the run: the component's instance is gone
    and the bridge does not reinstantiate it, so the engine sees the stream close
    without an answer. Verified 2026-09-01. **Not fixed here** — it is
    reinstantiation after a trap, its own piece of work, open as
    [#58](https://github.com/anlaco/anvil/issues/58) — and it is written into
    the quick-start guide as a known limitation, not left to be discovered.
- The Python executor keeps two things this SDK does not have: object references
  (ADR-0022) and `options`. That asymmetry is the WIT's, not the SDK's.
