; Unshipped analyzer release
; https://github.com/dotnet/roslyn-analyzers/blob/main/src/Microsoft.CodeAnalysis.Analyzers/ReleaseTrackingAnalyzers.Help.md

### New Rules
Rule ID | Category | Severity | Notes
--------|----------|----------|-------
ANVIL001 | Anvil.Step | Error | A step parameter of a type the contract cannot carry.
ANVIL002 | Anvil.Step | Error | A step return type that cannot become an outcome.
ANVIL003 | Anvil.Step | Error | Two steps claiming the same qualified name.
ANVIL004 | Anvil.Step | Error | A method that cannot be a step.
ANVIL005 | Anvil.Step | Error | An instance step whose class has no [StepConstructor].
ANVIL006 | Anvil.Step | Error | A class with more than one [StepConstructor].
ANVIL007 | Anvil.Step | Error | A step or module name a sequence could not write.
ANVIL008 | Anvil.Step | Error | A ref, in or params parameter on a step.
ANVIL012 | Anvil.Step | Info | An assembly that declares no steps.
