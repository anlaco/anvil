// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;

namespace Anvil.Step;

/// <summary>
/// Translates between what a step author sees and what travels on the wire.
/// </summary>
/// <remarks>
/// This is the only place that builds a <c>StepResult</c> or a <c>Catalog</c>,
/// which is what makes it impossible to answer without the contract echo.
/// </remarks>
internal static class Mapping
{
    /// <summary>
    /// The contract version this executor speaks, mirroring
    /// <c>crates/modelo/src/proto.rs:33</c>.
    /// </summary>
    /// <remarks>
    /// It is echoed in every answer. The echo is what prevents the silent false
    /// pass: an old executor ignores <c>inputs</c>, measures something else and
    /// reports "pass" (ADR-0020 §4b). Raise it only when an old peer's silence
    /// could alter a verdict.
    /// </remarks>
    internal const int Contract = 4;

    internal static StepValue FromWire(global::Value value) => value.ValueCase switch
    {
        global::Value.ValueOneofCase.Number => StepValue.Of(value.Number),
        global::Value.ValueOneofCase.Text => StepValue.Of(value.Text),
        global::Value.ValueOneofCase.Boolean => StepValue.Of(value.Boolean),
        global::Value.ValueOneofCase.Reference => StepValue.Of(new Reference(
            value.Reference.Payload,
            value.Reference.Lifetime,
            value.Reference.Executor)),

        // No branch set. Deliberately not a zero: reading it as one would be
        // measuring against something nobody declared (ADR-0020 §2).
        _ => default,
    };

    internal static global::Value ToWire(string name, StepValue value)
    {
        var wire = new global::Value { Name = name };
        switch (value.Kind)
        {
            case ValueKind.Number:
                wire.Number = value.AsNumber;
                break;
            case ValueKind.Text:
                wire.Text = value.AsText;
                break;
            case ValueKind.Boolean:
                wire.Boolean = value.AsBoolean;
                break;
            case ValueKind.Reference:
                var reference = value.AsReference;
                wire.Reference = new global::Reference
                {
                    Payload = reference.Payload,
                    Lifetime = reference.Lifetime,

                    // Left empty on purpose: Anvil stamps it. This process does
                    // not know what a sequence called it (paso.proto:21).
                    Executor = string.Empty,
                };
                break;
            default:
                throw new InvalidOperationException(string.Format(
                    CultureInfo.InvariantCulture,
                    "output '{0}' has no value set",
                    name));
        }

        return wire;
    }

    /// <summary>Builds the answer to one call. The only door, echo included.</summary>
    internal static StepResult ToWire(string name, Outcome outcome)
    {
        var result = new StepResult
        {
            Name = name,
            Status = outcome.Status,
            Message = outcome.Message,

            // Text, and written by the one place allowed to write it.
            MeasuredValue = Numbers.ToWire(outcome.MeasuredValue),

            // limit_min and limit_max stay empty: the threshold is the engine's
            // and the step does not know it (ADR-0008).
            Contract = Contract,
        };

        foreach (var (outputName, value) in outcome.Outputs)
        {
            result.Outputs.Add(ToWire(outputName, value));
        }

        return result;
    }

    private static global::ValueType ToWire(ValueKind kind) => kind switch
    {
        ValueKind.Number => global::ValueType.Number,
        ValueKind.Text => global::ValueType.Text,
        ValueKind.Boolean => global::ValueType.Boolean,
        ValueKind.Reference => global::ValueType.Reference,
        _ => global::ValueType.Unspecified,
    };

    internal static StepSpec ToWire(StepDescriptor step)
    {
        var spec = new StepSpec { Name = step.Qualified, Doc = step.Doc };
        foreach (var input in step.Inputs)
        {
            var parameter = new global::ParameterSpec
            {
                Name = input.Name,
                Type = ToWire(input.Kind),
                Required = input.Required,
                Doc = input.Doc,
            };
            if (input.Default is { } declared)
            {
                parameter.Default = ToWire(input.Name, declared);
            }

            spec.Inputs.Add(parameter);
        }

        foreach (var output in step.Outputs)
        {
            spec.Outputs.Add(new global::OutputSpec
            {
                Name = output.Name,
                Type = ToWire(output.Kind),
                Doc = output.Doc,
            });
        }

        return spec;
    }

    /// <summary>Builds the catalog. Always describing, echo and life included.</summary>
    internal static Catalog ToWire(Registry registry, string lifetime)
    {
        // `describes` is true even with an empty catalog: proto3 makes silence
        // false, and false means "do not check me". An executor that serves
        // nothing on purpose has to say so out loud (ADR-0021 §4).
        var catalog = new Catalog
        {
            Describes = true,
            Contract = Contract,
            Lifetime = lifetime,
        };

        foreach (var step in registry.Steps)
        {
            catalog.Steps.Add(ToWire(step));
        }

        return catalog;
    }
}
