# 6. Setup, main, cleanup and retries

Real tests power a unit before measuring it and must power it off afterwards,
whatever happened in between. Sequences have three phases for that.

Create `Bench/Fixture.cs` and restart the executor:

```csharp
using Anvil.Step;

/// <summary>The test fixture the board sits in.</summary>
public static class Fixture
{
    /// <summary>Clamps the board and powers it.</summary>
    [Step]
    public static void PowerOn() { }

    /// <summary>Cuts power and releases the board.</summary>
    [Step]
    public static void PowerOff() { }

    /// <summary>Opens the link to the board; the first attempt always times out.</summary>
    [Step]
    public static Outcome Connect(Ctx ctx) =>
        ctx.Attempt == 1
            ? Outcome.Errored("no answer from the board")
            : Outcome.Passed("connected on attempt " + ctx.Attempt);

    /// <summary>Measures the rail while it settles: low on the first attempt.</summary>
    [Step]
    public static double MeasureSettlingRail(Ctx ctx) => ctx.Attempt == 1 ? 4.1 : 4.97;
}
```

`Ctx` is optional: declare it as a parameter and the SDK hands you the attempt
number, among other things. It is not an input the sequence sends.

## Three phases

```yaml
name: phases

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

setup:
  - name: fixture/power_on
    type: action
    module: fixture/power_on
    executor: bench

main:
  - name: fixture/measure_settling_rail
    type: numeric_limit
    module: fixture/measure_settling_rail
    executor: bench
    retries: 3
    limit: { comparison: GELE, low: 4.75, high: 5.25 }

  - name: board/check_led
    type: pass_fail
    module: board/check_led
    executor: bench

cleanup:
  - name: fixture/power_off
    type: action
    module: fixture/power_off
    executor: bench
```

```console
$ anvil sequences/phases.yseq 2>/dev/null
=== phases: fail ===
  [done] fixture/power_on: 
  [fail] fixture/measure_settling_rail: 4.1 fuera de rango [4.75, 5.25]
  [done] fixture/power_off: 
```

- **`setup`** runs first. If any setup step fails or errors, `main` is skipped
  entirely. `fixture/power_on` is an `action`: it does something and judges
  nothing, so it reports `done`, and `done` lets the sequence go on.
- **`main`** runs next and stops at its first step that fails or errors.
- **`cleanup`** always runs, whatever happened before. `fixture/power_off`
  ran even though the measurement failed. Put everything that makes the bench
  safe here.

## Retries do not re-measure a failed limit

That sequence has `retries: 3` on `fixture/measure_settling_rail`, and the step
reads 4.97 V from its second attempt on. Yet the report shows 4.1 and `fail`.

This is how Anvil behaves today
([#76](https://github.com/anlaco/anvil/issues/76)): **`retries` repeats a step
only while its module does not pass.** The limit is applied once, after the last attempt. A
module that returns a measurement has passed as far as retries are concerned, so
it is not called again, and the limit then fails the first value.

Retries are for a step that fails or errors on its own — a link that times out
once, for example:

```yaml
name: retries

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

setup:
  - name: fixture/power_on
    type: action
    module: fixture/power_on
    executor: bench
  - name: fixture/connect
    type: action
    module: fixture/connect
    executor: bench
    retries: 3

main:
  - name: board/measure_rail
    type: numeric_limit
    module: board/measure_rail
    executor: bench
    limit: { comparison: GELE, low: 4.75, high: 5.25 }

cleanup:
  - name: fixture/power_off
    type: action
    module: fixture/power_off
    executor: bench
```

```console
$ anvil sequences/retries.yseq 2>/dev/null
=== retries: pass ===
  [done] fixture/power_on: 
  [done] fixture/connect: connected on attempt 2
  [pass] board/measure_rail: 
  [done] fixture/power_off: 
```

`fixture/connect` errored on attempt 1 and succeeded on attempt 2 — an action,
so it reports `done`. `retries` is
the **total** number of attempts: the default is 1, meaning no retry, and 0 is
refused when the file loads.

The report keeps only the last attempt. The "on attempt 2" in the message is
there because the step wrote it; Anvil does not record the earlier attempt.

Without the retries, the same link does not come up, and the setup rule shows:

```yaml
name: setup_fails

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

setup:
  - name: fixture/power_on
    type: action
    module: fixture/power_on
    executor: bench
  - name: fixture/connect
    type: action
    module: fixture/connect
    executor: bench

main:
  - name: board/measure_rail
    type: numeric_limit
    module: board/measure_rail
    executor: bench
    limit: { comparison: GELE, low: 4.75, high: 5.25 }

cleanup:
  - name: fixture/power_off
    type: action
    module: fixture/power_off
    executor: bench
```

```console
$ anvil sequences/setup-fails.yseq 2>/dev/null
=== setup_fails: error ===
  [done] fixture/power_on: 
  [error] fixture/connect: no answer from the board
  [done] fixture/power_off: 
```

The setup errored, `main` never ran, and `cleanup` still did.
