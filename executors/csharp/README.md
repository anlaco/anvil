# The C# step executor

Write a step in C#: mark a method, and run your own process.

```csharp
using Anvil.Step;

/// <summary>Checks on the board's power rail.</summary>
public static class Rail
{
    /// <summary>Measures the rail voltage and lets the engine judge it.</summary>
    /// <param name="nominal">What the rail should read, in volts.</param>
    [Step]
    public static double MeasureVoltage(double nominal) => Instrument.Read(nominal);
}
```

That publishes a step `rail/measure_voltage`, taking a required `nominal` of
type number, described with the sentence its author already wrote. Nothing is
written twice, so nothing can drift
([ADR-0024](../../docs/adr/0024-the-signature-is-the-catalog-in-rust-too.md)).

## The process is yours

Unlike [`../python/`](../python/), you do not download a server and point it at
a folder. You create a project, reference the package, and your executable
listens:

```csharp
// Program.cs, in full.
return await Anvil.Step.StepHost
    .RunAsync(args, Anvil.Step.Generated.AnvilSteps.Register);
```

In .NET every library comes from a project with its own dependency tree
resolved against its own machine; loading several into one process of ours
means isolating them one by one, and getting that wrong does not fail at
start-up — it fails mid-sequence, with the bench energised. So the binary is
yours, the native instrument libraries you pin are yours, and you set a
breakpoint in your own step instead of attaching to a process of ours
([ADR-0038](../../docs/adr/0038-the-csharp-step-sdk-is-hosted-by-the-users-own-process.md) §2).

The promise of [ADR-0012](../../docs/adr/0012-executores-de-lenguaje-como-modulos.md)
holds and then some: you edit no line of ours, because you have none of ours.

## Running it

```sh
dotnet run --project examples/HelloBench -- --port 9201
```

| flag | |
|---|---|
| `--port` | Where to listen. Default 9201, so it does not collide with the Python executor's 9101. |
| `--bind` | Which interface. Default `127.0.0.1`; `--bind 0.0.0.0` is the remote case, and it stays explicit. |
| `--option KEY=VALUE` | Deployment configuration, repeatable, read as `ctx.Options`. Which box this executor talks to — **not** what it measures. What changes the measurement goes in the sequence, where it reaches the report ([ADR-0019](../../docs/adr/0019-que-hace-anvil-cuando-no-puede-juzgar.md), Rule 3). |
| `--list` | Prints the catalog and exits, without a bench ([ADR-0028](../../docs/adr/0028-describe-answers-without-the-bench.md)). |

There is **no `--steps`**. Python has one because it scans a folder at
start-up; here the catalog is compiled into your own executable, so there is
nothing to point at.

`ANVIL_STEP_LOG=1` gives back the host's own logging, which is quiet by
default because the executor's stderr belongs to whoever is running the
sequence.

Then point a sequence at it — [`ejemplos/csharp.yaml`](../../ejemplos/csharp.yaml)
is the worked example:

```yaml
executors:
  - { name: csharp, type: grpc, host: 127.0.0.1, port: 9201 }
```

Anvil does **not** start it. The process is yours, so you bring it up.

## What a step may take and answer

A parameter is `double`, `string`, `bool`, a `Reference`, or a class of yours
that has a `[StepConstructor]`. Anything else is a **compile error**, not an
unchecked parameter: C# always knows the type, so "unchecked" would never be
honest here. A parameter that needs structure is a badly cut step
([ADR-0020](../../docs/adr/0020-parametros-del-paso-en-la-peticion.md) §2).

Declare a `Ctx` parameter and you are handed the attempt number, the qualified
name, the executor's options and the object store. It never appears in the
catalog.

A step returns `Outcome`, `double`, `bool` or `void`:

| returns | becomes |
|---|---|
| `Outcome` | itself |
| `double` | `Outcome.Measured(...)` |
| `bool` | `pass` / `fail` — a boolean step judges the unit, it does not measure it |
| `void` | `pass` |

**The threshold is not yours.** The step measures and the engine judges the
value against the `limit` declared in the sequence
([ADR-0008](../../docs/adr/0008-limites-evaluados-por-el-motor.md)), so the same
step serves a board with another acceptance criterion without being recompiled,
and whoever audits reads the criterion in the sequence.

