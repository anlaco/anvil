# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 ANLACO
"""A second executor that only knows how to wait — QA scaffolding, not product.

It exists so a run can be held **between two steps of another executor** while
the harness kills and restarts that one. The wait cannot live in the executor
being restarted: killing it mid-`Invoke` would break the call itself, and what
has to be reproduced is the restart being noticed on the *next* call.
"""

import time

from anvil_step import Context, Result, step


@step(name="wait")
def wait(ctx: Context, seconds: float) -> Result:
    """Waits, so the harness has a window to restart the other executor."""
    time.sleep(seconds)
    return Result.passed(f"waited {seconds}s")
