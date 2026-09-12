// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;

namespace Anvil.Step;

/// <summary>What the executor was told on the command line.</summary>
internal sealed class CommandLine
{
    private CommandLine(int port, string bind, Dictionary<string, string> options, bool list)
    {
        Port = port;
        Bind = bind;
        Options = options;
        List = list;
    }

    internal int Port { get; }

    internal string Bind { get; }

    internal IReadOnlyDictionary<string, string> Options { get; }

    internal bool List { get; }

    /// <summary>Reads the command line.</summary>
    /// <exception cref="FormatException">A flag that cannot be read.</exception>
    internal static CommandLine Parse(string[] args)
    {
        var port = StepHost.DefaultPort;

        // Loopback unless told otherwise: an executor that binds every interface
        // by default is one that reached the network without anyone deciding it.
        // `--bind 0.0.0.0` is the remote case, and it is explicit.
        var bind = "127.0.0.1";
        var options = new Dictionary<string, string>(StringComparer.Ordinal);
        var list = false;

        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--port":
                    port = ParsePort(Next(args, ref i, "--port"));
                    break;
                case "--bind":
                    bind = Next(args, ref i, "--bind");
                    break;
                case "--option":
                    var pair = Next(args, ref i, "--option");
                    var at = pair.IndexOf('=', StringComparison.Ordinal);
                    if (at <= 0)
                    {
                        throw new FormatException(string.Format(
                            CultureInfo.InvariantCulture,
                            "--option wants KEY=VALUE, and got '{0}'",
                            pair));
                    }

                    options[pair[..at]] = pair[(at + 1)..];
                    break;
                case "--list":
                    list = true;
                    break;
                default:
                    throw new FormatException(string.Format(
                        CultureInfo.InvariantCulture,
                        "unknown flag '{0}'. This executor takes --port, --bind, --option and --list",
                        args[i]));
            }
        }

        return new CommandLine(port, bind, options, list);
    }

    private static string Next(string[] args, ref int i, string flag) =>
        ++i < args.Length
            ? args[i]
            : throw new FormatException(string.Format(
                CultureInfo.InvariantCulture,
                "{0} wants a value after it",
                flag));

    // Invariant, like everything else that reads a number here: a port parsed
    // under a locale that groups digits is a port nobody meant.
    private static int ParsePort(string text) =>
        int.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out var port)
        && port is >= 0 and <= 65535
            ? port
            : throw new FormatException(string.Format(
                CultureInfo.InvariantCulture,
                "--port wants a number from 0 to 65535, and got '{0}'",
                text));
}
