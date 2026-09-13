# 4. Your first sequence

Open a second terminal — the first one is busy serving your step — and set it
up:

```console
$ cd ~/anvil-book
$ export PATH="$PWD/anvil-v0.5.0-x86_64-linux-musl:$PATH"
$ mkdir sequences
```

## The file

Create `sequences/first.yseq`:

```yaml
name: first

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

main:
  - name: board/measure_rail
    executor: bench
    limit: { type: range, min: 4.75, max: 5.25 }
```

It is YAML. Line by line:

- **`name`** names the sequence in reports.
- **`executors`** is the list of executors this sequence talks to. Each has a
  `name` you choose, a `type` — `grpc` for a process listening on a port, as
  your C# executor is — and where to find it.
- **`main`** is the list of steps to run. Chapter 6 adds `setup` and `cleanup`.
- Each step has the **`name`** the executor publishes and the **`executor`**
  that serves it, by the name you gave it above.
- **`limit`** is the acceptance criterion: a `range` passes when the
  measurement is between `min` and `max`.

**Always write `executor:` on a step.** A step without one is sent to a small
executor built into the engine for its own demonstrations, which does not know
your steps.

The extension can be `.yseq` or `.yaml`; Anvil reads both the same way.

## Run it

```console
$ anvil sequences/first.yseq
ejecutor de pasos escuchando en 40243
secuencia 'first' cargada (1 pasos en main, 0 subsecuencia(s) externa(s), 1 ejecutor(es))
motor conectado
conectado a los ejecutores de pasos (embebido en 127.0.0.1:40243)
catálogo pedido
1 paso(s) comprobados contra el catálogo de su ejecutor
=== first: pass ===
  [pass] board/measure_rail: 
conexión cerrada; esperando otra
$ echo $?
0
```

Two kinds of lines are mixed there.

The **report** is the part between `=== first: pass ===` and the step lines
under it: the sequence passed, and so did its one step. It goes to standard
output.

Everything else is **diagnostics** on the error stream: the engine starting its
built-in executor on a free port, loading the file, connecting, and — the line
that matters — `1 paso(s) comprobados contra el catálogo de su ejecutor`,
"1 step checked against its executor's catalog". Before running anything,
Anvil asked `bench` what it serves and checked that the sequence only asks for
steps that exist. These messages are still in Spanish in 0.5.0
([#59](https://github.com/anlaco/anvil/issues/59)).

The **exit code** is `0` because the sequence passed. Anything else — a fail,
an error, a file that does not load — exits with `1`.

To see only the report, send the diagnostics away:

```console
$ anvil sequences/first.yseq 2>/dev/null
=== first: pass ===
  [pass] board/measure_rail: 
```

The rest of the book does that, unless the diagnostics are the point.

The console report does not show the measured value — 4.98 appears nowhere.
The reports in chapter 10 carry it.

## When a name is wrong

Make a typo on purpose. `sequences/unknown.yseq` asks for `measure_rial`:

```yaml
name: unknown

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

main:
  - name: board/measure_rial
    executor: bench
```

```console
$ anvil sequences/unknown.yseq 2>&1 | tail -n 3
la secuencia no casa con lo que ofrecen los ejecutores (1 problema(s)):
  - step 'board/measure_rial': executor 'bench' does not serve it (it serves: board/measure_rail)
conexión cerrada; esperando otra
```

"The sequence does not match what the executors offer": `bench` does not serve
`board/measure_rial`, and the message lists what it does serve. Nothing ran.
This check happens on every run, before the first step, so a misspelt step
never gets as far as the bench.
