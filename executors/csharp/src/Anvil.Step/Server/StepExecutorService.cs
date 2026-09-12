// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;
using Grpc.Core;

namespace Anvil.Step;

/// <summary>
/// Serves the two doors of the contract: <c>Invoke</c> and <c>Describe</c>.
/// </summary>
/// <remarks>
/// The sibling of <c>executors/python/server.py</c>. The engine cannot tell the
/// two apart, and a sequence mixes them freely.
/// </remarks>
internal sealed class StepExecutorService : StepExecutor.StepExecutorBase
{
    private readonly Registry _registry;
    private readonly ObjectStore _objects;
    private readonly IReadOnlyDictionary<string, string> _options;

    internal StepExecutorService(
        Registry registry,
        ObjectStore objects,
        IReadOnlyDictionary<string, string> options)
    {
        _registry = registry;
        _objects = objects;
        _options = options;
    }

    /// <inheritdoc/>
    public override Task<StepResult> Invoke(StepRequest request, ServerCallContext context)
    {
        var step = _registry.Find(request.Name);
        if (step is null)
        {
            return Task.FromResult(Mapping.ToWire(request.Name, Outcome.Errored(Unknown(request.Name))));
        }

        var inputs = new Dictionary<string, StepValue>(StringComparer.Ordinal);
        foreach (var value in request.Inputs)
        {
            inputs[value.Name] = Mapping.FromWire(value);
        }

        var ctx = new Ctx(request.Name, request.Attempt, _options, _objects);

        Outcome outcome;
        try
        {
            outcome = step.Invoke(ctx, new Inputs(inputs));
        }
        catch (Exception e) when (e is not OutOfMemoryException and not StackOverflowException)
        {
            // An exception says the step could not judge — information about the
            // bench or the step, never about the unit. `error`, never `fail`
            // (ADR-0019, Rule 2). And it does not take the server down: the next
            // call is answered.
            outcome = Outcome.Errored(string.Format(
                CultureInfo.InvariantCulture,
                "the step raised {0}: {1}",
                e.GetType().Name,
                e.Message));
        }

        return Task.FromResult(Mapping.ToWire(request.Name, outcome));
    }

    /// <inheritdoc/>
    public override Task<Catalog> Describe(CatalogRequest request, ServerCallContext context) =>
        Task.FromResult(Mapping.ToWire(_registry, _objects.Lifetime));

    private string Unknown(string name)
    {
        var near = _registry.Near(name).Take(3).ToList();
        var message = string.Format(
            CultureInfo.InvariantCulture,
            "this executor serves no step called '{0}'",
            name);
        return near.Count == 0
            ? message
            : message + string.Format(
                CultureInfo.InvariantCulture,
                "; did you mean {0}?",
                string.Join(" or ", near.Select(n => "'" + n + "'")));
    }
}
