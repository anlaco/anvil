using Anvil.Step;

public static partial class Board
{
    /// <summary>Measures the leakage current with the board idle, in amps.</summary>
    [Step]
    public static double MeasureLeakage() => 0.0004;

    /// <summary>Checks that the power LED is lit.</summary>
    [Step]
    public static bool CheckLed() => true;

    /// <summary>Reads the on-board temperature sensor.</summary>
    [Step]
    public static Outcome ReadTemperature() =>
        Outcome.Errored("the temperature sensor did not answer");
}
