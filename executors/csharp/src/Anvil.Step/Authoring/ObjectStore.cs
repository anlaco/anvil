// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Collections.Concurrent;
using System.Globalization;

namespace Anvil.Step;

/// <summary>A reference that cannot be resolved: wrong life, spent slot, or no slot.</summary>
public sealed class ObjectStoreException : Exception
{
    /// <summary>Creates the exception.</summary>
    /// <param name="message">Why the reference could not be resolved.</param>
    public ObjectStoreException(string message) : base(message)
    {
    }

    /// <summary>Creates the exception.</summary>
    public ObjectStoreException()
    {
    }

    /// <summary>Creates the exception.</summary>
    /// <param name="message">Why the reference could not be resolved.</param>
    /// <param name="innerException">What caused it.</param>
    public ObjectStoreException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}

/// <summary>
/// The objects this executor keeps for itself, and the handles the sequence
/// carries to them (ADR-0022).
/// </summary>
/// <remarks>
/// <para>
/// A bench, an instrument session, a connection: a thing with open sockets and
/// vendor driver locks that cannot travel and must not be reopened per step. It
/// stays here, and what crosses the wire is a <see cref="Reference"/>.
/// </para>
/// <para>
/// This type is where the <b>two duties the contract cannot check for us</b>
/// live (ADR-0022 §7):
/// </para>
/// <list type="number">
///   <item><b>A payload is never recycled within one life.</b> The counter only
///   goes up and a closed key is remembered, because a recycled key would let an
///   old reference resolve cleanly to a live, <i>different</i> instrument — same
///   executor, same life, everything green, measuring the wrong bench. That is
///   the one failure Anvil cannot see from outside.</item>
///   <item><b>A new and different life on every start.</b> Minted in the
///   constructor and published in the catalog, so Anvil can find out that the
///   process holding its references died and was born again.</item>
/// </list>
/// </remarks>
public sealed class ObjectStore
{
    private readonly ConcurrentDictionary<string, object> _slots = new(StringComparer.Ordinal);
    private readonly ConcurrentDictionary<string, byte> _spent = new(StringComparer.Ordinal);
    private long _next;

    /// <summary>Creates a store with a fresh life.</summary>
    public ObjectStore() => Lifetime = Guid.NewGuid().ToString("N");

    /// <summary>Creates a store with a life given to it. For tests.</summary>
    /// <param name="lifetime">The life to publish.</param>
    internal ObjectStore(string lifetime) => Lifetime = lifetime;

    /// <summary>
    /// This process's life (ADR-0022 §6): minted once, at start-up, and published
    /// in the catalog. Never a "session" and never an "epoch".
    /// </summary>
    public string Lifetime { get; }

    /// <summary>How many slots are open right now.</summary>
    public int OpenCount => _slots.Count;

    /// <summary>Puts an object in a new slot and hands back the handle to it.</summary>
    /// <param name="value">The object that stays here.</param>
    /// <returns>The reference a step returns in its outputs.</returns>
    public Reference New(object value)
    {
        ArgumentNullException.ThrowIfNull(value);
        var key = string.Create(
            CultureInfo.InvariantCulture,
            $"s{Interlocked.Increment(ref _next)}");
        _slots[key] = value;
        return new Reference(key, Lifetime);
    }

    /// <summary>Resolves a reference to the object behind it.</summary>
    /// <param name="reference">The handle the sequence sent.</param>
    /// <exception cref="ObjectStoreException">The reference does not resolve.</exception>
    public object Get(Reference reference)
    {
        Check(reference);
        return _slots.TryGetValue(reference.Payload, out var value)
            ? value
            : throw Gone(reference);
    }

    /// <summary>Resolves a reference to the object behind it, as a given type.</summary>
    /// <typeparam name="T">What the object should be.</typeparam>
    /// <param name="reference">The handle the sequence sent.</param>
    /// <exception cref="ObjectStoreException">
    /// The reference does not resolve, or holds something else. A slot holding
    /// another type is a bench error and never a verdict on the unit.
    /// </exception>
    public T Get<T>(Reference reference)
    {
        var value = Get(reference);
        return value is T typed
            ? typed
            : throw new ObjectStoreException(string.Format(
                CultureInfo.InvariantCulture,
                "reference '{0}' holds a {1}, not a {2}",
                reference.Payload,
                value.GetType().Name,
                typeof(T).Name));
    }

    /// <summary>Empties a slot and spends its key for good.</summary>
    /// <param name="reference">The handle to close.</param>
    /// <returns>The object that was in it, so the caller can dispose of it.</returns>
    /// <exception cref="ObjectStoreException">The reference does not resolve.</exception>
    /// <remarks>
    /// The key is never handed out again, not even to the next open. An old
    /// reference resolving to a live, different bench is the one failure Anvil
    /// cannot see from outside (ADR-0022 §7).
    /// </remarks>
    public object Close(Reference reference)
    {
        Check(reference);
        if (!_slots.TryRemove(reference.Payload, out var value))
        {
            throw Gone(reference);
        }

        _spent[reference.Payload] = 0;
        return value;
    }

    private void Check(Reference reference)
    {
        if (reference.IsEmpty)
        {
            throw new ObjectStoreException("no reference was given");
        }

        // An empty life is not waved through: it would mean trusting a handle
        // whose origin we cannot place.
        if (!string.Equals(reference.Lifetime, Lifetime, StringComparison.Ordinal))
        {
            throw new ObjectStoreException(string.Format(
                CultureInfo.InvariantCulture,
                "reference '{0}' belongs to life '{1}', and this executor is life '{2}': "
                + "the process that minted it is gone",
                reference.Payload,
                reference.Lifetime,
                Lifetime));
        }
    }

    private ObjectStoreException Gone(Reference reference) =>
        new(_spent.ContainsKey(reference.Payload)
            ? string.Format(
                CultureInfo.InvariantCulture,
                "reference '{0}' was closed; its slot is spent and is never reused",
                reference.Payload)
            : string.Format(
                CultureInfo.InvariantCulture,
                "reference '{0}' names no slot in this executor",
                reference.Payload));
}
