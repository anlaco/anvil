# 4. Your first sequence

Open a second terminal — the first one is busy serving your step — and set it
up:

```console
$ cd ~/anvil-book
$ export PATH="$PWD/anvil-v0.7.0-x86_64-linux-musl:$PATH"
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
    type: numeric_limit
    module: board/measure_rail
    executor: bench
    limit: { comparison: GELE, low: 4.75, high: 5.25 }
```

It is YAML. Line by line:

- **`name`** names the sequence in reports.
- **`executors`** is the list of executors this sequence talks to. Each has a
  `name` you choose, a `type` — `grpc` for a process listening on a port, as
  your C# executor is — and where to find it.
- **`main`** is the list of steps to run. Chapter 6 adds `setup` and `cleanup`.
- Each step has a **`name`**, which is how the report shows it, and a
  **`type`**, which says how it is judged. `numeric_limit` judges a number
  against a limit; chapter 5 shows the others. They are TestStand's step types.
- **`module`** is what the step calls — the name the executor publishes — and
  **`executor`** is who serves it, by the name you gave it above. Here the
  step is named after its module; two steps calling one module with different
  inputs would each get a name of their own.
- **`limit`** is the acceptance criterion, in TestStand's terms: `GELE` passes
  when the measurement is greater than or equal to `low` and less than or equal
  to `high`.

**Every step that calls an executor names it with `executor:`.** Anvil has no
executor of its own to fall back on, so a step without one is refused when the
sequence loads, with a message that says what to add.

The extension can be `.yseq` or `.yaml`; Anvil reads both the same way.

## Run it

```console
$ anvil sequences/first.yseq
secuencia 'first' cargada (1 pasos en main, 0 subsecuencia(s) externa(s), 1 ejecutor(es))
connected to the step executors (bench)
1 paso(s) comprobados contra el catálogo de su ejecutor
=== first: pass ===
  [pass] board/measure_rail: 
$ echo $?
0
```

Two kinds of lines are mixed there.

The **report** is the part between `=== first: pass ===` and the step lines
under it: the sequence passed, and so did its one step. It goes to standard
output.

Everything else is **diagnostics** on the error stream: loading the file,
connecting to `bench`, and — the line that matters — `1 paso(s) comprobados contra el catálogo de su ejecutor`,
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
    type: pass_fail
    module: board/measure_rial
    executor: bench
```

```console
$ anvil sequences/unknown.yseq 2>&1 | tail -n 3
connected to the step executors (bench)
la secuencia no casa con lo que ofrecen los ejecutores (1 problema(s)):
  - step 'board/measure_rial': executor 'bench' does not serve it (it serves: board/measure_rail)
```

"The sequence does not match what the executors offer": `bench` does not serve
`board/measure_rial`, and the message lists what it does serve. Nothing ran.
This check happens on every run, before the first step, so a misspelt step
never gets as far as the bench.
