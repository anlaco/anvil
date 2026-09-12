// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using System.Globalization;
using Anvil.Step;

namespace HelloBench;

/// <summary>A bench power supply, held open across several steps (ADR-0022).</summary>
/// <remarks>
/// In production this holds a socket or a vendor driver handle, which is why it
/// cannot travel and stays in this process. Here it is simulated, so the example
/// runs with nothing on the bench.
/// </remarks>
[StepModule("psu")]
public sealed class PowerSupply
{
    private double _volts;

    /// <summary>Opens the session with the supply and leaves it ready.</summary>
    /// <param name="resource">The address of the supply, VISA-style.</param>
    [StepConstructor]
    public PowerSupply(string resource) => Resource = resource;

    /// <summary>Where this supply is.</summary>
    public string Resource { get; }

    /// <summary>Sets the output voltage, in volts.</summary>
    /// <param name="volts">What to set the output to.</param>
    [Step]
    public Outcome SetVoltage(double volts)
    {
        _volts = volts;
        return Outcome.Passed(string.Format(
            CultureInfo.InvariantCulture, "output set to {0} V on {1}", volts, Resource));
    }

    /// <summary>Measures the current the supply delivers, in amps.</summary>
    [Step]
    public Outcome MeasureCurrent() =>
        Outcome.Measured(_volts * 0.0625, "measured through the open session")
            .Output("volts_applied", _volts);

    /// <summary>Cuts the output. Meant for the cleanup phase.</summary>
    /// <param name="ctx">The executor, for the handle this step spends.</param>
    [Step]
    public Outcome OutputOff(Ctx ctx)
    {
        _volts = 0;
        return Outcome.Passed("output cut");
    }
}
