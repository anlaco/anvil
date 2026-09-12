// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

namespace Anvil.Step;

/// <summary>One input of a step, as the catalog describes it.</summary>
/// <param name="Name">The name a sequence writes in <c>inputs:</c>.</param>
/// <param name="Kind">What the value is.</param>
/// <param name="Required">Whether the sequence has to send it.</param>
/// <param name="Default">
/// Declared <b>for the reader and for the editor, not for the engine</b>: the
/// engine never sends it, the step applies its own (ADR-0021 §5).
/// </param>
/// <param name="Doc">The <c>&lt;param&gt;</c> line of the method's comment.</param>
public sealed record ParameterSpec(
    string Name,
    ValueKind Kind,
    bool Required,
    StepValue? Default = null,
    string Doc = "");

/// <summary>
/// One named output of a step — what <c>assign</c> reads as
/// <c>result.outputs.&lt;name&gt;</c>.
/// </summary>
/// <param name="Name">The name <c>assign</c> writes.</param>
/// <param name="Kind">What the value is.</param>
/// <param name="Doc">What it is, for the reader.</param>
/// <remarks>
/// Declaring it is what lets Anvil check that expression without running the
/// sequence (ADR-0020 §3).
/// </remarks>
public sealed record OutputSpec(string Name, ValueKind Kind, string Doc = "");

/// <summary>One step of the catalog: the name a sequence uses and the shape of the call.</summary>
public sealed class StepDescriptor
{
    /// <summary>Describes a step.</summary>
    /// <param name="module">The module, derived from the class.</param>
    /// <param name="name">The step name, derived from the method.</param>
    /// <param name="inputs">What it takes.</param>
    /// <param name="outputs">What it answers besides the measurement.</param>
    /// <param name="doc">The first line of the method's <c>///</c> comment.</param>
    /// <param name="invoke">How to call it.</param>
    public StepDescriptor(
        string module,
        string name,
        IReadOnlyList<ParameterSpec> inputs,
        IReadOnlyList<OutputSpec> outputs,
        string doc,
        Func<Ctx, Inputs, Outcome> invoke)
    {
        Module = module;
        Name = name;
        Inputs = inputs;
        Outputs = outputs;
        Doc = doc;
        Invoke = invoke;
    }

    /// <summary>The module this step lives in.</summary>
    public string Module { get; }

    /// <summary>The step's own name, without the module.</summary>
    public string Name { get; }

    /// <summary>
    /// The name a sequence writes: <c>module/step</c>.
    /// </summary>
    /// <remarks>
    /// Qualified always, never bare: two modules can each serve a
    /// <c>measure_voltage</c>, which is the whole point, so the module is part of
    /// the address and not decoration (ADR-0025, ADR-0026).
    /// </remarks>
    public string Qualified => string.IsNullOrEmpty(Module)
        ? Name
        : Module + Registry.ModuleSeparator + Name;

    /// <summary>What it takes.</summary>
    public IReadOnlyList<ParameterSpec> Inputs { get; }

    /// <summary>What it answers besides the measurement.</summary>
    public IReadOnlyList<OutputSpec> Outputs { get; }

    /// <summary>What the step does, from the first line of its comment.</summary>
    public string Doc { get; }

    /// <summary>How to call it. The generator writes this; nobody else does.</summary>
    public Func<Ctx, Inputs, Outcome> Invoke { get; }
}