**`fail` is about the unit; `error` is about the bench.** A step that cannot
judge answers `Outcome.Errored`, never `pass` and never `fail` (ADR-0019,
Rule 2). An exception you let escape becomes `error` too, and does not take the
executor down.

## Instruments that stay open

A session with an instrument holds sockets and vendor driver locks, so it
cannot travel. It stays in this process, and the sequence carries a reference
to it ([ADR-0022](../../docs/adr/0022-la-referencia-a-objeto-es-un-cuarto-tipo-y-nombra-una-ranura.md)).
You write the class anyone would write:

```csharp
[StepModule("psu")]                       // optional: the default is the class name
public sealed class PowerSupply
{
    /// <summary>Opens the session with the supply.</summary>
    [StepConstructor]                     // publishes psu/open by itself
    public PowerSupply(string resource) => _link = Open(resource);

    /// <summary>Sets the output voltage, in volts.</summary>
    [Step]                                // publishes psu/set_voltage
    public Outcome SetVoltage(double volts) { ... }
}
```

The marked constructor publishes `psu/open` on its own, so nobody writes
plumbing to get the bench open, and the sequence still says out loud when it
connects. The instance steps take an implicit `psu` input of type reference,
which the SDK resolves before calling you — your method never sees a handle.

`[StepModule]` is an **override**, not a declaration: any class holding a
`[Step]` is already a module, named after the class in `snake_case`. The name
is derived so that renaming or moving a module does not mean editing its steps
(ADR-0026).

## Names

The module comes from the class and the step from the method, both in
`snake_case`, and so do the parameter names — the sequence writes those in
`inputs:`, so they follow the idiom of the YAML and not of C#. `SetVoltage`
serves `set_voltage`; `expectLit` is written `expect_lit`. Override either with
`[Step("name")]` or `[StepModule("name")]`.

## Numbers

Write them as `double` and let the SDK put them on the wire. It has one place
that formats a number and it uses the invariant culture, because a measurement
crosses as **text** and the engine parses it with `s.parse::<f64>()`: under a
Spanish locale a bare `ToString()` writes `"0,8"`, the parse yields nothing,
the engine applies no limit without a measurement, and the step's own `pass`
stands. A false green out of the operator's locale.

The package raises `CA1305` to an **error in your project too**, so formatting
a SCPI command without a culture does not compile. That is deliberate: a
malformed command reaching an instrument is worse than a lost measurement.

## Errors the compiler gives you

The catalog is built at compile time, so a step that cannot be served is a
squiggle in your editor and not a surprise on the bench
(ADR-0038 §5):

| | |
|---|---|
| `ANVIL001` | A parameter type the contract cannot carry |
| `ANVIL002` | A return type that cannot become an outcome |
| `ANVIL003` | Two steps claiming the same name |
| `ANVIL004` | A method that cannot be a step (generic, not public) |
| `ANVIL005` | An instance step whose class has no `[StepConstructor]` |
| `ANVIL006` | More than one `[StepConstructor]` in a class |
| `ANVIL007` | A name a sequence could not write |
| `ANVIL008` | A `ref`, `in` or `params` parameter |
| `ANVIL012` | *(info)* the assembly declares no steps |

## Building and testing

```sh
make test-executors-csharp        # or: dotnet test executors/csharp/Anvil.Step.sln
```

The tests are on the authoring surface and not on the wire — the same posture
as `executors/python/test_anvil_step.py`, which needs no grpcio either.

The package multi-targets `net8.0` and `net10.0`. The generator targets
`netstandard2.0`, which is not a preference: built for anything newer, Roslyn
does not load it and says nothing, and the catalog comes out empty — a legal
answer on the wire, and therefore a silent one.

## License

**Apache-2.0**, like everything under [`executors/`](../README.md), and not the
repo's AGPL ([ADR-0004](../../docs/adr/0004-licencia-dual-agpl-apache.md)): what
you *use* is AGPL, what you *link* is Apache. This package goes inside your code
the moment you write `using Anvil.Step`, and copyleft there would be copyleft
over your test steps. **Your steps and your sequences are yours.**
