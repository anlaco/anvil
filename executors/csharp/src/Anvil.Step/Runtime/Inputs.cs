// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;

namespace Anvil.Step;

/// <summary>
/// The values the engine sent for one call, read by name and by type.
/// </summary>
/// <remarks>
/// Named rather than positional: sequences are written and reviewed by hand,
/// and reordering two parameters must not change what the bench measures.
/// </remarks>
public sealed class Inputs
{
    private readonly IReadOnlyDictionary<string, StepValue> _values;

    /// <summary>Wraps the values of one call.</summary>
    /// <param name="values">What the engine sent, by name.</param>
    public Inputs(IReadOnlyDictionary<string, StepValue> values) => _values = values;

    /// <summary>An empty set of inputs.</summary>
    public static Inputs Empty { get; } = new(new Dictionary<string, StepValue>(StringComparer.Ordinal));

    /// <summary>Whether the engine sent this one.</summary>
    /// <param name="name">The parameter name.</param>
    public bool Has(string name) => _values.ContainsKey(name);

    /// <summary>Reads a required number.</summary>
    /// <param name="name">The parameter name.</param>
    /// <exception cref="StepInputException">Missing, or not a number.</exception>
    public double Number(string name) => Required(name, ValueKind.Number).AsNumber;

    /// <summary>Reads a required text.</summary>
    /// <param name="name">The parameter name.</param>
    /// <exception cref="StepInputException">Missing, or not text.</exception>
    public string Text(string name) => Required(name, ValueKind.Text).AsText;

    /// <summary>Reads a required boolean.</summary>
    /// <param name="name">The parameter name.</param>
    /// <exception cref="StepInputException">Missing, or not a boolean.</exception>
    public bool Boolean(string name) => Required(name, ValueKind.Boolean).AsBoolean;

    /// <summary>Reads a required reference.</summary>
    /// <param name="name">The parameter name.</param>
    /// <exception cref="StepInputException">Missing, or not a reference.</exception>
    public Reference Reference(string name) => Required(name, ValueKind.Reference).AsReference;

    /// <summary>Reads an optional number; <see langword="null"/> when not sent.</summary>
    /// <param name="name">The parameter name.</param>
    public double? OptionalNumber(string name) => Optional(name, ValueKind.Number)?.AsNumber;

    /// <summary>Reads an optional text; <see langword="null"/> when not sent.</summary>
    /// <param name="name">The parameter name.</param>
    public string? OptionalText(string name) => Optional(name, ValueKind.Text)?.AsText;

    /// <summary>Reads an optional boolean; <see langword="null"/> when not sent.</summary>
    /// <param name="name">The parameter name.</param>
    public bool? OptionalBoolean(string name) => Optional(name, ValueKind.Boolean)?.AsBoolean;

    private StepValue Required(string name, ValueKind wanted)
    {
        if (!_values.TryGetValue(name, out var value))
        {
            throw new StepInputException(string.Format(
                CultureInfo.InvariantCulture,
                "the sequence did not send '{0}', which this step requires",
                name));
        }

        return Typed(name, value, wanted);
    }

    private StepValue? Optional(string name, ValueKind wanted) =>
        _values.TryGetValue(name, out var value) ? Typed(name, value, wanted) : null;

    private static StepValue Typed(string name, StepValue value, ValueKind wanted)
    {
        // An unset `oneof` is an error and not a zero (ADR-0020 §2): reading it
        // as one would be measuring against a threshold nobody declared.
        if (value.Kind == ValueKind.Unspecified)
        {
            throw new StepInputException(string.Format(
                CultureInfo.InvariantCulture,
                "'{0}' arrived with no value set; an empty value is not a zero",
                name));
        }

        return value.Kind == wanted
            ? value
            : throw new StepInputException(string.Format(
                CultureInfo.InvariantCulture,
                "'{0}' is {1}, and this step takes {2}",
                name,
                value.Kind.ToString().ToLowerInvariant(),
                wanted.ToString().ToLowerInvariant()));
    }
}

/// <summary>An input the step cannot use: missing, untyped, or of another type.</summary>
public sealed class StepInputException : Exception
{
    /// <summary>Creates the exception.</summary>
    /// <param name="message">What was wrong with the input.</param>
    public StepInputException(string message) : base(message)
    {
    }

    /// <summary>Creates the exception.</summary>
    public StepInputException()
    {
    }

    /// <summary>Creates the exception.</summary>
    /// <param name="message">What was wrong with the input.</param>
    /// <param name="innerException">What caused it.</param>
    public StepInputException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}
