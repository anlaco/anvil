// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

namespace Anvil.Step;

/// <summary>
/// A handle to an object this executor keeps for itself (ADR-0022).
/// </summary>
/// <remarks>
/// <para>
/// The object never crosses the wire — it holds open sockets and vendor driver
/// locks — so what travels is this, and Anvil never looks inside it.
/// </para>
/// <para>
/// <b>It names a slot, not an object.</b> Changing the state behind a reference
/// does not change its identity: a step that reconfigures the bench answers the
/// same reference it was given (ADR-0022 §5).
/// </para>
/// </remarks>
public readonly record struct Reference
{
    /// <summary>Creates a reference. Only the object store mints these.</summary>
    /// <param name="payload">The executor's own opaque key for the slot.</param>
    /// <param name="lifetime">The life of the executor that minted it.</param>
    /// <param name="executor">
    /// The name the sequence gave the owning executor. <b>Anvil stamps this</b>,
    /// not us: this process cannot know what a sequence called it.
    /// </param>
    public Reference(string payload, string lifetime, string executor = "")
    {
        Payload = payload;
        Lifetime = lifetime;
        Executor = executor;
    }

    /// <summary>The executor's own opaque key for the slot.</summary>
    public string Payload { get; }

    /// <summary>The life of the executor that minted it (ADR-0022 §6).</summary>
    public string Lifetime { get; }

    /// <summary>The name the sequence gave the owning executor; stamped by Anvil.</summary>
    public string Executor { get; }

    /// <summary>True when this is not a reference at all — the default value.</summary>
    public bool IsEmpty => string.IsNullOrEmpty(Payload);
}
