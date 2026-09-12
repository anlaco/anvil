// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

namespace Anvil.Step;

/// <summary>
/// Marks a method as a step Anvil can call.
/// </summary>
/// <remarks>
/// <para>
/// There is no base class to inherit, no registration file and no name table.
/// The same mark serves a static method — a step with no state — and an instance
/// method — a step operating on a live object; the SDK tells them apart, not you
/// (ADR-0038 §3).
/// </para>
/// <para>
/// <b>The signature is the catalog.</b> Names, types and which inputs are
/// required come from the method's own parameters, and the first line of its
/// <c>///</c> comment becomes the step's description. Nothing is written twice,
/// so nothing can drift (ADR-0024).
/// </para>
/// </remarks>
[AttributeUsage(AttributeTargets.Method)]
public sealed class StepAttribute : Attribute
{
    /// <summary>Marks a step, named after the method in <c>snake_case</c>.</summary>
    public StepAttribute()
    {
    }

    /// <summary>Marks a step under a name of your own.</summary>
    /// <param name="name">
    /// The name a sequence writes, without the module. Use it when the method
    /// name is not the address you want.
    /// </param>
    public StepAttribute(string name) => Name = name;

    /// <summary>The name a sequence writes, or <see langword="null"/> to derive it.</summary>
    public string? Name { get; }
}

/// <summary>
/// Overrides the module name a class publishes its steps under.
/// </summary>
/// <remarks>
/// <para>
/// <b>It is an override, not a declaration.</b> Any class holding a
/// <see cref="StepAttribute"/> is already a module, named after the class in
/// <c>snake_case</c>. The module name is derived so that renaming or moving a
/// module does not mean editing its steps (ADR-0026, ADR-0038 §4) — reach for
/// this only when the type name is not the address you want.
/// </para>
/// </remarks>
[AttributeUsage(AttributeTargets.Class | AttributeTargets.Struct)]
public sealed class StepModuleAttribute : Attribute
{
    /// <summary>Publishes this class's steps under another module name.</summary>
    /// <param name="name">The module name a sequence writes before the <c>/</c>.</param>
    public StepModuleAttribute(string name) => Name = name;

    /// <summary>The module name a sequence writes.</summary>
    public string Name { get; }
}

/// <summary>
/// Marks the constructor that opens the object an instance step runs on, and
/// publishes it as a step of its own.
/// </summary>
/// <remarks>
/// <para>
/// With object references, every sequence would otherwise start with plumbing
/// steps that measure nothing and that somebody has to remember to write. The
/// marked constructor publishes <c>&lt;module&gt;/open</c> by itself, so the
/// step exists without anyone writing it — and the sequence still says out loud
/// when the bench is connected, which is healthy (ADR-0038 §3).
/// </para>
/// </remarks>
[AttributeUsage(AttributeTargets.Constructor)]
public sealed class StepConstructorAttribute : Attribute
{
    /// <summary>Publishes this constructor as <c>&lt;module&gt;/open</c>.</summary>
    public StepConstructorAttribute()
    {
    }

    /// <summary>Publishes this constructor under a name of your own.</summary>
    /// <param name="name">The step name, without the module.</param>
    public StepConstructorAttribute(string name) => Name = name;

    /// <summary>The step name; <c>open</c> when not given.</summary>
    public string Name { get; } = "open";
}
