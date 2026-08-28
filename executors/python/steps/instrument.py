# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 ANLACO
"""The steps this executor ships with — and the example of how to write yours.

Nothing here is privileged. This file lives under `steps/`, is discovered like
any other, and could be deleted without touching a line of `server.py`. Drop a
file of your own next to it and it is served the same way (ADR-0021).

The scenario is the one behind ADR-0012: a step that touches hardware need not
live inside Anvil's OS. Here the instrument steps talk **over TCP** with an
instrument simulator; in production that endpoint can be a box running the
vendor's driver on a Windows 7 machine (LID), a VM, or a real simulator. To
Anvil it is all "a gRPC endpoint on some IP".
"""

import socket

from anvil_step import Context, Reference, Result, step

#: Where the simulator listens if `--option simulator=host:port` says nothing.
SIMULATOR_DEFAULT = ("127.0.0.1", 4000)


def _simulator_of(ctx: Context):
    """The simulator's address for this run.

    It comes from the executor's options and not from a step parameter on
    purpose: it is **deployment configuration** (which box the executor talks
    to), not a condition of the measurement. What changes what is measured goes
    in the sequence, where it ends up in the report (ADR-0019, Rule 3) — the
    channel below is exactly that.
    """
    raw = ctx.options.get("simulator")
    if not raw:
        return SIMULATOR_DEFAULT
    host, _, port = raw.partition(":")
    return (host, int(port))


def _ask_simulator(host, port, command, timeout=2.0):
    """One request to the simulator: open TCP, send `command`, read the line.

    The protocol is deliberately trivial (a line of text, answered with
    `medida: <value>` or `ok`). When the simulator team closes their real
    contract this is replaced by their client, and nothing else in the executor
    moves: a step only ever returns a `Result`.
    """
    with socket.create_connection((host, port), timeout=timeout) as sock:
        sock.sendall((command + "\n").encode("utf-8"))
        return sock.recv(4096).decode("utf-8").strip()


@step(name="medir_simulador", outputs={"canal_usado": float})
def measure_simulator(ctx: Context, canal: float = 1) -> Result:
    """Measures against the TCP simulator and returns the reading.

    The **threshold is not here**: the step measures and the engine judges the
    value against the `limit` declared in the sequence (ADR-0008). The channel
    travels in the request and comes back as a named output, so two runs on
    different channels no longer produce identical reports (ADR-0020).
    """
    command = "medir" if canal == 1 else f"medir {canal}"
    try:
        line = _ask_simulator(*_simulator_of(ctx), command)
    except OSError as e:
        # The bench, not the unit: `error`, never `fail` (ADR-0019, Rule 2).
        return Result.error(f"could not talk to the simulator: {e}")
    if not line.lower().startswith("medida:"):
        return Result.error(f"unreadable answer from the simulator: {line!r}")
    return Result.measured(
        float(line.split(":", 1)[1].strip()),
        message=f"simulator answered {line}",
        outputs={"canal_usado": canal},
    )


@step(name="conectar_equipo")
def connect_instrument(ctx: Context) -> Result:
    """Connects to the simulated instrument; fails once, then passes.

    The same shape as `pasos_demo::conectar` in the embedded executor: a
    transient failure on attempt 1 that passes from attempt 2 (RF-09 — the
    attempt number reaches the step, through `ctx`).
    """
    if ctx.attempt == 1:
        return Result.failed("lost the simulator handshake (transient)")
    return Result.passed("connected")


@step(name="verificar_led")
def check_led() -> Result:
    """Checks the LED is lit: pass/fail with no measurement.

    The simplest step there is — and note it takes no `ctx`: a step only gets
    one if it asks for one.
    """
    return Result.passed("led lit")


# ---------------------------------------------------------------------------
# An object that stays here: the bench session (ADR-0022).
#
# `_ask_simulator` above opens a socket, asks and closes, every time. That is
# fine for one shot and wrong for a bench: a real instrument session is opened
# once, configured, measured against several times and closed — and it holds an
# open socket and, in production, a vendor driver lock. **It cannot travel.**
#
# So it stays in this process and the sequence carries a `Reference` to it. The
# steps below are the shape any executor's object steps take: one opens and
# mints, several use, one closes.
# ---------------------------------------------------------------------------


class Bench:
    """An open session with the bench: a live socket and the state set on it.

    Nothing about it could be serialised and handed to Anvil, which is the
    whole reason references exist.
    """

    def __init__(self, address, timeout=2.0):
        self.address = address
        self.sock = socket.create_connection(address, timeout=timeout)
        self.channel = 1

    def ask(self, command):
        self.sock.sendall((command + "\n").encode("utf-8"))
        return self.sock.recv(4096).decode("utf-8").strip()

    def close(self):
        self.sock.close()


@step(name="open_bench", outputs={"bench": Reference})
def open_bench(ctx: Context) -> Result:
    """Opens a session with the bench and hands back a handle to it."""
    try:
        bench = Bench(_simulator_of(ctx))
    except OSError as e:
        # The bench, not the unit: `error`, never `fail` (ADR-0019, Rule 2).
        return Result.error(f"could not open the bench: {e}")
    ref = ctx.objects.new(bench)
    return Result.passed(f"bench open at {bench.address}", outputs={"bench": ref})


@step(name="configure_bench", outputs={"bench": Reference})
def configure_bench(ctx: Context, bench: Reference, channel: float) -> Result:
    """Sets the channel on an already open bench.

    It answers **the same reference it was given**, and that is the point of
    ADR-0022 §5: the state changed, the identity did not. Minting a new handle
    here would break retries — the engine evaluates the parameters once and
    re-sends the same ones on every attempt, so attempt 2 would go out with a
    handle attempt 1 had already replaced.
    """
    try:
        session = ctx.objects.get(bench)
    except KeyError as e:
        return Result.error(str(e))
    session.channel = channel
    return Result.passed(f"channel {channel}", outputs={"bench": bench})


@step(name="measure_bench")
def measure_bench(ctx: Context, bench: Reference) -> Result:
    """Measures through the open session, on the channel it was configured for.

    The threshold is not here: the engine judges the value against the
    sequence's `limit` (ADR-0008).
    """
    try:
        session = ctx.objects.get(bench)
    except KeyError as e:
        return Result.error(str(e))
    try:
        line = session.ask("medir")
    except OSError as e:
        return Result.error(f"the bench session broke: {e}")
    if not line.lower().startswith("medida:"):
        return Result.error(f"unreadable answer from the bench: {line!r}")
    return Result.measured(
        float(line.split(":", 1)[1].strip()),
        message=f"channel {session.channel}",
    )


@step(name="close_bench")
def close_bench(ctx: Context, bench: Reference) -> Result:
    """Closes the session and spends the slot.

    The key is never handed out again, not even to the next `open_bench`: an
    old reference resolving to a live, different bench is the one failure Anvil
    cannot see from outside (ADR-0022 §7). `ObjectStore` is what guarantees it.
    """
    try:
        ctx.objects.close(bench).close()
    except KeyError as e:
        return Result.error(str(e))
    return Result.passed("bench closed")
