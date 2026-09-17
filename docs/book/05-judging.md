# 5. Measuring and judging

A step reports what it saw. The sequence says what is acceptable. Anvil
compares the two. Keeping the criterion out of the step is what lets you
tighten a tolerance by editing a text file that anyone can review, without
rebuilding the executor.

## Three more steps

Create `Bench/Board.Judging.cs`, which adds three steps to the `board` module:

```csharp
using Anvil.Step;

public static partial class Board
{
    /// <summary>Measures the leakage current with the board idle, in amps.</summary>
    [Step]
    public static double MeasureLeakage() => 0.0004;

    /// <summary>Checks that the power LED is lit.</summary>
    [Step]
    public static bool CheckLed() => true;

    /// <summary>Reads the on-board temperature sensor.</summary>
    [Step]
    public static Outcome ReadTemperature() =>
        Outcome.Errored("the temperature sensor did not answer");
}
```

Stop the executor with Ctrl+C in its terminal and start it again with
`dotnet run --project Bench -- --port 9201`, so it serves the new steps.

## Limits

A step's `type` says how it is judged. `sequences/judging.yseq` uses the two
that judge something:

```yaml
name: judging

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

main:
  - name: board/measure_rail
    type: numeric_limit
    module: board/measure_rail
    executor: bench
    limit: { comparison: GELE, low: 4.75, high: 5.25 }

  - name: board/measure_leakage
    type: numeric_limit
    module: board/measure_leakage
    executor: bench
    limit: { comparison: LE, low: 0.001 }

  - name: board/check_led
    type: pass_fail
    module: board/check_led
    executor: bench
```

```console
$ anvil sequences/judging.yseq 2>/dev/null
=== judging: pass ===
  [pass] board/measure_rail: 
  [pass] board/measure_leakage: 
  [pass] board/check_led: 
```

- `board/measure_rail` is a **`numeric_limit`**: it returns a number, and the
  limit `GELE` judges it between `low` and `high`, both included.
- `board/measure_leakage` is a `numeric_limit` with **`LE`**, one limit only:
  "less than or equal to 1 mA". The one-limit codes are `EQ`, `NE`, `GT`, `LT`,
  `GE` and `LE`, and they use `low`.
- `board/check_led` is a **`pass_fail`**: it returns a `bool`, so the step
  itself decides, and it needs no limit.

The two-limit codes are TestStand's: `GTLT`, `GELE`, `GELT` and `GTLE` pass
**inside** `low` and `high` — `GE`/`LE` include the limit, `GT`/`LT` do not —
and `LTGT`, `LEGE`, `LEGT` and `LTGE` pass **outside** them. `EQT` passes
within a tolerance of a `nominal` value, and `none` records the value and
judges nothing. A limit can carry `units`, which the report shows and the
comparison ignores.

A numeric limit **needs a number**. A `numeric_limit` step whose module returns
none is `error`, not a pass: a limit that was never checked must not read as a
limit that was met.

The third type, **`action`**, is a step that does something and judges nothing
— switching a supply on, connecting to a fixture. When its module succeeds it
reports **`done`**, not `pass`: the report never says a step passed a check it
did not make. Chapter 6 uses them.

Now the same rail against a limit it cannot meet, in
`sequences/rail-tight.yseq`:

```yaml
name: rail_tight

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

main:
  - name: board/measure_rail
    type: numeric_limit
    module: board/measure_rail
    executor: bench
    limit: { comparison: GELE, low: 5.0, high: 5.25 }
```

```console
$ anvil sequences/rail-tight.yseq 2>/dev/null
=== rail_tight: fail ===
  [fail] board/measure_rail: 4.98 fuera de rango [5, 5.25]
$ echo $?
1
```

The step still returned 4.98 and did not change; the verdict did. The message
reads "4.98 out of range [5, 5.25]" — the brackets say both ends are included,
as `GELE` does — and the exit code is `1`.

## Fail is about the unit, error is about the bench

`board/read_temperature` does not measure anything: it answers
`Outcome.Errored`, as a real step would when a sensor does not reply.

```yaml
name: error

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

main:
  - name: board/read_temperature
    type: pass_fail
    module: board/read_temperature
    executor: bench
  - name: board/check_led
    type: pass_fail
    module: board/check_led
    executor: bench
```

```console
$ anvil sequences/error.yseq 2>/dev/null
=== error: error ===
  [error] board/read_temperature: the temperature sensor did not answer
$ echo $?
1
```

The verdict is `error`, not `fail`, and the difference is deliberate. A `fail`
is a statement about the unit — it is out of spec, reject it. An `error` says
Anvil could not judge the unit at all: the bench is broken, the unit may be
perfectly good, and treating it as rejected would scrap a good board.

So when you write a step:

- If you measured, return the measurement and let the limit judge.
- If the step itself can decide, return `bool` or `Outcome.Failed`.
- If you **could not** measure or decide, return `Outcome.Errored` with a
  message saying why. Never return a fail to mean "something went wrong".

An exception that escapes your method also becomes `error`, and the executor
keeps running.

Two more things the output shows:

- **`main` stops at the first step that fails or errors.** `board/check_led`
  never ran. A `done` step does not stop anything.
- A sequence's verdict is the worst of its steps: any `error` makes it `error`;
  otherwise any `fail` makes it `fail`. Both exit with `1`; the report tells
  them apart. `done` and `skipped` move the verdict neither way.
