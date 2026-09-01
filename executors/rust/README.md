# The Rust step SDK (`anvil-step`)

Write a step for Anvil in Rust: annotate a function, compile to WASM. This is
the sibling of [`../python/`](../python/) — the same idea, a different shape,
because a WASM component is a function Anvil loads and not a server it talks to
([ADR-0015](../../docs/adr/0015-el-wasm-del-usuario-es-una-funcion-puenteado-a-grpc.md),
[ADR-0024](../../docs/adr/0024-the-signature-is-the-catalog-in-rust-too.md)).

> **You never write a `run` that dispatches by name**, never copy a `wit/`
> directory, never keep a generated `bindings.rs`, and never install
> `cargo component`. One dependency and the plain toolchain.

## Writing a step

```toml
# hola/Cargo.toml
[dependencies]
anvil-step = "0.4"

[lib]
crate-type = ["cdylib"]
```

```rust
// hola/src/lib.rs
use anvil_step::{step, Ctx, Outcome};

/// Measures the voltage on a channel.
#[step(outputs(channel_used: f64))]
fn measure_voltage(channel: f64, scale: Option<String>) -> Result<Outcome, String> {
    // `?` on a bench problem: the `Err` comes out as `error`, never `fail`.
    let volts = read_instrument(channel, scale)?;
    Ok(Outcome::measured(volts).output("channel_used", channel))
}

/// Checks the LED is lit.
#[step]
fn check_led() -> Outcome {
    Outcome::passed("led lit")
}

anvil_step::export!();
```

```sh
cargo build --target wasm32-wasip2
# → target/wasm32-wasip2/debug/hola.wasm
```

**The signature is the catalog.** The name of each parameter, its type and
whether it is required come from the function itself: they are not written
twice, so they cannot drift. That is what lets Anvil check a sequence **without
running it** (`--validate --with-executors`) and tell you that you wrote
`channell` instead of `channel` before the unit is on the bench
([ADR-0021](../../docs/adr/0021-el-ejecutor-describe-su-catalogo.md)).

What the signature cannot say, the attribute takes:

| | |
|---|---|
| `outputs(name: Type, …)` | The named outputs. A `return` carries no names, and these are what `assign: result.outputs.<name>` reads in the sequence. |
| `name = "…"` | The step's name in the sequence, when it cannot be the function's. |

A parameter is `f64`, `String` or `bool`, or an `Option` of one of them — which
is how it is declared optional. **Anything else does not compile**: a parameter
that needs structure is a badly cut step
([ADR-0020 §2](../../docs/adr/0020-parametros-del-paso-en-la-peticion.md)).

The first line of the doc comment becomes the step's description in the catalog.

### `ctx`, if you ask for one

```rust
/// Connects to the instrument; retries are the engine's business.
#[step]
fn connect(ctx: Ctx) -> Outcome {
    if ctx.attempt == 1 {
        return Outcome::failed("lost the handshake (transient)");
    }
    Outcome::passed("connected")
}
```

`ctx` carries the attempt number and the name the sequence used. It is never
part of the described signature: it is the executor talking to the step, not a
value out of the sequence. A step gets one **only if it declares it**.

### What a step gives back

`Outcome::measured(v)` for a measurement, `Outcome::passed(…)` /
`Outcome::failed(…)` for a pass/fail, `Outcome::error(…)` when it could not
judge. Shortcuts also work: a number is a measurement, a `bool` is pass/fail,
`()` is a pass with no measurement, and a `Result` whose `Err` is a bench
problem comes out as `error`.

**The threshold is not the step's business**: return the measurement and let the
engine judge it against the sequence's `limit`
([ADR-0008](../../docs/adr/0008-limites-evaluados-por-el-motor.md)).

The distinction that matters most is between the two reds:

- **`fail` is the unit's** — "I measured and it does not comply".
- **`error` is the bench's or the step's** — "I could not measure".

A step that blew up is never a failed unit
([ADR-0019](../../docs/adr/0019-que-hace-anvil-cuando-no-puede-juzgar.md),
Rule 2). Prefer `?` and `Result` over `panic!` and `unwrap()` — see the
limitations below.

### Testing your steps

Your steps stay ordinary functions: `#[step]` registers them and hands them back
untouched, and outside `wasm32` `export!()` expands to nothing. So `cargo test`
needs neither WASM nor Anvil.

## What this SDK does not have

- **Object references** ([ADR-0022 §8](../../docs/adr/0022-la-referencia-a-objeto-es-un-cuarto-tipo-y-nombra-una-ranura.md)):
  a component is a function with no state between calls, so it cannot hold an
  open instrument session. A step that needs one is served from a `grpc`
  executor of its own process, such as the Python one.
- **Executor options** (`--option key=value`): they are not in the WIT.
- **`panic!` as an error path.** WASM aborts, the instance is gone and the
  bridge does not reinstantiate it, so the run is cut. Verified 2026-09-01.
  Return an `Outcome::error` or a `Result`.

## Layout

| | |
|---|---|
| `anvil-step/` | The SDK: `Outcome`, `Ctx`, the registry, `export!` and the WIT bindings. |
| `anvil-step-macros/` | The `#[step]` attribute. |

The complete hello world — three steps in Rust, compiled and run by `anvil` — is
[`ejemplos/hola-paso/`](../../ejemplos/hola-paso/), the reference for the
[quick-start guide](../../docs/guia-inicio-rapido.md#writing-your-own-step-in-rust-adr-0015-adr-0024).

## License

**Apache-2.0**, like everything under [`executors/`](../README.md)
([ADR-0004](../../docs/adr/0004-licencia-dual-agpl-apache.md)): what you *use*
is AGPL; what you *link* is Apache. Your step links this crate and stays yours,
under whatever license you want.
