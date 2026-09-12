// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;
using Xunit;

namespace Anvil.Step.Tests;

/// <summary>
/// The regression that this SDK exists to prevent.
/// </summary>
/// <remarks>
/// A measurement crosses the wire as text and the engine parses it with
/// `s.parse::&lt;f64&gt;()`. Under a comma-decimal locale a bare `ToString()`
/// writes "0,8"; the parse then yields nothing, the engine applies no limit
/// without a measurement, and the step's own `pass` stands. A false green out
/// of the operator's locale (ADR-0038 §6).
///
/// These tests are the reason `Numbers` exists. Seen to fail: dropping
/// `CultureInfo.InvariantCulture` from `Numbers.ToWire` turns the first one red
/// with "0,8".
/// </remarks>
public class CultureTests : IDisposable
{
    private readonly CultureInfo _saved = CultureInfo.CurrentCulture;

    public CultureTests()
    {
        // The locale of the machines this is written on, and of the benches.
        CultureInfo.CurrentCulture = new CultureInfo("es-ES");
    }

    public void Dispose()
    {
        CultureInfo.CurrentCulture = _saved;
        GC.SuppressFinalize(this);
    }

    [Fact]
    public void A_measurement_is_written_with_a_point_under_a_comma_locale()
    {
        Assert.Equal("0.8", Numbers.ToWire(0.8));
    }

    [Fact]
    public void The_whole_range_survives_the_locale()
    {
        Assert.Equal("4.2", Numbers.ToWire(4.2));
        Assert.Equal("-0.001", Numbers.ToWire(-0.001));
        Assert.Equal("1234.5678", Numbers.ToWire(1234.5678));
    }

    // Rust writes an integral value without a fraction (proto.rs:404-410) and
    // Python writes str(5.0) as "5.0", so the two existing executors already
    // differ. Only that `s.parse::<f64>()` accepts it is contractual.
    [Fact]
    public void An_integral_value_is_written_without_a_fraction()
    {
        Assert.Equal("5", Numbers.ToWire(5.0));
        Assert.Equal("0", Numbers.ToWire(0.0));
    }

    [Fact]
    public void No_measurement_is_the_empty_string_and_not_a_zero()
    {
        Assert.Equal(string.Empty, Numbers.ToWire((double?)null));
        Assert.Equal("0", Numbers.ToWire((double?)0.0));
    }

    // What goes out must come back: this is what the engine will do to it.
    [Fact]
    public void What_is_written_parses_back_to_the_same_double()
    {
        foreach (var original in new[] { 0.8, 4.2, -0.001, 5.0, 1e-7, 1234.5678 })
        {
            var text = Numbers.ToWire(original);
            Assert.True(double.TryParse(
                text, NumberStyles.Float, CultureInfo.InvariantCulture, out var back));
            Assert.Equal(original, back);
        }
    }

    [Fact]
    public void Reading_a_number_written_by_someone_else_is_invariant_too()
    {
        Assert.True(Numbers.TryParse("0.8", out var v));
        Assert.Equal(0.8, v);

        // "0,8" is not a number we accept: under es-ES a permissive parse would
        // read it as 0.8 here and as 8 somewhere else.
        Assert.False(Numbers.TryParse("0,8", out _));
    }
}
