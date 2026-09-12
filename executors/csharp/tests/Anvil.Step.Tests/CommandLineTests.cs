// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;
using Xunit;

namespace Anvil.Step.Tests;

/// <summary>What the executor was told, and what it refuses to be told.</summary>
public class CommandLineTests
{
    [Fact]
    public void Told_nothing_it_listens_on_loopback_at_9201()
    {
        var cli = CommandLine.Parse([]);

        Assert.Equal(9201, cli.Port);

        // Not every interface. An executor that reached the network without
        // anyone deciding it is a bench exposed by default.
        Assert.Equal("127.0.0.1", cli.Bind);
        Assert.False(cli.List);
        Assert.Empty(cli.Options);
    }

    [Fact]
    public void Options_are_key_equals_value_and_repeatable()
    {
        var cli = CommandLine.Parse(["--option", "simulator=10.0.0.5:4000", "--option", "rack=A"]);

        Assert.Equal("10.0.0.5:4000", cli.Options["simulator"]);
        Assert.Equal("A", cli.Options["rack"]);
    }

    [Fact]
    public void A_value_containing_an_equals_sign_keeps_it()
    {
        var cli = CommandLine.Parse(["--option", "url=http://x/?a=b"]);

        Assert.Equal("http://x/?a=b", cli.Options["url"]);
    }

    [Theory]
    [InlineData("--option", "nokey")]
    [InlineData("--port", "not-a-number")]
    [InlineData("--port", "70000")]
    public void A_flag_it_cannot_read_stops_it_rather_than_guessing(string flag, string value)
    {
        Assert.Throws<FormatException>(() => CommandLine.Parse([flag, value]));
    }

    [Fact]
    public void An_unknown_flag_says_what_it_does_take()
    {
        var ex = Assert.Throws<FormatException>(() => CommandLine.Parse(["--steps", "./steps"]));

        Assert.Contains("--option", ex.Message, StringComparison.Ordinal);
    }

    // Invariant: a port read under a locale that groups digits is a port
    // nobody meant.
    [Fact]
    public void The_port_is_read_the_same_under_any_locale()
    {
        var saved = CultureInfo.CurrentCulture;
        try
        {
            CultureInfo.CurrentCulture = new CultureInfo("es-ES");
            Assert.Equal(9300, CommandLine.Parse(["--port", "9300"]).Port);
            Assert.Throws<FormatException>(() => CommandLine.Parse(["--port", "9.300"]));
        }
        finally
        {
            CultureInfo.CurrentCulture = saved;
        }
    }
}
