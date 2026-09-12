// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;

namespace Anvil.Step;

/// <summary>
/// The only place in this package where a number becomes text, and the only
/// place where text becomes a number.
/// </summary>
/// <remarks>
/// <para>
/// A measurement crosses the wire as <b>text</b>: <c>StepResult.measured_value</c>
/// is a string (<c>crates/modelo/paso.proto:74</c>) and the engine turns it back
/// with <c>s.parse::&lt;f64&gt;()</c> (<c>crates/modelo/src/proto.rs:412-418</c>),
/// which takes a decimal point and nothing else.
/// </para>
/// <para>
/// Under a Spanish locale <c>double.ToString()</c> writes <c>"0,8"</c>. What
/// follows is not a parse error anybody sees:
/// </para>
/// <list type="number">
///   <item>the parse yields nothing, so the result reaches the engine with no
///   measurement;</item>
///   <item>the engine applies no limit when there is no measurement
///   (<c>crates/motor/src/lib.rs:1238-1239</c>);</item>
///   <item>so the status the step set stands — and a step that measured 0.8 A
///   against a 0.80 A limit reports <c>pass</c>.</item>
/// </list>
/// <para>
/// A false green out of the operator's locale, invisible to a CI running in
/// English. Hence this type, hence it being the only door, and hence CA1305
/// being a build error (ADR-0038 §6).
/// </para>
/// </remarks>
internal static class Numbers
{
    /// <summary>Writes a measurement for the wire; empty when there is none.</summary>
    internal static string ToWire(double? value) =>
        value is null ? string.Empty : ToWire(value.Value);

    /// <summary>Writes a number for the wire.</summary>
    internal static string ToWire(double value) =>
        value.ToString(CultureInfo.InvariantCulture);

    /// <summary>
    /// Reads a number written by someone else — a CLI flag, an executor option.
    /// Invariant for the same reason as <see cref="ToWire(double)"/>.
    /// </summary>
    internal static bool TryParse(string text, out double value) =>
        double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out value);
}
