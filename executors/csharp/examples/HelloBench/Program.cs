// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 ANLACO

// The whole entry point. `AnvilSteps.Register` is written by the generator from
// the signatures in this project — there is no list of steps to keep up to date.
return await Anvil.Step.StepHost
    .RunAsync(args, Anvil.Step.Generated.AnvilSteps.Register)
    .ConfigureAwait(false);
