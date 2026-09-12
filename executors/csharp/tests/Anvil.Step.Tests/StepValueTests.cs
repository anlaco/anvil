// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using Xunit;

namespace Anvil.Step.Tests;

/// <summary>The four types of the contract, and nothing else.</summary>
public class StepValueTests
{
    [Fact]
    public void Each_type_knows_what_it_is()
    {
        Assert.Equal(ValueKind.Number, StepValue.Of(1.0).Kind);
        Assert.Equal(ValueKind.Text, StepValue.Of("hello").Kind);
        Assert.Equal(ValueKind.Boolean, StepValue.Of(true).Kind);
        Assert.Equal(ValueKind.Reference, StepValue.Of(new Reference("s1", "life")).Kind);
    }

    // Reading a value as the wrong type is a bug in the executor, not a zero.
    // The contract says the same of an unset `oneof` (ADR-0020 §2).
    [Fact]
    public void Reading_a_value_as_the_wrong_type_throws_and_says_both()
    {
        var ex = Assert.Throws<InvalidOperationException>(() => StepValue.Of("hello").AsNumber);

        Assert.Contains("text", ex.Message, StringComparison.Ordinal);
        Assert.Contains("number", ex.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void A_reference_keeps_its_payload_and_life_and_leaves_the_executor_to_anvil()
    {
        var r = new Reference("s7", "abc123");

        Assert.Equal("s7", r.Payload);
        Assert.Equal("abc123", r.Lifetime);
        Assert.Equal(string.Empty, r.Executor);
        Assert.False(r.IsEmpty);
    }

    [Fact]
    public void A_default_reference_is_empty()
    {
        Assert.True(default(Reference).IsEmpty);
    }
}
