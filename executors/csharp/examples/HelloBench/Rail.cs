// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

using Anvil.Step;

namespace HelloBench;

/// <summary>Checks on the board's power rail, with no bench state to keep.</summary>
public static class Rail
{
    /// <summary>Measures the rail voltage and lets the engine judge it.</summary>
    /// <param name="nominal">What the rail should read, in volts.</param>
    /// <param name="droop">How far below nominal this board actually sits.</param>
    [Step]
    public static double MeasureVoltage(double nominal, double droop = 0.01) =>
        nominal * (1 - droop);

    /// <summary>Checks that the board's pilot light is lit.</summary>
    /// <param name="expectLit">Whether it should be lit.</param>
    [Step]
    public static Outcome CheckLed(bool expectLit = true) =>
        expectLit
            ? Outcome.Passed("pilot light lit")
            : Outcome.Failed("pilot light out");

    /// <summary>Reads the serial number off the unit.</summary>
    [Step]
    public static Outcome ReadSerial(Ctx ctx) =>
        Outcome.Passed("serial read").Output("serial", "SN-00" + ctx.Attempt);
}
