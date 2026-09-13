# 7. Inputs, variables and flow

So far every step has taken nothing and returned a fixed value. Real steps take
parameters — which channel, which range — and later steps depend on earlier
results.

Create `Bench/Dmm.cs` and `Bench/Unit.cs`, then restart the executor:

```csharp
using Anvil.Step;

/// <summary>A multimeter with four input channels.</summary>
public static class Dmm
{
    /// <summary>Measures the voltage on a channel.</summary>
    /// <param name="channel">The input channel, 1 to 4.</param>
    /// <param name="range">The measurement range.</param>
    [Step]
    public static Outcome MeasureVoltage(double channel, string range = "auto") =>
        channel switch
        {
            1 => Outcome.Measured(1.1, "range " + range),
            2 => Outcome.Measured(1.8, "range " + range),
            3 => Outcome.Measured(3.3, "range " + range),
            4 => Outcome.Measured(5.0, "range " + range),
            _ => Outcome.Errored("the multimeter has no such channel"),
        };
}
```

```csharp
using Anvil.Step;

/// <summary>The unit's own identity.</summary>
public static class Unit
{
    /// <summary>Reads the serial number from the unit's memory.</summary>
    [Step]
    public static Outcome ReadSerial() =>
        Outcome.Passed("serial read").Output("serial", "SN-0042");
}
```

## Inputs

A method's parameters are the step's inputs, named in `snake_case` like the
step. `channel` has no default, so it is required; `range` has one, so it is
optional. A parameter can be a `double`, a `string`, a `bool`, or a reference
(chapter 8); anything else does not compile.

`--list` shows that signature — `dmm/measure_voltage(channel: number, range?: text)` —
and Anvil checks every sequence against it.

## Variables and results

```yaml
name: data

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

locals:
  channel: 2
  rail: 0.0
  led_ok: false

main:
  - name: unit/read_serial
    executor: bench

  - name: dmm/measure_voltage
    executor: bench
    inputs:
      channel: '${locals.channel}'
      range: "10V"
    limit: { type: range, min: 1.7, max: 1.9 }
    assign:
      rail: result.measured_value

  - name: board/check_led
    executor: bench
    assign:
      led_ok: 'result.status == "pass"'

  - name: rail_in_spec
    type: pass_fail
    condition: 'locals.rail > 1.75 && locals.led_ok'
```

```console
$ anvil sequences/data.yseq --json data.json 2>/dev/null
=== data: pass ===
  [pass] unit/read_serial: serial read
  [pass] dmm/measure_voltage: range 10V
  [pass] board/check_led: 
  [pass] rail_in_spec: condición cumplida
$ cat data.json
{
  "sequence": "data",
  "status": "pass",
  "skipped_steps": 0,
  "total_steps": 4,
  "steps": [
    {
      "name": "unit/read_serial",
      "status": "pass",
      "phase": "main",
      "message": "serial read",
      "measured_value": null,
      "limit_min": null,
      "limit_max": null,
      "expected_value": null,
      "operator": null,
      "inputs": {},
      "outputs": {
        "serial": "SN-0042"
      }
    },
    {
      "name": "dmm/measure_voltage",
      "status": "pass",
      "phase": "main",
      "message": "range 10V",
      "measured_value": 1.8,
      "limit_min": 1.7,
      "limit_max": 1.9,
      "expected_value": null,
      "operator": null,
      "inputs": {
        "channel": 2.0,
        "range": "10V"
      },
      "outputs": {}
    },
    {
      "name": "board/check_led",
      "status": "pass",
      "phase": "main",
      "message": "",
      "measured_value": null,
      "limit_min": null,
      "limit_max": null,
      "expected_value": null,
      "operator": null,
      "inputs": {},
      "outputs": {}
    },
    {
      "name": "rail_in_spec",
      "status": "pass",
      "phase": "main",
      "message": "condición cumplida",
      "measured_value": null,
      "limit_min": null,
      "limit_max": null,
      "expected_value": null,
      "operator": null,
      "inputs": {},
      "outputs": {}
    }
  ]
}
```

What happens there:

- **`locals`** declares the sequence's variables with their initial values. The
  type comes from the value: `2` and `0.0` are numbers, `false` is a boolean,
  `""` would be text.
- **`inputs`** gives a step its parameters. A plain value is sent as it is
  (`"10V"`); a value in `${...}` is an expression, evaluated before the step
  runs (`${locals.channel}` sends `2`).
- **`assign`** stores something from the step's `result` in a local after it
  runs: `result.measured_value`, or an expression such as
  `result.status == "pass"`.
