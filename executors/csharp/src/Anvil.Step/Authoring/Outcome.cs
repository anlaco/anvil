// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Collections.ObjectModel;
using System.Globalization;

namespace Anvil.Step;

/// <summary>
/// What a step answers: a verdict, what it measured, and anything else it wants
/// on the report.
/// </summary>
/// <remarks>
/// <para>
/// <b>The threshold is not here.</b> A step measures and the engine judges the
/// value against the <c>limit</c> declared in the sequence (ADR-0008), so the
/// same step serves a board with a different acceptance criterion without being
/// recompiled — and whoever audits reads the criterion in the sequence.
/// </para>
/// <para>
/// The vocabulary is closed: <c>pass</c>, <c>fail</c> and <c>error</c>. A step
/// never answers <c>skipped</c>; only the engine produces that.
/// </para>
/// </remarks>
public sealed class Outcome
{
    /// <summary>The step met its criterion.</summary>
    public const string Pass = "pass";

    /// <summary>The unit does not comply — information about the DUT.</summary>
    public const string Fail = "fail";

    /// <summary>Could not be judged — information about the bench or the step.</summary>
    public const string Error = "error";

    private readonly List<KeyValuePair<string, StepValue>> _outputs;

    private Outcome(string status, string message, double? measured, List<KeyValuePair<string, StepValue>>? outputs)
    {
        Status = status;
        Message = message;
        MeasuredValue = measured;
        _outputs = outputs ?? [];
    }

    /// <summary>The verdict: one of <see cref="Pass"/>, <see cref="Fail"/>, <see cref="Error"/>.</summary>
    public string Status { get; }

    /// <summary>Free text for the report: what the step did, or why it could not.</summary>
    public string Message { get; }

    /// <summary>
    /// The measurement, if the step measures. A <see langword="double"/> and never
    /// a string: how it is written on the wire is the SDK's business, and getting
    /// that wrong loses the measurement silently (ADR-0038 §6).
    /// </summary>
    public double? MeasuredValue { get; }

    /// <summary>
    /// Named values the step returns <b>besides</b> the measurement. They take no
    /// part in the verdict; <c>assign</c> reads them as
    /// <c>result.outputs.&lt;name&gt;</c> (ADR-0020 §3).
    /// </summary>
    public IReadOnlyList<KeyValuePair<string, StepValue>> Outputs =>
        new ReadOnlyCollection<KeyValuePair<string, StepValue>>(_outputs);

    /// <summary>The step met its criterion.</summary>
    /// <param name="message">What it did, for the report.</param>
    public static Outcome Passed(string message = "") => new(Pass, message, null, null);

    /// <summary>
    /// The unit does not comply. <b>Information about the DUT</b>, never about the
    /// bench (ADR-0019, Rule 2).
    /// </summary>
    /// <param name="message">Why the unit does not comply.</param>
    public static Outcome Failed(string message = "") => new(Fail, message, null, null);

    /// <summary>
    /// The step could not judge. <b>Information about the bench, the step or the
    /// configuration</b> — never about the unit (ADR-0019, Rule 2).
    /// </summary>
    /// <param name="message">Why it could not judge.</param>
    public static Outcome Errored(string message = "") => new(Error, message, null, null);

    /// <summary>
    /// The step measured. The engine judges the value against the sequence's
    /// <c>limit</c>; the step does not.
    /// </summary>
    /// <param name="value">What was measured.</param>
    /// <param name="message">What was measured and how, for the report.</param>
    /// <remarks>
    /// A value that is not finite answers <see cref="Errored"/> instead. A NaN
    /// crossing the wire is a measurement nobody can reconstruct afterwards, and
    /// an unreconstructable measurement is a defect even when every test passes
    /// (ADR-0019, Rule 3).
    /// </remarks>
    public static Outcome Measured(double value, string message = "") =>
        double.IsFinite(value)
            ? new Outcome(Pass, message, value, null)
            : Errored(string.Format(
                CultureInfo.InvariantCulture,
                "the step measured {0}, which is not a value a report can carry",
                double.IsNaN(value) ? "NaN" : (value > 0 ? "+infinity" : "-infinity")));

    /// <summary>Adds a named number to the outputs.</summary>
    /// <param name="name">The name <c>assign</c> will read.</param>
    /// <param name="value">The value.</param>
    public Outcome Output(string name, double value) => Add(name, StepValue.Of(value));

    /// <summary>Adds a named text to the outputs.</summary>
    /// <param name="name">The name <c>assign</c> will read.</param>
    /// <param name="value">The value.</param>
    public Outcome Output(string name, string value) => Add(name, StepValue.Of(value));

    /// <summary>Adds a named boolean to the outputs.</summary>
    /// <param name="name">The name <c>assign</c> will read.</param>
    /// <param name="value">The value.</param>
    public Outcome Output(string name, bool value) => Add(name, StepValue.Of(value));

    /// <summary>Adds a named reference to the outputs — how a handle reaches the sequence.</summary>
    /// <param name="name">The name <c>assign</c> will read.</param>
    /// <param name="value">The reference.</param>
    public Outcome Output(string name, Reference value) => Add(name, StepValue.Of(value));

    private Outcome Add(string name, StepValue value)
    {
        ArgumentException.ThrowIfNullOrEmpty(name);
        var copy = new List<KeyValuePair<string, StepValue>>(_outputs)
        {
            new(name, value),
        };
        return new Outcome(Status, Message, MeasuredValue, copy);
    }
}
