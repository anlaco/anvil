# 12. Steps in Python and Rust

Everything from chapter 4 on is about the sequence, and the sequence does not
know what language a step is written in. It names the step and the executor.
To show that, this chapter writes `board/measure_rail` twice more — in Python
and in Rust — and runs it with a sequence that differs from
`sequences/first.yseq` only in its `executors:` line.

Writing the step is much the same in all three languages: the function's
signature is the catalog, a returned number is a measurement, and the limit
lives in the sequence. What differs is **how the executor is started**, and
that is what this chapter is about.

| | C# | Python | Rust |
|---|---|---|---|
| the executor is | your own program | a server from the repository, pointed at a folder | `anvil-exec-wasm`, next to your compiled modules |
| who starts it | you | you | Anvil, when a sequence uses it |
| executor `type` | `grpc` | `grpc` | `wasm` |
| a step is | a method with `[Step]` | a function with `@step` | a function with `#[step]` |

## Python

The Python executor is not in the release archive: it is in the repository you
cloned in chapter 2. It needs `grpcio`, and the gRPC code it imports has to be
generated once after cloning. From `~/anvil-book`:

```console
$ python3 -m venv .venv
$ .venv/bin/pip install grpcio grpcio-tools
$ cd anvil/executors/python
$ ../../../.venv/bin/python -m grpc_tools.protoc -I ../../crates/modelo --python_out=. --grpc_python_out=. ../../crates/modelo/paso.proto
$ cd ~/anvil-book
```

If `python3 -m venv` fails, your distribution may ship it as a separate
package — `python3-venv` on Debian and Ubuntu. That case is **not verified**
in this book.

A step is a decorated function. Create `python_steps/board.py`:

```python
from anvil_step import step


@step
def measure_rail() -> float:
    """Measures the supply rail, in volts."""
    return 4.98
```

The file name is the module, so this is `board/measure_rail` again. Returning a
`float` is a measurement, a `bool` is pass or fail, and `Result.error(...)`
reports what could not be judged, as in C#. An exception becomes `error`.

Start the executor in its own terminal and leave it running:

```console
$ .venv/bin/python anvil/executors/python/anvil-exec-python --steps python_steps
module 'board' (/home/you/anvil-book/python_steps/board.py) sha256:831270a42ae7bb6a050f189a2dcf434cf8cc44ca68122871a74e84f3808009e6
python executor listening on 127.0.0.1:9101 — 1 step(s) from 1 module(s): board/measure_rail
```

It serves every module in the folder given to `--steps`, on port 9101 by
default. You never edit the server; to add steps, add files to the folder and
restart it.

```yaml
name: python

executors:
  - { name: py, type: grpc, host: 127.0.0.1, port: 9101 }

main:
  - name: board/measure_rail
    type: numeric_limit
    module: board/measure_rail
    executor: py
    limit: { comparison: GELE, low: 4.75, high: 5.25 }
```

```console
$ anvil sequences/python.yseq 2>/dev/null
=== python: pass ===
  [pass] board/measure_rail: 
```

The Python executor's own README, [`executors/python/README.md`](https://github.com/anlaco/anvil/blob/main/executors/python/README.md),
covers the rest: named outputs, references and options. It is still in Spanish.

## Rust

Rust steps are compiled to WebAssembly modules, and the executor is a program
that ships in the release archive, `anvil-exec-wasm`. It serves every `.wasm`
file in the folder it sits in. You need a Rust toolchain and one extra target:

```console
$ rustup target add wasm32-wasip2
```

Create `board-wasm/Cargo.toml` — the package name is the module name:

```toml
[package]
name = "board"
version = "0.1.0"
edition = "2021"

[dependencies]
anvil-step = { path = "../anvil/executors/rust/anvil-step" }

[lib]
crate-type = ["cdylib"]
```

and `board-wasm/src/lib.rs`:

```rust
use anvil_step::{step, Outcome};

/// Measures the supply rail, in volts.
#[step]
fn measure_rail() -> Outcome {
    Outcome::measured(4.98)
}

anvil_step::export!();
```

Build it, and put the module next to a copy of the executor. That folder is the
executor, and you can copy it to another machine as it is:

```console
$ cargo build --target wasm32-wasip2 --manifest-path board-wasm/Cargo.toml
$ mkdir wasm-dept
$ cp anvil-v0.9.0-x86_64-linux-musl/anvil-exec-wasm board-wasm/target/wasm32-wasip2/debug/board.wasm wasm-dept/
```

The first build takes about a minute. Then:

```yaml
name: wasm

executors:
  - { name: rs, type: grpc, host: 127.0.0.1, port: 9101 }

main:
  - name: board/measure_rail
    type: numeric_limit
    module: board/measure_rail
    executor: rs
    limit: { comparison: GELE, low: 4.75, high: 5.25 }

# Optional, and the engine never reads it: how to bring that executor up on a
# machine you are writing on. In production it is not here — the bench is
# already running and the sequence only needs its address.
dev:
  rs:
    runtime: wasm
    code: ../wasm-dept
```

Start the department, then run the sequence:

```console
$ ./wasm-dept/anvil-exec-wasm --modules wasm-dept --port 9101 &
$ anvil sequences/wasm.yseq 2>/dev/null
=== wasm: pass ===
  [pass] board/measure_rail: 
$ ./wasm-dept/anvil-exec-wasm --list --modules wasm-dept
board  sha256:2087caa515293f3a0cda2b14c78a09d6d9b2bfb98a5e2c63916c872cb1d807d5
    /home/you/anvil-book/wasm-dept/board.wasm
    board/measure_rail()
        Measures the supply rail, in volts.
```

**Nothing starts an executor for you** (ADR-0046). A sequence says where one is
listening; putting something there is the job of whoever runs the bench, which
on a real one is the machine's start-up and here is you. It is the same for
Python, for C# and for anything that comes next — which is the point: one kind
of executor, and it is an address.

`anvil-exec-wasm --list` shows each module with the SHA-256 of the file that
serves it, and `anvil describe sequences/wasm.yseq` asks the same thing through
the sequence.

The step-by-step [Rust quick start](https://anlaco.github.io/quickstart.html)
goes further, with two modules and optional inputs, and
[`executors/rust/README.md`](https://github.com/anlaco/anvil/blob/main/executors/rust/README.md) covers the SDK.
