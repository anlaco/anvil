# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 ANLACO
"""A step that mints the worst payload the report format can be handed.

`;` and `=` are what the CSV cell uses to pack `name=value` pairs, and a
payload is minted by the executor and opaque to Anvil — so there is no
character it can be promised not to contain (ADR-0022, §Consequences).

It is built by hand rather than through `ctx.objects` because the point is the
payload's shape on the wire and in the report, not the slot behind it.
"""

from anvil_step import Context, Reference, Result, step


@step(name="mint_nasty_reference", outputs={"handle": Reference})
def mint_nasty_reference(ctx: Context) -> Result:
    """Hands back a reference whose payload carries ';' and '='."""
    ref = Reference(payload="rack;canal=2", lifetime=ctx.objects.lifetime)
    return Result.passed("minted", outputs={"handle": ref})
