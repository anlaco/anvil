using Anvil.Step;

/// <summary>A bench power supply, kept open across several steps.</summary>
[StepModule("psu")]
public sealed class PowerSupply
{
    private double _volts;

    /// <summary>Opens the session with the supply.</summary>
    /// <param name="resource">Where the supply is.</param>
    [StepConstructor]
    public PowerSupply(string resource) => Resource = resource;

    /// <summary>Where this supply is.</summary>
    public string Resource { get; }

    /// <summary>Sets the output voltage, in volts.</summary>
    /// <param name="volts">What to set the output to.</param>
    [Step]
    public void SetVoltage(double volts) => _volts = volts;

    /// <summary>Measures the current the load draws, in amps.</summary>
    [Step]
    public double MeasureCurrent() => _volts * 0.0625;

    /// <summary>Cuts the output.</summary>
    [Step]
    public void OutputOff() => _volts = 0;
}
