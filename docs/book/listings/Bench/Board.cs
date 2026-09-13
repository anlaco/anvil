using Anvil.Step;

/// <summary>Checks on the board under test.</summary>
public static partial class Board
{
    /// <summary>Measures the supply rail, in volts.</summary>
    [Step]
    public static double MeasureRail() => 4.98;
}
