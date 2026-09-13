# 3. Your first step

A step is a C# method with an attribute on it. In this chapter you write one
that pretends to measure a supply rail, and you turn `Bench/` into an executor
that serves it.

## Point the project at the SDK

Replace the contents of `Bench/Bench.csproj` with:

```xml
<Project Sdk="Microsoft.NET.Sdk">

  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
  </PropertyGroup>

  <ItemGroup>
    <ProjectReference Include="../anvil/executors/csharp/src/Anvil.Step/Anvil.Step.csproj" />
    <ProjectReference Include="../anvil/executors/csharp/src/Anvil.Step.Generator/Anvil.Step.Generator.csproj"
                      OutputItemType="Analyzer"
                      ReferenceOutputAssembly="false" />
  </ItemGroup>

</Project>
```

The two references are the SDK and its *generator*: at compile time, the
generator reads your methods and writes the list of steps your executor
publishes, so there is no registration file to keep up to date. The paths are
relative to `Bench/`, which is why the repository had to be cloned as
`anvil` next to it. When `Anvil.Step` is on NuGet, both lines become a single
`PackageReference`.

## The entry point

Replace `Bench/Program.cs` with this one line:

```csharp
return await Anvil.Step.StepHost.RunAsync(args, Anvil.Step.Generated.AnvilSteps.Register);
```

`AnvilSteps.Register` is the code the generator wrote. `StepHost.RunAsync`
reads the command line, starts a server and waits for Anvil to call.

## The step

Create `Bench/Board.cs`:

```csharp
using Anvil.Step;

/// <summary>Checks on the board under test.</summary>
public static partial class Board
{
    /// <summary>Measures the supply rail, in volts.</summary>
    [Step]
    public static double MeasureRail() => 4.98;
}
```

That is a complete step. What each part does:

- **`[Step]`** publishes the method.
- **The name** comes from the code, in `snake_case`: the class `Board` is the
  *module* `board`, the method `MeasureRail` is the step `measure_rail`, and
  sequences call it `board/measure_rail`.
- **The `<summary>`** is the step's description. Anvil shows it, so you write it
  once, next to the code it describes.
- **Returning a `double`** means "this is a measurement". Notice what is not
  here: nothing says whether 4.98 V is good. That belongs to the sequence.

The class is `partial` only so that later chapters can add steps to the same
module in files of their own.

A step can return:

| returns | means |
|---|---|
| `double` | a measurement, for the sequence's limit to judge |
| `bool` | `true` is `pass`, `false` is `fail` — the step judges, nothing is measured |
| `void` | `pass`, when the step is an action |
| `Outcome` | anything else: `Outcome.Measured(...)`, `Passed`, `Failed`, `Errored`, with a message |

## Ask the executor what it serves

```console
$ dotnet run --project Bench -- --list
life 275f5aeae20e4fc5acad7e3f418a6717, contract 4
  board/measure_rail()  — Measures the supply rail, in volts.
```

`--list` prints the catalog and exits, without starting anything. The `life`
line identifies this run of the executor and changes every time; `contract 4`
is the version of the protocol Anvil and the executor speak. The first run also
compiles the project, which takes a little while.

## Serve it

Now start it for real, and **leave this terminal open**:

```console
$ dotnet run --project Bench -- --port 9201
anvil C# executor: 1 step(s) on 127.0.0.1:9201, life bfd160da9cfc4688bd98c5eaa03a6736
```

The executor is listening on port 9201, only on your own machine
(`127.0.0.1`). Anvil does **not** start a C# executor for you: the process is
yours, so you bring it up, and you stop it with Ctrl+C.

| flag | |
|---|---|
| `--port` | where to listen; 9201 by default |
| `--bind` | which interface; `127.0.0.1` by default |
| `--list` | print the catalog and exit |

**Every time you change a step, stop the executor and start it again.** The
running process serves the code it was built from; `dotnet run` rebuilds it.
