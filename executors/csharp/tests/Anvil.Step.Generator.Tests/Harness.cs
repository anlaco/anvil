// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Collections.Immutable;
using System.Reflection;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;

namespace Anvil.Step.Generator.Tests;

/// <summary>Runs the real generator over source, and says what came out.</summary>
internal static class Harness
{
    internal sealed record Result(string Source, ImmutableArray<Diagnostic> Diagnostics)
    {
        /// <summary>The ids the generator reported, in order.</summary>
        internal IEnumerable<string> Ids => Diagnostics.Select(d => d.Id);
    }

    internal static Result Run(string source) => Run(source, DocumentationMode.Parse);

    /// <summary>
    /// Runs the generator with the compilation parsing documentation the way a
    /// given project would.
    /// </summary>
    /// <remarks>
    /// A user's own project does not set GenerateDocumentationFile, so its
    /// compilation parses `///` as plain comments and the structured trivia is
    /// not there. The generator has to find the description anyway.
    /// </remarks>
    internal static Result Run(string source, DocumentationMode mode)
    {
        var compilation = CSharpCompilation.Create(
            "StepsUnderTest",
            [CSharpSyntaxTree.ParseText(source, new CSharpParseOptions(documentationMode: mode))],
            References,
            new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary));

        var driver = CSharpGeneratorDriver
            .Create(new StepGenerator())
            .RunGeneratorsAndUpdateCompilation(compilation, out _, out var diagnostics);

        var generated = driver.GetRunResult().Results
            .SelectMany(r => r.GeneratedSources)
            .Select(s => s.SourceText.ToString())
            .FirstOrDefault() ?? string.Empty;

        return new Result(generated, diagnostics);
    }

    private static IEnumerable<MetadataReference> References =>
        AppDomain.CurrentDomain.GetAssemblies()
            .Where(a => !a.IsDynamic && !string.IsNullOrEmpty(a.Location))
            .Select(a => MetadataReference.CreateFromFile(a.Location))
            .Append(MetadataReference.CreateFromFile(typeof(Anvil.Step.StepAttribute).Assembly.Location))
            .Append(MetadataReference.CreateFromFile(typeof(object).Assembly.Location))
            .Distinct();
}
