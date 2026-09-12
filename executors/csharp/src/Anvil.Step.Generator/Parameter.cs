// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;
using Microsoft.CodeAnalysis;

namespace Anvil.Step.Generator;

/// <summary>One input of a step, read off the method's own parameter.</summary>
internal sealed class Parameter
{
    private string _name = string.Empty;
    private string _doc = string.Empty;
    private bool _required;
    private string? _default;

    /// <summary>The contract type, or <see langword="null"/> when it has none.</summary>
    internal string? Kind { get; private set; }

    /// <summary>True for the <c>Ctx</c> parameter, which is never published.</summary>
    internal bool IsContext { get; private set; }

    internal static Parameter Of(IParameterSymbol symbol, ISymbol owner)
    {
        var p = new Parameter
        {
            // The sequence writes this name in `inputs:`, so it follows the
            // idiom of the YAML and not of C#: `expectLit` is `expect_lit`.
            _name = Naming.SnakeCase(symbol.Name),
            _doc = Naming.Param(owner, symbol.Name),

            // An optional parameter is one the sequence may leave out: it has a
            // default, or it is nullable.
            _required = !symbol.HasExplicitDefaultValue
                && symbol.NullableAnnotation != NullableAnnotation.Annotated,
        };

        var type = symbol.Type.ToDisplayString().TrimEnd('?');
        p.Kind = type switch
        {
            "double" or "float" => "Number",
            "string" => "Text",
            "bool" => "Boolean",
            "Anvil.Step.Reference" => "Reference",
            "Anvil.Step.Ctx" => null,
            _ => null,
        };

        if (type == "Anvil.Step.Ctx")
        {
            // The executor talking to the step. A step gets one only if it asks,
            // and it never appears in the catalog.
            p.IsContext = true;
            p.Kind = "Context";
        }

        if (symbol.HasExplicitDefaultValue && symbol.ExplicitDefaultValue is not null)
        {
            p._default = Literal(symbol.ExplicitDefaultValue, p.Kind);
        }

        return p;
    }

    /// <summary>How the generated code reads this parameter for the call.</summary>
    internal static string Read(IParameterSymbol symbol)
    {
        var type = symbol.Type.ToDisplayString().TrimEnd('?');
        if (type == "Anvil.Step.Ctx")
        {
            return "__ctx";
        }

        var optional = symbol.HasExplicitDefaultValue
            || symbol.NullableAnnotation == NullableAnnotation.Annotated;
        var name = StepModel.Quote(Naming.SnakeCase(symbol.Name));

        var read = type switch
        {
            "double" => optional ? "__inputs.OptionalNumber(" + name + ")" : "__inputs.Number(" + name + ")",
            "float" => optional
                ? "(float?)__inputs.OptionalNumber(" + name + ")"
                : "(float)__inputs.Number(" + name + ")",
            "string" => optional ? "__inputs.OptionalText(" + name + ")" : "__inputs.Text(" + name + ")",
            "bool" => optional ? "__inputs.OptionalBoolean(" + name + ")" : "__inputs.Boolean(" + name + ")",
            "Anvil.Step.Reference" => "__inputs.Reference(" + name + ")",
            _ => "default",
        };

        // A default declared in C# is applied by the step, not sent by the
        // engine (ADR-0021 §5) — so it is applied here, where it was written.
        if (symbol.HasExplicitDefaultValue && symbol.ExplicitDefaultValue is not null)
        {
            read += " ?? " + Literal(symbol.ExplicitDefaultValue, null);
        }

        return read;
    }

    internal string RenderSpec()
    {
        var kind = "global::Anvil.Step.ValueKind." + (Kind ?? "Unspecified");
        var fallback = _default is null
            ? "null"
            : "global::Anvil.Step.StepValue.Of(" + _default + ")";
        return "new(" + StepModel.Quote(_name) + ", " + kind + ", "
            + (_required ? "true" : "false") + ", " + fallback + ", "
            + StepModel.Quote(_doc) + ")";
    }

    private static string Literal(object value, string? _) => value switch
    {
        // Invariant, like every other number this package writes: a default
        // rendered under a comma locale would not even compile.
        double d => d.ToString("R", CultureInfo.InvariantCulture),
        float f => f.ToString("R", CultureInfo.InvariantCulture),
        int i => i.ToString(CultureInfo.InvariantCulture) + "d",
        bool b => b ? "true" : "false",
        string s => StepModel.Quote(s),
        _ => "default",
    };
}
