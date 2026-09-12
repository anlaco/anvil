// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;

namespace Anvil.Step;

/// <summary>What a value is, for the catalog. The vocabulary of `ValueType`.</summary>
public enum ValueKind
{
    /// <summary>Unknown, which Anvil reads as <i>unchecked</i> and never as "number".</summary>
    Unspecified = 0,

    /// <summary>A number.</summary>
    Number = 1,

    /// <summary>Text.</summary>
    Text = 2,

    /// <summary>A boolean.</summary>
    Boolean = 3,

    /// <summary>A reference to an object that stays in the executor (ADR-0022).</summary>
    Reference = 4,
}

/// <summary>
/// A typed value crossing the boundary: the four types of the contract, and no
/// more.
/// </summary>
/// <remarks>
/// No lists and no maps — a parameter that needs structure is a badly cut step
/// (ADR-0020 §2).
/// </remarks>
public readonly struct StepValue : IEquatable<StepValue>
{
    private readonly double _number;
    private readonly string? _text;
    private readonly bool _boolean;
    private readonly Reference _reference;

    private StepValue(ValueKind kind, double number, string? text, bool boolean, Reference reference)
    {
        Kind = kind;
        _number = number;
        _text = text;
        _boolean = boolean;
        _reference = reference;
    }

    /// <summary>Which of the four types this is.</summary>
    public ValueKind Kind { get; }

    /// <summary>Wraps a number.</summary>
    /// <param name="value">The number.</param>
    public static StepValue Of(double value) =>
        new(ValueKind.Number, value, null, false, default);

    /// <summary>Wraps text.</summary>
    /// <param name="value">The text.</param>
    public static StepValue Of(string value) =>
        new(ValueKind.Text, 0, value, false, default);

    /// <summary>Wraps a boolean.</summary>
    /// <param name="value">The boolean.</param>
    public static StepValue Of(bool value) =>
        new(ValueKind.Boolean, 0, null, value, default);

    /// <summary>Wraps a reference.</summary>
    /// <param name="value">The reference.</param>
    public static StepValue Of(Reference value) =>
        new(ValueKind.Reference, 0, null, false, value);

    /// <summary>The number, when <see cref="Kind"/> says so.</summary>
    /// <exception cref="InvalidOperationException">This value is not a number.</exception>
    public double AsNumber => Kind == ValueKind.Number
        ? _number
        : throw Wrong(ValueKind.Number);

    /// <summary>The text, when <see cref="Kind"/> says so.</summary>
    /// <exception cref="InvalidOperationException">This value is not text.</exception>
    public string AsText => Kind == ValueKind.Text
        ? _text!
        : throw Wrong(ValueKind.Text);

    /// <summary>The boolean, when <see cref="Kind"/> says so.</summary>
    /// <exception cref="InvalidOperationException">This value is not a boolean.</exception>
    public bool AsBoolean => Kind == ValueKind.Boolean
        ? _boolean
        : throw Wrong(ValueKind.Boolean);

    /// <summary>The reference, when <see cref="Kind"/> says so.</summary>
    /// <exception cref="InvalidOperationException">This value is not a reference.</exception>
    public Reference AsReference => Kind == ValueKind.Reference
        ? _reference
        : throw Wrong(ValueKind.Reference);

    private InvalidOperationException Wrong(ValueKind wanted) =>
        new(string.Format(
            CultureInfo.InvariantCulture,
            "value is {0}, not {1}",
            Kind.ToString().ToLowerInvariant(),
            wanted.ToString().ToLowerInvariant()));

    /// <inheritdoc/>
    public bool Equals(StepValue other) =>
        Kind == other.Kind
        && _number.Equals(other._number)
        && _text == other._text
        && _boolean == other._boolean
        && _reference.Equals(other._reference);

    /// <inheritdoc/>
    public override bool Equals(object? obj) => obj is StepValue v && Equals(v);

    /// <inheritdoc/>
    public override int GetHashCode() =>
        HashCode.Combine(Kind, _number, _text, _boolean, _reference);

    /// <summary>Compares two values.</summary>
    /// <param name="left">One value.</param>
    /// <param name="right">The other.</param>
    public static bool operator ==(StepValue left, StepValue right) => left.Equals(right);

    /// <summary>Compares two values.</summary>
    /// <param name="left">One value.</param>
    /// <param name="right">The other.</param>
    public static bool operator !=(StepValue left, StepValue right) => !left.Equals(right);
}
