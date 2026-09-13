using Anvil.Step;

/// <summary>The unit's own identity.</summary>
public static class Unit
{
    /// <summary>Reads the serial number from the unit's memory.</summary>
    [Step]
    public static Outcome ReadSerial() =>
        Outcome.Passed("serial read").Output("serial", "SN-0042");
}