- A step with **`type: pass_fail`** calls no executor. Anvil evaluates its
  `condition` and the step passes or fails on it — the place for a verdict that
  combines several measurements.

The JSON report (chapter 10) records what each step was given in `inputs` and
what it returned in `outputs`. That is what makes a result reconstructible
later: two runs on different channels produce different reports.

### Outputs from a C# step cannot be assigned yet

`unit/read_serial` returns a named output, `serial`, and the report above shows
it. But reading it into a variable does not work in 0.5.0:

```yaml
name: serial

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

locals:
  serial: ""

main:
  - name: unit/read_serial
    executor: bench
    assign:
      serial: result.outputs.serial
```

```console
$ anvil sequences/serial.yseq 2>&1 | tail -n 3
la secuencia no casa con lo que ofrecen los ejecutores (1 problema(s)):
  - step 'unit/read_serial' (bench): 'assign' reads result.outputs.serial and the step does not return it (it returns: none)
conexión cerrada; esperando otra
```

Anvil checks `assign` against the outputs a step declares in its catalog, and
the C# SDK has no way yet to declare them — only the `open` step of chapter 8
declares one ([#77](https://github.com/anlaco/anvil/issues/77)). So the check refuses the sequence before it runs. Until that is
fixed, use named outputs from C# for the report only, and pass values between
steps through `result.measured_value` or `result.status`.

## Flow

```yaml
name: flow

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

file_globals:
  station: "bench-3"

locals:
  channel: 1

main:
  - name: pick_channel
    type: statement
    statement: 'locals.channel = locals.channel + 2'

  - name: dmm/measure_voltage
    executor: bench
    inputs: { channel: '${locals.channel}' }
    limit: { type: range, min: 3.0, max: 3.6 }

  - name: board/measure_leakage
    executor: bench
    precondition: 'file_globals.station == "bench-1"'
    limit: { type: comparison, op: le, expected: 0.001 }

  - name: board/read_temperature
    executor: bench
    disable: true
```

```console
$ anvil sequences/flow.yseq 2>/dev/null
=== flow: pass ===
  [pass] pick_channel: statement ok
  [pass] dmm/measure_voltage: range auto
  [skipped] board/measure_leakage: precondición falsa
  [skipped] board/read_temperature: disable
  (2 de 4 pasos saltados)
```

- **`type: statement`** runs an assignment inside the engine, without calling
  any executor: `locals.channel` becomes 3 before the measurement uses it.
- **`file_globals`** are variables visible to every sequence in the file.
  Steps read them; writing to one is refused when the file loads.
- **`precondition`** is an expression; when it is false the step is `skipped`
  without being called. Here the station is not `bench-1`.
- **`disable: true`** skips a step without evaluating anything.

Look at the verdict: the sequence **passed with two steps skipped**. A skipped
step is neither a pass nor a fail, and it does not fail the sequence. If a
measurement must always happen, do not put a precondition on it.

## Checking a sequence without running it

A typo in an input name, in `sequences/typo.yseq`:

```yaml
name: typo

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

main:
  - name: dmm/measure_voltage
    executor: bench
    inputs: { chanel: 2 }
```

```console
$ anvil sequences/typo.yseq --validate
secuencia 'typo' cargada (1 pasos en main, 0 subsecuencia(s) externa(s), 1 ejecutor(es))
'typo' válida (1 paso(s) en main, 0 subsecuencia(s) externa(s))
$ anvil sequences/typo.yseq --validate --with-executors
ejecutor de pasos escuchando en 46505
secuencia 'typo' cargada (1 pasos en main, 0 subsecuencia(s) externa(s), 1 ejecutor(es))
'typo' válida (1 paso(s) en main, 0 subsecuencia(s) externa(s))
motor conectado
catálogo pedido
1 paso(s) comprobados contra el catálogo de su ejecutor
la secuencia no casa con lo que ofrecen los ejecutores (2 problema(s)):
  - step 'dmm/measure_voltage' (bench): it takes no input called 'chanel' (it takes: channel, range)
  - step 'dmm/measure_voltage' (bench): the input 'channel' is required and the sequence does not send it
conexión cerrada; esperando otra
```

`--validate` loads the file and checks it — the schema, the expressions, the
subsequences — without starting or connecting to anything, so it runs in CI
with no bench. It cannot know what inputs a step takes, so it says the file is
valid.

`--validate --with-executors` also asks each executor for its catalog, and
catches both problems: there is no input `chanel`, and the required `channel`
is missing. A real run makes the same check before its first step.

Do not treat a green `--with-executors` as a guarantee in 0.5.0: when an
executor fails while describing itself, it still passes
([#70](https://github.com/anlaco/anvil/issues/70)).
