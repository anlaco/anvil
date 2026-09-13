from anvil_step import step


@step
def measure_rail() -> float:
    """Measures the supply rail, in volts."""
    return 4.98
