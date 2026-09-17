using Anvil.Step;

/// <summary>The test fixture the board sits in.</summary>
public static class Fixture
{
    /// <summary>Clamps the board and powers it.</summary>
    [Step]
    public static void PowerOn() { }

    /// <summary>Cuts power and releases the board.</summary>
    [Step]
    public static void PowerOff() { }

    /// <summary>Opens the link to the board; the first attempt always times out.</summary>
    [Step]
    public static Outcome Connect(Ctx ctx) =>
        ctx.Attempt == 1
            ? Outcome.Errored("no answer from the board")
            : Outcome.Passed("connected on attempt " + ctx.Attempt);

    /// <summary>Measures the rail while it settles: low on the first attempt.</summary>
    [Step]
    public static double MeasureSettlingRail(Ctx ctx) => ctx.Attempt == 1 ? 4.1 : 4.97;
}
