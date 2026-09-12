// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Server.Kestrel.Core;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

namespace Anvil.Step;

/// <summary>
/// Brings up the executor. This is the whole of your <c>Program.cs</c>.
/// </summary>
/// <remarks>
/// <para>
/// The process is <b>yours</b>, not ours: you reference this package, mark your
/// methods, and your executable listens. We ship no binary that loads your
/// compiled assemblies, because a dependency tree resolved on somebody else's
/// machine fails mid-sequence and not at start-up (ADR-0038 §2).
/// </para>
/// <para>
/// There is no <c>--steps</c> here, unlike the Python executor: your catalog is
/// compiled into this executable, so there is no folder to scan (ADR-0038 §7).
/// </para>
/// </remarks>
public static class StepHost
{
    /// <summary>The port this executor listens on when none is given.</summary>
    /// <remarks>
    /// 9201, so it does not collide with the Python executor's 9101 when both
    /// serve the same bench.
    /// </remarks>
    public const int DefaultPort = 9201;

    /// <summary>Runs the executor until it is stopped.</summary>
    /// <param name="args">The command line: <c>--port</c>, <c>--bind</c>, <c>--option</c>, <c>--list</c>.</param>
    /// <param name="register">Fills the registry with your steps.</param>
    /// <returns>The process exit code.</returns>
    public static async Task<int> RunAsync(string[] args, Action<Registry> register)
    {
        ArgumentNullException.ThrowIfNull(register);

        CommandLine options;
        try
        {
            options = CommandLine.Parse(args);
        }
        catch (FormatException e)
        {
            await Console.Error.WriteLineAsync(e.Message).ConfigureAwait(false);
            return 2;
        }

        var registry = new Registry();
        try
        {
            register(registry);
        }
        catch (DuplicateStepException e)
        {
            // An ambiguous catalog is worse than no executor: refuse to start
            // and name the clash.
            await Console.Error.WriteLineAsync(e.Message).ConfigureAwait(false);
            return 2;
        }

        var objects = new ObjectStore();

        if (options.List)
        {
            await ListAsync(registry, objects).ConfigureAwait(false);
            return 0;
        }

        var builder = WebApplication.CreateBuilder();
        builder.Logging.ClearProviders();
        builder.Services.AddGrpc();
        builder.Services.AddSingleton(registry);
        builder.Services.AddSingleton(objects);
        builder.Services.AddSingleton(options.Options);
        builder.Services.AddSingleton<StepExecutorService>();
        builder.WebHost.ConfigureKestrel(kestrel =>
            kestrel.Listen(
                System.Net.IPAddress.Parse(options.Bind),
                options.Port,

                // HTTP/2 without TLS, declared explicitly. gRPC cannot negotiate
                // it over plain h2c otherwise, and the failure is a connection
                // that opens and then says nothing.
                listen => listen.Protocols = HttpProtocols.Http2));

        var app = builder.Build();
        app.MapGrpcService<StepExecutorService>();

        await Console.Error.WriteLineAsync(string.Format(
            CultureInfo.InvariantCulture,
            "anvil C# executor: {0} step(s) on {1}:{2}, life {3}",
            registry.Steps.Count,
            options.Bind,
            options.Port,
            objects.Lifetime)).ConfigureAwait(false);

        await app.RunAsync().ConfigureAwait(false);
        return 0;
    }

    private static async Task ListAsync(Registry registry, ObjectStore objects)
    {
        // The enumerate door an editor needs, and the way to answer "which steps
        // does this executor serve?" without starting a bench (ADR-0028).
        await Console.Out.WriteLineAsync(string.Format(
            CultureInfo.InvariantCulture,
            "life {0}, contract {1}",
            objects.Lifetime,
            Mapping.Contract)).ConfigureAwait(false);

        foreach (var step in registry.Steps.OrderBy(s => s.Qualified, StringComparer.Ordinal))
        {
            var inputs = string.Join(", ", step.Inputs.Select(Describe));
            await Console.Out.WriteLineAsync(string.Format(
                CultureInfo.InvariantCulture,
                "  {0}({1}){2}",
                step.Qualified,
                inputs,
                string.IsNullOrEmpty(step.Doc) ? string.Empty : "  — " + step.Doc)).ConfigureAwait(false);
        }
    }

    private static string Describe(ParameterSpec p)
    {
        var kind = p.Kind.ToString().ToLowerInvariant();
        return p.Required
            ? string.Format(CultureInfo.InvariantCulture, "{0}: {1}", p.Name, kind)
            : string.Format(CultureInfo.InvariantCulture, "{0}?: {1}", p.Name, kind);
    }
}
