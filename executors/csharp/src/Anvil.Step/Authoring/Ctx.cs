// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

namespace Anvil.Step;

/// <summary>
/// The executor talking to the step: which attempt this is, what the sequence
/// called it, the executor's options, and the objects that stay here.
/// </summary>
/// <remarks>
/// <para>
/// A step gets one only if it asks for one — declare a <see cref="Ctx"/>
/// parameter and it is handed over. It is never part of the described signature,
/// so it costs nothing in the catalog.
/// </para>
/// <para>
/// Inputs arrive <b>already evaluated</b>: the engine resolves the
/// <c>${...}</c> expressions of the YAML against its environment before calling
/// (ADR-0009). A step never sees <c>locals</c>; it is handed values.
/// </para>
/// </remarks>
public sealed class Ctx
{
    /// <summary>Creates a context. The SDK builds these, not you.</summary>
    /// <param name="name">The qualified name the sequence used.</param>
    /// <param name="attempt">Which attempt this is, counting from 1.</param>
    /// <param name="options">The executor's options.</param>
    /// <param name="objects">The objects that stay in this executor.</param>
    /// <param name="reference">The handle that resolved an instance step, if any.</param>
    public Ctx(
        string name,
        int attempt,
        IReadOnlyDictionary<string, string> options,
        ObjectStore objects,
        Reference? reference = null)
    {
        Name = name;
        Attempt = attempt;
        Options = options;
        Objects = objects;
        Reference = reference;
    }

    /// <summary>The qualified name the sequence used: <c>module/step</c>.</summary>
    public string Name { get; }

    /// <summary>Which attempt this is, counting from 1 (RF-09).</summary>
    public int Attempt { get; }

    /// <summary>
    /// The executor's options, from <c>--option KEY=VALUE</c>.
    /// </summary>
    /// <remarks>
    /// These are <b>deployment configuration</b> — which box this executor talks
    /// to — and not a condition of the measurement. What changes what is measured
    /// goes in the sequence, where it ends up in the report (ADR-0019, Rule 3).
    /// </remarks>
    public IReadOnlyDictionary<string, string> Options { get; }

    /// <summary>The objects this executor keeps for itself (ADR-0022).</summary>
    public ObjectStore Objects { get; }

    /// <summary>
    /// The handle that resolved this step's object, when it is an instance step.
    /// A step that closes its own object needs it; most steps do not.
    /// </summary>
    public Reference? Reference { get; }
}
