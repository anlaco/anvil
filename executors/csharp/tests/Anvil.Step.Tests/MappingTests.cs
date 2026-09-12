// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;
using Xunit;

namespace Anvil.Step.Tests;

/// <summary>What actually goes on the wire.</summary>
public class MappingTests
{
    // The echo is what prevents the silent false pass: an old executor ignores
    // `inputs`, measures something else and reports "pass" (ADR-0020 §4b).
    [Fact]
    public void Every_answer_carries_the_contract_echo()
    {
        Assert.Equal(4, Mapping.ToWire("m", Outcome.Passed()).Contract);
        Assert.Equal(4, Mapping.ToWire(new Registry(), "life").Contract);
    }

    [Fact]
    public void A_measurement_crosses_as_text_and_the_limits_stay_empty()
    {
        var result = Mapping.ToWire("psu/measure_current", Outcome.Measured(0.8));

        Assert.Equal("0.8", result.MeasuredValue);

        // The threshold is the engine's; the step does not know it (ADR-0008).
        Assert.Equal(string.Empty, result.LimitMin);
        Assert.Equal(string.Empty, result.LimitMax);
    }

    [Fact]
    public void A_measurement_written_under_a_comma_locale_still_crosses_with_a_point()
    {
        var saved = CultureInfo.CurrentCulture;
        try
        {
            CultureInfo.CurrentCulture = new CultureInfo("es-ES");
            Assert.Equal("0.8", Mapping.ToWire("m", Outcome.Measured(0.8)).MeasuredValue);
        }
        finally
        {
            CultureInfo.CurrentCulture = saved;
        }
    }

    [Fact]
    public void No_measurement_is_an_empty_string_and_not_a_zero()
    {
        Assert.Equal(string.Empty, Mapping.ToWire("m", Outcome.Passed()).MeasuredValue);
    }

    // Anvil stamps the executor name; this process cannot know what a sequence
    // called it (paso.proto:21).
    [Fact]
    public void A_reference_travels_without_an_executor_name()
    {
        var outcome = Outcome.Passed().Output("psu", new Reference("s1", "life-abc"));

        var wire = Mapping.ToWire("psu/open", outcome).Outputs[0];

        Assert.Equal("s1", wire.Reference.Payload);
        Assert.Equal("life-abc", wire.Reference.Lifetime);
        Assert.Equal(string.Empty, wire.Reference.Executor);
    }

    // proto3 makes silence false, and false means "do not check me". An
    // executor serving nothing has to say so out loud (ADR-0021 §4).
    [Fact]
    public void An_empty_catalog_still_says_it_describes_itself()
    {
        var catalog = Mapping.ToWire(new Registry(), "life-abc");

        Assert.True(catalog.Describes);
        Assert.Empty(catalog.Steps);
        Assert.Equal("life-abc", catalog.Lifetime);
    }

    [Fact]
    public void A_step_publishes_its_signature_with_types_and_docs()
    {
        var registry = new Registry();
        registry.Add(new StepDescriptor(
            "psu",
            "set_voltage",
            [new ParameterSpec("volts", ValueKind.Number, true, null, "The output voltage.")],
            [new OutputSpec("applied", ValueKind.Number, "What was applied.")],
            "Sets the output voltage.",
            (_, _) => Outcome.Passed()));

        var step = Mapping.ToWire(registry, "life").Steps[0];

        Assert.Equal("psu/set_voltage", step.Name);
        Assert.Equal("Sets the output voltage.", step.Doc);
        Assert.Equal("volts", step.Inputs[0].Name);
        Assert.Equal(global::ValueType.Number, step.Inputs[0].Type);
        Assert.True(step.Inputs[0].Required);
        Assert.Equal("The output voltage.", step.Inputs[0].Doc);
        Assert.Equal("applied", step.Outputs[0].Name);
    }

    // An unset oneof is an error and not a zero (ADR-0020 §2).
    [Fact]
    public void A_value_with_no_branch_set_arrives_unspecified_and_not_as_zero()
    {
        Assert.Equal(ValueKind.Unspecified, Mapping.FromWire(new global::Value { Name = "x" }).Kind);
    }

    [Fact]
    public void The_four_types_round_trip()
    {
        Assert.Equal(1.5, Mapping.FromWire(Mapping.ToWire("n", StepValue.Of(1.5))).AsNumber);
        Assert.Equal("t", Mapping.FromWire(Mapping.ToWire("s", StepValue.Of("t"))).AsText);
        Assert.True(Mapping.FromWire(Mapping.ToWire("b", StepValue.Of(true))).AsBoolean);
        Assert.Equal(
            "s3",
            Mapping.FromWire(Mapping.ToWire("r", StepValue.Of(new Reference("s3", "l")))).AsReference.Payload);
    }
}
