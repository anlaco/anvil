// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Text;
using Microsoft.CodeAnalysis;

namespace Anvil.Step.Generator;

/// <summary>One step, as read off a signature and about to be written out.</summary>
internal sealed class StepModel
{
    private readonly List<Parameter> _parameters = [];
    private IMethodSymbol? _symbol;
    private bool _isOpener;
    private string _module = string.Empty;
    private string _name = string.Empty;
    private string _doc = string.Empty;
    private string _call = string.Empty;
    private string? _handle;
    private string? _ownerType;

    internal string Qualified => _module + "/" + _name;

    internal Location? Location { get; private set; }

    /// <summary>Reads the constructor that opens the object: <c>module/open</c>.</summary>
    internal static StepModel FromConstructor(
        string module, IMethodSymbol ctor, INamedTypeSymbol owner)
    {
        var declared = ctor.GetAttributes()
            .First(a => a.AttributeClass?.ToDisplayString() == "Anvil.Step.StepConstructorAttribute")
            .ConstructorArguments.FirstOrDefault().Value as string;

        var model = new StepModel
        {
            _module = module,
            _name = declared ?? "open",
            _doc = Naming.Summary(ctor),
            Location = ctor.Locations.FirstOrDefault() ?? owner.Locations.FirstOrDefault(),
            _ownerType = owner.ToDisplayString(),
            _symbol = ctor,
            _isOpener = true,
        };

        foreach (var p in ctor.Parameters)
        {
            model._parameters.Add(Parameter.Of(p, ctor));
        }

        return model;
    }

    /// <summary>Reads a marked method.</summary>
    internal static StepModel? FromMethod(
        string module,
        IMethodSymbol method,
        INamedTypeSymbol owner,
        bool hasOpener,
        SourceProductionContext context)
    {
        if (method.IsGenericMethod || method.DeclaredAccessibility != Accessibility.Public)
        {
            context.ReportDiagnostic(Diagnostic.Create(
                Diagnostics.BadMethod,
                method.Locations.FirstOrDefault(),
                method.Name,
                method.IsGenericMethod ? "generic" : "not public"));
            return null;
        }

        // An instance step runs on an object, and something has to open it.
        if (!method.IsStatic && !hasOpener)
        {
            context.ReportDiagnostic(Diagnostic.Create(
                Diagnostics.NoConstructor, method.Locations.FirstOrDefault(), method.Name));
            return null;
        }

        var declared = method.GetAttributes()
            .First(a => a.AttributeClass?.ToDisplayString() == "Anvil.Step.StepAttribute")
            .ConstructorArguments.FirstOrDefault().Value as string;

        var name = declared ?? Naming.SnakeCase(method.Name);
        if (Naming.IsUnusable(name))
        {
            context.ReportDiagnostic(Diagnostic.Create(
                Diagnostics.BadName, method.Locations.FirstOrDefault(), name));
            return null;
        }

        var model = new StepModel
        {
            _module = module,
            _name = name,
            _doc = Naming.Summary(method),
            Location = method.Locations.FirstOrDefault(),
            _ownerType = owner.ToDisplayString(),
            _symbol = method,

            // The implicit first input of an instance step: the handle to the
            // object it runs on, named after the module.
            _handle = method.IsStatic ? null : module,
        };

        foreach (var p in method.Parameters)
        {
            if (p.RefKind != RefKind.None || p.IsParams)
            {
                context.ReportDiagnostic(Diagnostic.Create(
                    Diagnostics.BadParameterModifier, p.Locations.FirstOrDefault(), method.Name));
                return null;
            }

            var parameter = Parameter.Of(p, method);
            if (parameter.Kind is null)
            {
                context.ReportDiagnostic(Diagnostic.Create(
                    Diagnostics.UnsupportedParameter,
                    p.Locations.FirstOrDefault(),
                    p.Name,
                    p.Type.ToDisplayString()));
                return null;
            }

            model._parameters.Add(parameter);
        }

        model._call = Call(method, owner);
        if (model._call.Length == 0)
        {
            context.ReportDiagnostic(Diagnostic.Create(
                Diagnostics.UnsupportedReturn,
                method.Locations.FirstOrDefault(),
                method.Name,
                method.ReturnType.ToDisplayString()));
            return null;
        }

        return model;
    }

    private static string Call(IMethodSymbol method, INamedTypeSymbol owner)
    {
        var target = method.IsStatic
            ? "global::" + owner.ToDisplayString() + "." + method.Name
            : "__target." + method.Name;
        var args = string.Join(", ", method.Parameters.Select(Parameter.Read));
        var invocation = target + "(" + args + ")";

        return method.ReturnType.ToDisplayString() switch
        {
            "Anvil.Step.Outcome" => "return " + invocation + ";",
            "double" or "float" =>
                "return global::Anvil.Step.Outcome.Measured(" + invocation + ");",

            // `false` is a fail and not a measured zero: a step answering a
            // boolean is judging the unit, not measuring it.
            "bool" => "return " + invocation
                + " ? global::Anvil.Step.Outcome.Passed() : global::Anvil.Step.Outcome.Failed();",
            "void" => invocation + "; return global::Anvil.Step.Outcome.Passed();",
            _ => string.Empty,
        };
    }

    internal void Render(StringBuilder sb)
    {
        sb.AppendLine("        registry.Add(new global::Anvil.Step.StepDescriptor(");
        sb.AppendLine("            " + Quote(_module) + ",");
        sb.AppendLine("            " + Quote(_name) + ",");

        sb.AppendLine("            new global::Anvil.Step.ParameterSpec[]");
        sb.AppendLine("            {");
        if (_handle is not null)
        {
            sb.AppendLine("                new(" + Quote(_handle)
                + ", global::Anvil.Step.ValueKind.Reference, true, null, "
                + Quote("The object this step runs on.") + "),");
        }

        foreach (var p in _parameters.Where(p => !p.IsContext))
        {
            sb.AppendLine("                " + p.RenderSpec() + ",");
        }

        sb.AppendLine("            },");

        // A constructor answers one output: the handle to what it opened.
        sb.AppendLine("            new global::Anvil.Step.OutputSpec[]");
        sb.AppendLine("            {");
        if (_isOpener)
        {
            sb.AppendLine("                new(" + Quote(_module)
                + ", global::Anvil.Step.ValueKind.Reference, "
                + Quote("The object that was opened.") + "),");
        }

        sb.AppendLine("            },");
        sb.AppendLine("            " + Quote(_doc) + ",");
        sb.AppendLine("            (__ctx, __inputs) =>");
        sb.AppendLine("            {");

        if (_isOpener)
        {
            // The opener: build the object, keep it here, hand back the handle.
            var args = string.Join(", ", _symbol!.Parameters.Select(Parameter.Read));
            sb.AppendLine("                var __opened = new global::" + _ownerType + "(" + args + ");");
            sb.AppendLine("                return global::Anvil.Step.Outcome.Passed()");
            sb.AppendLine("                    .Output(" + Quote(_module) + ", __ctx.Objects.New(__opened));");
        }
        else
        {
            if (_handle is not null)
            {
                sb.AppendLine("                var __target = __ctx.Objects.Get<global::" + _ownerType
                    + ">(__inputs.Reference(" + Quote(_handle) + "));");
            }

            sb.AppendLine("                " + _call);
        }

        sb.AppendLine("            }));");
    }

    internal static string Quote(string s) =>
        "\"" + s.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\"";
}
