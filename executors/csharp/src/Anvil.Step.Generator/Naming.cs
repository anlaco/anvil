// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Text;
using System.Xml.Linq;
using Microsoft.CodeAnalysis;

namespace Anvil.Step.Generator;

/// <summary>How a C# name becomes a name a sequence writes, and where docs come from.</summary>
internal static class Naming
{
    /// <summary>Turns <c>SetVoltage</c> into <c>set_voltage</c>.</summary>
    internal static string SnakeCase(string name)
    {
        var sb = new StringBuilder(name.Length + 4);
        for (var i = 0; i < name.Length; i++)
        {
            var c = name[i];
            if (char.IsUpper(c))
            {
                // Keep runs of capitals together: `ReadDCVoltage` is
                // `read_dc_voltage`, not `read_d_c_voltage`.
                var startsRun = i > 0
                    && (!char.IsUpper(name[i - 1])
                        || (i + 1 < name.Length && char.IsLower(name[i + 1])));
                if (startsRun)
                {
                    sb.Append('_');
                }

                sb.Append(char.ToLowerInvariant(c));
            }
            else
            {
                sb.Append(c);
            }
        }

        return sb.ToString();
    }

    /// <summary>True when a sequence could not write this name.</summary>
    internal static bool IsUnusable(string name) =>
        string.IsNullOrWhiteSpace(name) || name.Contains("/");

    /// <summary>
    /// The first line of the <c>///</c> summary, which becomes the step's doc.
    /// </summary>
    /// <remarks>
    /// Tries the compiler's XML first, then the syntax, because
    /// <c>GetDocumentationCommentXml</c> comes back empty when the compilation
    /// runs with <c>DocumentationMode.None</c> — and requiring a user to set an
    /// MSBuild property before their catalog has any documentation would be
    /// requiring them to write it twice, which is what ADR-0024 refuses.
    /// </remarks>
    internal static string Summary(ISymbol symbol)
    {
        var doc = Parse(symbol);
        var summary = doc?.Element("summary")?.Value;
        return FirstLine(summary);
    }

    /// <summary>The <c>&lt;param&gt;</c> line for one parameter.</summary>
    internal static string Param(ISymbol owner, string name)
    {
        var doc = Parse(owner);
        var param = doc?.Elements("param")
            .FirstOrDefault(e => (string?)e.Attribute("name") == name)?.Value;
        return FirstLine(param);
    }

    private static XElement? Parse(ISymbol symbol)
    {
        var xml = symbol.GetDocumentationCommentXml(expandIncludes: true);
        if (string.IsNullOrWhiteSpace(xml))
        {
            xml = FromSyntax(symbol);
        }

        if (string.IsNullOrWhiteSpace(xml))
        {
            return null;
        }

        try
        {
            return XElement.Parse(xml);
        }
        catch (System.Xml.XmlException)
        {
            return null;
        }
    }

    private static string? FromSyntax(ISymbol symbol)
    {
        foreach (var reference in symbol.DeclaringSyntaxReferences)
        {
            var leading = reference.GetSyntax().GetLeadingTrivia();

            // With documentation parsing on, the comment is structured.
            var structured = leading
                .Select(t => t.GetStructure())
                .OfType<Microsoft.CodeAnalysis.CSharp.Syntax.DocumentationCommentTriviaSyntax>()
                .FirstOrDefault();
            if (structured is not null)
            {
                return Wrap(structured.ToFullString());
            }

            // With it off — which is the default in a user's own project, since
            // nobody sets GenerateDocumentationFile to get a catalog — the same
            // `///` lines arrive as plain single-line comments and
            // GetStructure() gives nothing. Read them as text instead: a step's
            // description must not depend on an MSBuild property the author
            // never heard of.
            var raw = leading
                .Where(t => t.IsKind(Microsoft.CodeAnalysis.CSharp.SyntaxKind.SingleLineDocumentationCommentTrivia)
                    || t.IsKind(Microsoft.CodeAnalysis.CSharp.SyntaxKind.SingleLineCommentTrivia))
                .Select(t => t.ToFullString())
                .Where(l => l.TrimStart().StartsWith("///", StringComparison.Ordinal))
                .ToList();

            if (raw.Count > 0)
            {
                return Wrap(string.Join("\n", raw));
            }
        }

        return null;
    }

    private static string Wrap(string commentText) =>
        "<member>"
        + string.Join(
            "\n",
            commentText
                .Split('\n')
                .Select(l => l.TrimStart().TrimStart('/').Trim()))
        + "</member>";

    private static string FirstLine(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return string.Empty;
        }

        foreach (var line in text!.Split('\n'))
        {
            var trimmed = line.Trim();
            if (trimmed.Length > 0)
            {
                return trimmed;
            }
        }

        return string.Empty;
    }
}
