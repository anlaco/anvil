// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using Xunit;

namespace Anvil.Step.Tests;

/// <summary>What a step answers, and what it is not allowed to answer.</summary>
public class OutcomeTests
{
    [Fact]
    public void The_vocabulary_is_closed_and_lowercase()
    {
        Assert.Equal("pass", Outcome.Passed().Status);
        Assert.Equal("fail", Outcome.Failed("the rail is low").Status);
        Assert.Equal("error", Outcome.Errored("no answer from the bench").Status);
    }

    [Fact]
    public void A_measurement_passes_and_carries_its_value()
    {
        var o = Outcome.Measured(4.2, "rail measured");

        Assert.Equal("pass", o.Status);
        Assert.Equal(4.2, o.MeasuredValue);
        Assert.Equal("rail measured", o.Message);
    }

    // A NaN reaching the report is a measurement nobody can reconstruct later,
    // which ADR-0019 Rule 3 makes a defect even when every test passes.
    [Theory]
    [InlineData(double.NaN)]
    [InlineData(double.PositiveInfinity)]
    [InlineData(double.NegativeInfinity)]
    public void A_measurement_that_is_not_finite_is_an_error_not_a_measurement(double value)
    {
        var o = Outcome.Measured(value);

        Assert.Equal("error", o.Status);
        Assert.Null(o.MeasuredValue);
    }

    [Fact]
    public void Outputs_keep_their_order_and_their_types()
    {
        var o = Outcome.Measured(1.0)
            .Output("channel_used", 2.0)
            .Output("serial", "SN-0042")
            .Output("warmed_up", true);

        Assert.Equal(3, o.Outputs.Count);
        Assert.Equal("channel_used", o.Outputs[0].Key);
        Assert.Equal(2.0, o.Outputs[0].Value.AsNumber);
        Assert.Equal("SN-0042", o.Outputs[1].Value.AsText);
        Assert.True(o.Outputs[2].Value.AsBoolean);
    }

    // An Outcome is immutable: `Output` answers a new one. A step that built a
    // result and then added to it must not mutate what it already returned.
    [Fact]
    public void Adding_an_output_does_not_touch_the_outcome_it_came_from()
    {
        var first = Outcome.Passed();
        var second = first.Output("x", 1.0);

        Assert.Empty(first.Outputs);
        Assert.Single(second.Outputs);
    }

    [Fact]
    public void An_output_with_no_name_is_refused()
    {
        Assert.Throws<ArgumentException>(() => Outcome.Passed().Output("", 1.0));
    }
}
