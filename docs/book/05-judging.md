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

`sequences/judging.yseq` uses each kind of judgement:

```yaml
name: judging

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

main:
  - name: board/measure_rail
    executor: bench
    limit: { type: range, min: 4.75, max: 5.25 }

  - name: board/measure_leakage
    executor: bench
    limit: { type: comparison, op: le, expected: 0.001 }

  - name: board/check_led
    executor: bench
```

```console
$ anvil sequences/judging.yseq 2>/dev/null
=== judging: pass ===
  [pass] board/measure_rail: 
  [pass] board/measure_leakage: 
  [pass] board/check_led: 
```

- `board/measure_rail` returns a number and has a **`range`** limit.
- `board/measure_leakage` has a **`comparison`**: the measurement is compared
  with `expected` using `op`, one of `eq`, `ne`, `lt`, `le`, `gt` or `ge`. Here,
  "less than or equal to 1 mA".
- `board/check_led` returns a `bool`, so the step itself decides pass or fail
  and needs no limit.

Now the same rail against a limit it cannot meet, in
`sequences/rail-tight.yseq`:

```yaml
name: rail_tight

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

main:
  - name: board/measure_rail
    executor: bench
    limit: { type: range, min: 5.0, max: 5.25 }
```

```console
$ anvil sequences/rail-tight.yseq 2>/dev/null
=== rail_tight: fail ===
  [fail] board/measure_rail: 4.98 fuera de rango [5, 5.25]
$ echo $?
1
```

The step still returned 4.98 and did not change; the verdict did. The message
reads "4.98 out of range [5, 5.25]", and the exit code is `1`.

## Fail is about the unit, error is about the bench

`board/read_temperature` does not measure anything: it answers
`Outcome.Errored`, as a real step would when a sensor does not reply.

```yaml
name: error

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

main:
  - name: board/read_temperature
    executor: bench
  - name: board/check_led
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

- **`main` stops at the first step that does not pass.** `board/check_led`
  never ran.
- A sequence's verdict is the worst of its steps: any `error` makes it `error`;
  otherwise any `fail` makes it `fail`. Both exit with `1`; the report tells
  them apart.
