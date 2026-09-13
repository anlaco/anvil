# 8. Instruments that stay open

A session with a power supply holds a socket or a vendor driver handle. It
cannot be sent to Anvil and back, and it should not be reopened for every step.
So it stays inside the executor, and the sequence holds a **reference** to it:
a handle that means something only to the executor that issued it.

In C# you write the class you would write anyway. Create
`Bench/PowerSupply.cs` and restart the executor:

```csharp
using Anvil.Step;

/// <summary>A bench power supply, kept open across several steps.</summary>
[StepModule("psu")]
public sealed class PowerSupply
{
    private double _volts;

    /// <summary>Opens the session with the supply.</summary>
    /// <param name="resource">Where the supply is.</param>
    [StepConstructor]
    public PowerSupply(string resource) => Resource = resource;

    /// <summary>Where this supply is.</summary>
    public string Resource { get; }

    /// <summary>Sets the output voltage, in volts.</summary>
    /// <param name="volts">What to set the output to.</param>
    [Step]
    public void SetVoltage(double volts) => _volts = volts;

    /// <summary>Measures the current the load draws, in amps.</summary>
    [Step]
    public double MeasureCurrent() => _volts * 0.0625;

    /// <summary>Cuts the output.</summary>
    [Step]
    public void OutputOff() => _volts = 0;
}
```

- **`[StepConstructor]`** on the constructor publishes a step named `open` —
  `psu/open` — that creates the object, keeps it in the executor, and returns
  a reference to it as an output named after the module, `psu`.
- **`[StepModule("psu")]`** names the module. Without it the module would be
  `power_supply`, from the class name.
- **Instance steps** — `SetVoltage`, `MeasureCurrent`, `OutputOff` — take an
  extra input, `psu`, of type reference. The SDK finds the object it points to
  and calls the method on it; your method never sees the handle.

`--list` shows it: `psu/set_voltage(psu: reference, volts: number)`.

## Using it from a sequence

```yaml
name: supply

executors:
  - { name: bench, type: grpc, host: 127.0.0.1, port: 9201 }

locals:
  psu: { type: reference, executor: bench }

setup:
  - name: psu/open
    executor: bench
    inputs: { resource: "TCPIP::192.168.0.50::5025::SOCKET" }
    assign:
      psu: result.outputs.psu

main:
  - name: psu/set_voltage
    executor: bench
    inputs: { psu: '${locals.psu}', volts: 12.0 }

  - name: psu/measure_current
    executor: bench
    inputs: { psu: '${locals.psu}' }
    limit: { type: range, min: 0.5, max: 0.8 }

cleanup:
  - name: psu/output_off
    executor: bench
    inputs: { psu: '${locals.psu}' }
```

```console
$ anvil sequences/supply.yseq --json supply.json 2>/dev/null
=== supply: pass ===
  [pass] psu/open: 
  [pass] psu/set_voltage: 
  [pass] psu/measure_current: 
  [pass] psu/output_off: 
$ grep -A 6 "\"outputs\": {$" supply.json | head -n 8
      "outputs": {
        "psu": {
          "type": "reference",
          "executor": "bench",
          "lifetime": "ad96dffbdcf9425fb91af7bf196e34d8",
          "payload": "s1"
        }
```

- **`locals`** declares `psu` as a reference that comes from `bench`. It is the
  one kind of variable with no initial value: you cannot write a reference by
  hand, only receive it from a step.
- **`setup`** opens the supply and stores the reference with `assign`. This is
  the one output a C# step can have assigned in 0.5.0 (see chapter 7).
- **`main`** passes `${locals.psu}` to each step that needs the supply.
- **`cleanup`** turns the output off, which runs whatever happens in `main`.

The report records the reference like any other input or output, so you can
tell afterwards which session each step used. `lifetime` identifies the run of
the executor that issued it — the same value as the `life` it prints when it
starts — and `payload` is the executor's own name for the object.
