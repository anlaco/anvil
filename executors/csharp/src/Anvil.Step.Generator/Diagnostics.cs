// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using Microsoft.CodeAnalysis;

namespace Anvil.Step.Generator;

/// <summary>
/// What the compiler says when a step cannot be served.
/// </summary>
/// <remarks>
/// This is the whole point of building the catalog at compile time: the author
/// of a step sees the mistake underlined in their editor, and not at three in
/// the morning on a test floor (ADR-0038 §5).
/// </remarks>
internal static class Diagnostics
{
    private const string Category = "Anvil.Step";

    internal static readonly DiagnosticDescriptor UnsupportedParameter = new(
        "ANVIL001",
        "Parameter type cannot cross the contract",
        "'{0}' is a {1}, and a step parameter must be double, string, bool, a reference to a class with [StepConstructor], or Ctx",
        Category,
        DiagnosticSeverity.Error,
        isEnabledByDefault: true,
        description: "A parameter that needs structure is a badly cut step (ADR-0020 §2).");

    internal static readonly DiagnosticDescriptor UnsupportedReturn = new(
        "ANVIL002",
        "Return type cannot become an outcome",
        "'{0}' returns {1}, and a step returns Outcome, double, bool or void",
        Category,
        DiagnosticSeverity.Error,
        isEnabledByDefault: true);

    internal static readonly DiagnosticDescriptor DuplicateName = new(
        "ANVIL003",
        "Two steps claim the same name",
        "'{0}' is served twice, and a catalog that answers to one name twice runs whichever registered last",
        Category,
        DiagnosticSeverity.Error,
        isEnabledByDefault: true);

    internal static readonly DiagnosticDescriptor BadMethod = new(
        "ANVIL004",
        "This method cannot be a step",
        "'{0}' cannot be a step because it is {1}",
        Category,
        DiagnosticSeverity.Error,
        isEnabledByDefault: true);

    internal static readonly DiagnosticDescriptor NoConstructor = new(
        "ANVIL005",
        "An instance step needs a way to be born",
        "'{0}' is an instance step but its class has no [StepConstructor], so nothing could ever open the object it runs on",
        Category,
        DiagnosticSeverity.Error,
        isEnabledByDefault: true);

    internal static readonly DiagnosticDescriptor TooManyConstructors = new(
        "ANVIL006",
        "A class opens one way",
        "'{0}' has more than one [StepConstructor]",
        Category,
        DiagnosticSeverity.Error,
        isEnabledByDefault: true);

    internal static readonly DiagnosticDescriptor BadName = new(
        "ANVIL007",
        "A name a sequence could not write",
        "'{0}' is not a name a sequence can use because it is empty or carries the module separator",
        Category,
        DiagnosticSeverity.Error,
        isEnabledByDefault: true);

    internal static readonly DiagnosticDescriptor BadParameterModifier = new(
        "ANVIL008",
        "A parameter modifier the contract has no room for",
        "'{0}' takes a ref, in or params parameter, and values go in through inputs and out through outputs with no third path",
        Category,
        DiagnosticSeverity.Error,
        isEnabledByDefault: true);

    internal static readonly DiagnosticDescriptor NoSteps = new(
        "ANVIL012",
        "This assembly declares no steps",
        "No method here carries [Step], so this executor would serve an empty catalog",
        Category,
        DiagnosticSeverity.Info,
        isEnabledByDefault: true);
}
