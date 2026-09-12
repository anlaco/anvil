// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using Xunit;

namespace Anvil.Step.Tests;

/// <summary>
/// The two duties of ADR-0022 §7 — the ones no contract can verify for us.
/// </summary>
public class ObjectStoreTests
{
    private sealed class Bench
    {
        public double Channel { get; set; } = 1;
    }

    [Fact]
    public void An_object_goes_in_and_comes_back_out()
    {
        var store = new ObjectStore();
        var bench = new Bench();

        var reference = store.New(bench);

        Assert.Same(bench, store.Get(reference));
        Assert.Same(bench, store.Get<Bench>(reference));
        Assert.Equal(store.Lifetime, reference.Lifetime);
    }

    // Duty 1. A recycled key would let an old reference resolve cleanly to a
    // live, different instrument: same executor, same life, everything green,
    // measuring the wrong bench.
    [Fact]
    public void A_closed_key_is_never_handed_out_again()
    {
        var store = new ObjectStore();
        var first = store.New(new Bench());

        store.Close(first);
        var second = store.New(new Bench());

        Assert.NotEqual(first.Payload, second.Payload);
        Assert.Equal("s1", first.Payload);
        Assert.Equal("s2", second.Payload);
    }

    [Fact]
    public void A_closed_reference_says_it_was_closed_and_not_that_it_never_existed()
    {
        var store = new ObjectStore();
        var reference = store.New(new Bench());
        store.Close(reference);

        var ex = Assert.Throws<ObjectStoreException>(() => store.Get(reference));

        Assert.Contains("was closed", ex.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void Closing_hands_back_the_object_so_the_step_can_dispose_of_it()
    {
        var store = new ObjectStore();
        var bench = new Bench();
        var reference = store.New(bench);

        Assert.Same(bench, store.Close(reference));
        Assert.Equal(0, store.OpenCount);
    }

    // Duty 2. Two starts, two lives — this is what lets Anvil find out that the
    // process holding its references died and was born again.
    [Fact]
    public void Every_start_mints_a_different_life()
    {
        Assert.NotEqual(new ObjectStore().Lifetime, new ObjectStore().Lifetime);
    }

    [Fact]
    public void A_reference_from_another_life_is_refused_and_says_why()
    {
        var dead = new ObjectStore();
        var reference = dead.New(new Bench());
        var reborn = new ObjectStore();

        var ex = Assert.Throws<ObjectStoreException>(() => reborn.Get(reference));

        Assert.Contains("is gone", ex.Message, StringComparison.Ordinal);
    }

    // The reference names a slot, not an object: changing the state behind it
    // does not change its identity (ADR-0022 §5). A step that reconfigures the
    // bench answers the same reference it was given.
    [Fact]
    public void Mutating_the_object_does_not_change_the_reference()
    {
        var store = new ObjectStore();
        var reference = store.New(new Bench());

        store.Get<Bench>(reference).Channel = 2;

        Assert.Equal(2, store.Get<Bench>(reference).Channel);
        Assert.Equal("s1", reference.Payload);
    }

    [Fact]
    public void A_slot_holding_another_type_is_a_bench_error_and_names_both()
    {
        var store = new ObjectStore();
        var reference = store.New(new Bench());

        var ex = Assert.Throws<ObjectStoreException>(() => store.Get<string>(reference));

        Assert.Contains("Bench", ex.Message, StringComparison.Ordinal);
        Assert.Contains("String", ex.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void An_unknown_reference_says_it_names_no_slot()
    {
        var store = new ObjectStore();

        var ex = Assert.Throws<ObjectStoreException>(
            () => store.Get(new Reference("s99", store.Lifetime)));

        Assert.Contains("names no slot", ex.Message, StringComparison.Ordinal);
    }
}
