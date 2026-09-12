// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;

namespace Anvil.Step;

/// <summary>Two steps claiming the same qualified name.</summary>
public sealed class DuplicateStepException : Exception
{
    /// <summary>Creates the exception.</summary>
    /// <param name="message">Which name was claimed twice.</param>
    public DuplicateStepException(string message) : base(message)
    {
    }

    /// <summary>Creates the exception.</summary>
    public DuplicateStepException()
    {
    }

    /// <summary>Creates the exception.</summary>
    /// <param name="message">Which name was claimed twice.</param>
    /// <param name="innerException">What caused it.</param>
    public DuplicateStepException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}

/// <summary>The steps this executor serves, by qualified name.</summary>
public sealed class Registry
{
    /// <summary>What separates the module from the step: <c>/</c>.</summary>
    public const string ModuleSeparator = "/";

    private readonly Dictionary<string, StepDescriptor> _steps = new(StringComparer.Ordinal);

    /// <summary>The steps, in the order they were added.</summary>
    public IReadOnlyCollection<StepDescriptor> Steps => _steps.Values;

    /// <summary>Adds a step.</summary>
    /// <param name="step">The step to serve.</param>
    /// <exception cref="DuplicateStepException">
    /// The name is already taken. Refused rather than served, because an
    /// ambiguous catalog means the bench runs whichever of the two happened to
    /// register last.
    /// </exception>
    public void Add(StepDescriptor step)
    {
        ArgumentNullException.ThrowIfNull(step);
        if (!_steps.TryAdd(step.Qualified, step))
        {
            throw new DuplicateStepException(string.Format(
                CultureInfo.InvariantCulture,
                "two steps are called '{0}'; a catalog that answers to the same name twice "
                + "runs whichever registered last",
                step.Qualified));
        }
    }

    /// <summary>Finds a step by the name the sequence used.</summary>
    /// <param name="qualified">The <c>module/step</c> name.</param>
    /// <returns>The step, or <see langword="null"/> when nothing serves that name.</returns>
    public StepDescriptor? Find(string qualified) =>
        _steps.TryGetValue(qualified, out var step) ? step : null;

    /// <summary>
    /// Names close to one that was not found, for the message that says so.
    /// </summary>
    /// <param name="qualified">The name the sequence asked for.</param>
    /// <remarks>
    /// A typo in a sequence is found with the unit already on the bench often
    /// enough; naming the near miss costs nothing here.
    /// </remarks>
    public IEnumerable<string> Near(string qualified) =>
        _steps.Keys.Where(k => Distance(k, qualified) <= 2).OrderBy(k => k, StringComparer.Ordinal);

    private static int Distance(string a, string b)
    {
        var previous = new int[b.Length + 1];
        var current = new int[b.Length + 1];
        for (var j = 0; j <= b.Length; j++)
        {
            previous[j] = j;
        }

        for (var i = 1; i <= a.Length; i++)
        {
            current[0] = i;
            for (var j = 1; j <= b.Length; j++)
            {
                var cost = a[i - 1] == b[j - 1] ? 0 : 1;
                current[j] = Math.Min(
                    Math.Min(current[j - 1] + 1, previous[j] + 1),
                    previous[j - 1] + cost);
            }

            (previous, current) = (current, previous);
        }

        return previous[b.Length];
    }
}
