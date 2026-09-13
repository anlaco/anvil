using Anvil.Step;

/// <summary>A multimeter with four input channels.</summary>
public static class Dmm
{
    /// <summary>Measures the voltage on a channel.</summary>
    /// <param name="channel">The input channel, 1 to 4.</param>
    /// <param name="range">The measurement range.</param>
    [Step]
    public static Outcome MeasureVoltage(double channel, string range = "auto") =>
        channel switch
        {
            1 => Outcome.Measured(1.1, "range " + range),
            2 => Outcome.Measured(1.8, "range " + range),
            3 => Outcome.Measured(3.3, "range " + range),
            4 => Outcome.Measured(5.0, "range " + range),
            _ => Outcome.Errored("the multimeter has no such channel"),
        };
}
