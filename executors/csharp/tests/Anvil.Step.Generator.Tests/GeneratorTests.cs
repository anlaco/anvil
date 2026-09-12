// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using Xunit;

namespace Anvil.Step.Generator.Tests;

/// <summary>
/// The generator, driven by the real compiler over real source.
/// </summary>
public class GeneratorTests
{
    [Fact]
    public void A_static_method_becomes_a_step_named_after_its_class_and_itself()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public static class Rail
            {
                /// <summary>Measures the rail voltage.</summary>
                /// <param name="nominal">What it should read.</param>
                [Step]
                public static double MeasureVoltage(double nominal) => nominal;
            }
            """);

        Assert.Empty(result.Ids);
        Assert.Contains("\"rail\"", result.Source, StringComparison.Ordinal);
        Assert.Contains("\"measure_voltage\"", result.Source, StringComparison.Ordinal);
        Assert.Contains("\"nominal\"", result.Source, StringComparison.Ordinal);
    }

    // The description comes from the comment its author already wrote, so it
    // cannot drift from the code (ADR-0024).
    [Fact]
    public void The_doc_comment_becomes_the_steps_description()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public static class Rail
            {
                /// <summary>Measures the rail voltage.</summary>
                /// <param name="nominal">What it should read, in volts.</param>
                [Step]
                public static double MeasureVoltage(double nominal) => nominal;
            }
            """);

        Assert.Contains("Measures the rail voltage.", result.Source, StringComparison.Ordinal);
        Assert.Contains("What it should read, in volts.", result.Source, StringComparison.Ordinal);
    }

    // The sequence writes the parameter name in `inputs:`, so it follows the
    // idiom of the YAML and not of C#.
    [Fact]
    public void Parameter_names_go_to_snake_case()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public static class Board
            {
                /// <summary>Checks the pilot light.</summary>
                [Step]
                public static Outcome CheckLed(bool expectLit) => Outcome.Passed();
            }
            """);

        Assert.Contains("\"expect_lit\"", result.Source, StringComparison.Ordinal);
        Assert.DoesNotContain("\"expectLit\"", result.Source, StringComparison.Ordinal);
    }

    [Fact]
    public void A_parameter_with_a_default_is_optional_and_publishes_it()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public static class Rail
            {
                /// <summary>Measures.</summary>
                [Step]
                public static double Measure(double nominal, double droop = 0.01) => nominal;
            }
            """);

        // Required, then optional with its declared default — which is for the
        // reader and the editor, not for the engine (ADR-0021 §5).
        Assert.Contains("\"nominal\", global::Anvil.Step.ValueKind.Number, true", result.Source, StringComparison.Ordinal);
        Assert.Contains("\"droop\", global::Anvil.Step.ValueKind.Number, false", result.Source, StringComparison.Ordinal);
        Assert.Contains("0.01", result.Source, StringComparison.Ordinal);
    }

    [Fact]
    public void A_marked_constructor_publishes_open_and_its_instance_steps_take_a_handle()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            [StepModule("psu")]
            public sealed class PowerSupply
            {
                /// <summary>Opens the supply.</summary>
                [StepConstructor]
                public PowerSupply(string resource) { }

                /// <summary>Sets the voltage.</summary>
                [Step]
                public Outcome SetVoltage(double volts) => Outcome.Passed();
            }
            """);

        Assert.Empty(result.Ids);
        Assert.Contains("\"open\"", result.Source, StringComparison.Ordinal);

        // The instance step takes the handle to the object it runs on, and the
        // SDK resolves it before the call: the method never sees a Reference.
        Assert.Contains("\"psu\", global::Anvil.Step.ValueKind.Reference, true", result.Source, StringComparison.Ordinal);
        Assert.Contains("__ctx.Objects.Get<", result.Source, StringComparison.Ordinal);
    }

    // [StepModule] is an override: the default is the class name, so renaming a
    // module does not mean editing its steps (ADR-0026).
    [Fact]
    public void The_module_name_is_derived_from_the_class_when_not_overridden()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public static class PowerSupply
            {
                /// <summary>Reads.</summary>
                [Step]
                public static double Read() => 1.0;
            }
            """);

        Assert.Contains("\"power_supply\"", result.Source, StringComparison.Ordinal);
    }

    [Fact]
    public void A_parameter_the_contract_cannot_carry_is_ANVIL001()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            using System;
            public static class Bad
            {
                /// <summary>Takes something that cannot travel.</summary>
                [Step]
                public static Outcome Go(DateTime when) => Outcome.Passed();
            }
            """);

        Assert.Contains("ANVIL001", result.Ids);
    }

    [Fact]
    public void A_return_type_that_cannot_become_an_outcome_is_ANVIL002()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public static class Bad
            {
                /// <summary>Answers something that is not a verdict.</summary>
                [Step]
                public static string Go() => "nope";
            }
            """);

        Assert.Contains("ANVIL002", result.Ids);
    }

    [Fact]
    public void Two_steps_with_one_name_is_ANVIL003()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public static class Rail
            {
                /// <summary>One.</summary>
                [Step("measure")]
                public static double A() => 1.0;

                /// <summary>Two.</summary>
                [Step("measure")]
                public static double B() => 2.0;
            }
            """);

        Assert.Contains("ANVIL003", result.Ids);
    }

    // An instance step whose class cannot be opened could never run.
    [Fact]
    public void An_instance_step_with_no_constructor_is_ANVIL005()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public sealed class Orphan
            {
                /// <summary>Runs on an object nothing can open.</summary>
                [Step]
                public Outcome Go() => Outcome.Passed();
            }
            """);

        Assert.Contains("ANVIL005", result.Ids);
    }

    [Fact]
    public void A_ref_parameter_is_ANVIL008()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public static class Bad
            {
                /// <summary>Wants a third path for values.</summary>
                [Step]
                public static Outcome Go(ref double x) => Outcome.Passed();
            }
            """);

        Assert.Contains("ANVIL008", result.Ids);
    }

    // Legal on the wire — describes=true with zero steps — and therefore
    // silent, which is why it is worth saying once.
    [Fact]
    public void An_assembly_with_no_steps_is_ANVIL012()
    {
        var result = Harness.Run("public static class Nothing { }");

        Assert.Contains("ANVIL012", result.Ids);
    }

    [Fact]
    public void Ctx_is_handed_over_but_never_published_in_the_catalog()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public static class Rail
            {
                /// <summary>Reads the serial.</summary>
                [Step]
                public static Outcome ReadSerial(Ctx ctx) => Outcome.Passed();
            }
            """);

        Assert.Empty(result.Ids);
        Assert.Contains("__ctx", result.Source, StringComparison.Ordinal);
        Assert.DoesNotContain("\"ctx\"", result.Source, StringComparison.Ordinal);
    }

    // A boolean step is judging the unit, not measuring it: false is a fail and
    // not a measured zero.
    [Fact]
    public void A_boolean_return_becomes_a_verdict_and_not_a_measurement()
    {
        var result = Harness.Run("""
            using Anvil.Step;
            public static class Rail
            {
                /// <summary>Checks something.</summary>
                [Step]
                public static bool Check() => true;
            }
            """);

        Assert.Contains("Outcome.Passed()", result.Source, StringComparison.Ordinal);
        Assert.Contains("Outcome.Failed()", result.Source, StringComparison.Ordinal);
    }
}
