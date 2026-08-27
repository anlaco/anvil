# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 ANLACO
"""Write a step for Anvil in Python: decorate a function and drop the file in.

This is the whole authoring surface of the Python executor (ADR-0021). You
never edit ``server.py``: you put a module under the steps path and the
executor discovers it, serves it, and describes it to Anvil.

    from anvil_step import step, Result

    @step(outputs={"channel_used": float})
    def measure_voltage(ctx, channel: float = 1) -> Result:
        \"\"\"Measures the DC voltage on a channel.\"\"\"
        volts = read_instrument(channel)
        return Result.measured(volts, outputs={"channel_used": channel})

**The signature is the catalog.** Names, types, and which inputs are required
come from the function's own parameters and annotations — nothing is written
twice, so nothing can drift. What Python cannot infer, the decorator takes:
``outputs`` (a return value has no names) and ``name`` (when the step's name in
a sequence is not a valid Python identifier).

What an executor cannot infer, it does not claim: an unannotated parameter is
described as *unspecified*, and Anvil leaves it unchecked rather than guessing
it is a number (ADR-0019, Rule 2).
"""

from __future__ import annotations

import hashlib
import importlib.util
import inspect
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, List, Optional

__all__ = [
    "step",
    "Result",
    "Context",
    "ParameterSpec",
    "OutputSpec",
    "StepSpec",
    "Registry",
    "REGISTRY",
    "discover",
    "DiscoveryError",
]

# The three types of the contract, and no more: a parameter that needs
# structure is a badly cut step (ADR-0020 §2). `UNSPECIFIED` is what an
# unannotated parameter gets — it means "unchecked", never "number".
UNSPECIFIED = "unspecified"
NUMBER = "number"
TEXT = "text"
BOOLEAN = "boolean"

# `bool` before `int`: in Python `bool` is a subclass of `int`, and a step
# declaring `flag: bool` must not be described as taking a number.
_TYPES = {bool: BOOLEAN, int: NUMBER, float: NUMBER, str: TEXT}

# The step's verdict vocabulary, closed and lowercase, exactly as `paso.proto`
# defines it. "skipped" is not here: only the engine produces it.
PASS = "pass"
FAIL = "fail"
ERROR = "error"


class DiscoveryError(Exception):
    """A steps path that cannot be loaded, or two steps with the same name.

    It is raised at start-up and kills the executor on purpose: an ambiguous
    catalog served quietly means the bench runs whichever of the two steps
    happened to load last.
    """


@dataclass(frozen=True)
class ParameterSpec:
    """One input a step accepts, as Anvil will be told about it."""

    name: str
    type: str = UNSPECIFIED
    required: bool = True
    default: Any = None
    doc: str = ""


@dataclass(frozen=True)
class OutputSpec:
    """One named value the step returns besides its measurement."""

    name: str
    type: str = UNSPECIFIED
    doc: str = ""


@dataclass(frozen=True)
class StepSpec:
    """A step's signature: what Anvil can check without running it."""

    name: str
    inputs: List[ParameterSpec] = field(default_factory=list)
    outputs: List[OutputSpec] = field(default_factory=list)
    doc: str = ""


@dataclass
class Context:
    """What the executor knows about this invocation and the step does not.

    A step receives it only if it declares a ``ctx`` parameter, and ``ctx`` is
    never part of the described signature: it is the executor talking to the
    step, not a value that comes out of the sequence.

    - ``attempt``: attempt number, starting at 1 (RF-09). It is here and not in
      the signature because it is not something the sequence sets.
    - ``options``: whatever was passed with ``--option key=value`` when the
      executor was started — deployment configuration (an instrument address, a
      bench id), not a parameter of the measurement. Anything that changes
      *what is measured* belongs in the sequence, where it ends up in the
      report (ADR-0019, Rule 3).
    - ``step_name``: the name the sequence used, useful when one function
      serves several names.
    """

    attempt: int = 1
    options: Dict[str, str] = field(default_factory=dict)
    step_name: str = ""


@dataclass
class Result:
    """What a step gives back.

    Build it with the constructors below rather than by hand — ``status`` is a
    closed vocabulary and a string Anvil cannot read turns the step into
    ``error`` on the other side.

    A step may also return a plain value instead of a ``Result``:

    - a number → a measurement that passed (``Result.measured``),
    - a ``bool`` → pass/fail, the simplest step there is,
    - ``None`` → passed with no measurement.

    **The threshold is not the step's business**: return the measurement and
    let the engine judge it against the sequence's ``limit`` (ADR-0008).
    """

    status: str = PASS
    message: str = ""
    measured_value: Optional[float] = None
    outputs: Dict[str, Any] = field(default_factory=dict)

    @classmethod
    def passed(cls, message: str = "", outputs: Optional[Dict[str, Any]] = None) -> "Result":
        return cls(PASS, message, None, dict(outputs or {}))

    @classmethod
    def failed(cls, message: str = "", outputs: Optional[Dict[str, Any]] = None) -> "Result":
        """The unit does not comply. Information about the DUT."""
        return cls(FAIL, message, None, dict(outputs or {}))

    @classmethod
    def error(cls, message: str = "", outputs: Optional[Dict[str, Any]] = None) -> "Result":
        """The step could not judge. Information about the bench, not the DUT."""
        return cls(ERROR, message, None, dict(outputs or {}))

    @classmethod
    def measured(
        cls,
        value: float,
        message: str = "",
        outputs: Optional[Dict[str, Any]] = None,
        status: str = PASS,
    ) -> "Result":
        return cls(status, message, float(value), dict(outputs or {}))

    @classmethod
    def of(cls, value: Any) -> "Result":
        """Whatever a step returned, as a ``Result``.

        ``bool`` is checked before the numeric types on purpose: in Python
        ``True`` is also an ``int``, and a pass/fail step returning ``False``
        must not be recorded as "measured 0.0, passed".
        """
        if isinstance(value, Result):
            return value
        if value is None:
            return cls.passed()
        if isinstance(value, bool):
            return cls.passed() if value else cls.failed("the step returned False")
        if isinstance(value, (int, float)):
            return cls.measured(value)
        raise TypeError(
            f"a step returns a Result, a number, a bool or None; it returned "
            f"{type(value).__name__}"
        )


@dataclass
class Step:
    """A registered step: the function to call and the signature to publish."""

    spec: StepSpec
    func: Callable[..., Any]
    #: Whether the function declared a ``ctx`` parameter to be injected.
    wants_ctx: bool = False

    def check(self, inputs: Dict[str, Any]) -> Optional[str]:
        """Why these inputs do not fit the signature, or ``None`` if they do.

        The executor enforces its own catalog. If it did not, the catalog would
        be a promise nobody keeps: a parameter the step does not know would be
        dropped by Python, or worse, quietly swallowed, and the step would
        **measure something else and say pass** — the same false green the
        contract echo exists to prevent (ADR-0020 §4b).

        A mismatch is always ``error``, never ``fail`` and never a default.
        """
        for nombre in inputs:
            if not any(p.name == nombre for p in self.spec.inputs):
                conocidos = ", ".join(p.name for p in self.spec.inputs) or "none"
                return (
                    f"the step '{self.spec.name}' does not take a parameter "
                    f"called '{nombre}' (it takes: {conocidos})"
                )
        for p in self.spec.inputs:
            if p.name not in inputs:
                if p.required:
                    return (
                        f"the step '{self.spec.name}' needs the parameter "
                        f"'{p.name}' and the sequence did not send it"
                    )
                continue
            malo = _type_mismatch(p.type, inputs[p.name])
            if malo is not None:
                return (
                    f"the parameter '{p.name}' of '{self.spec.name}' is "
                    f"{p.type} and a {malo} arrived"
                )
        return None

    def invoke(self, ctx: Context, inputs: Dict[str, Any]) -> Result:
        """Runs the step, turning **any** exception into ``error``.

        Never ``fail``: a step that blew up says nothing about the unit under
        test, and reporting it as a failed unit is the false red that mirrors
        ADR-0019's false green. Never a crash either (RF-12): the executor
        serves the next step.
        """
        malo = self.check(inputs)
        if malo is not None:
            return Result.error(malo)
        kwargs = dict(inputs)
        if self.wants_ctx:
            kwargs["ctx"] = ctx
        try:
            return Result.of(self.func(**kwargs))
        except Exception as e:  # noqa: BLE001 — deliberate: see the docstring
            return Result.error(f"the step raised {type(e).__name__}: {e}")


class Registry:
    """The steps this executor serves, keyed by the name a sequence uses."""

    def __init__(self) -> None:
        self._steps: Dict[str, Step] = {}

    def add(self, s: Step) -> None:
        previo = self._steps.get(s.spec.name)
        if previo is not None:
            raise DiscoveryError(
                f"two steps are called '{s.spec.name}': "
                f"{_where(previo.func)} and {_where(s.func)}. "
                f"The name is what a sequence dispatches on, so it must be unique"
            )
        self._steps[s.spec.name] = s

    def get(self, name: str) -> Optional[Step]:
        return self._steps.get(name)

    def catalog(self) -> List[StepSpec]:
        """The signatures, sorted by name so two runs describe identically."""
        return [self._steps[n].spec for n in sorted(self._steps)]

    def __len__(self) -> int:
        return len(self._steps)


#: The registry `@step` writes to and `server.py` serves. One per process.
REGISTRY = Registry()

#: The registry `discover` is filling right now, if any. It exists so that a
#: step module —which imports `step` and knows nothing about registries— lands
#: in the registry the caller asked for. It is what makes the SDK testable
#: without a process-wide global.
_ACTUAL: Optional[Registry] = None


def _destino(registry: Optional[Registry]) -> Registry:
    """Where a `@step` goes: the one named, the one being discovered, or the
    process-wide one.

    Written with `is not None` and never with `or`: an empty `Registry` is
    falsy —it has a `__len__`— so `registry or REGISTRY` would quietly send
    every step of a fresh registry to the global one. That bug is invisible
    until two tests collide.
    """
    if registry is not None:
        return registry
    return _ACTUAL if _ACTUAL is not None else REGISTRY


def _where(func: Callable[..., Any]) -> str:
    """``file:line`` of a step function, for the ambiguity message."""
    try:
        code = func.__code__
        return f"{code.co_filename}:{code.co_firstlineno}"
    except AttributeError:
        return getattr(func, "__name__", "<unknown>")


def _type_mismatch(declarado: str, valor: Any) -> Optional[str]:
    """The name of the type that arrived when it is not the declared one.

    ``UNSPECIFIED`` accepts anything: a type the step never claimed cannot be
    contradicted. The lookup is by exact ``type`` and not by ``isinstance``,
    which is what keeps ``True`` out of a number parameter — in Python ``bool``
    is a subclass of ``int``, so an ``isinstance`` test would let a boolean
    through and the step would measure on channel 1 while the sequence said
    ``true``.
    """
    if declarado == UNSPECIFIED:
        return None
    llego = _TYPES.get(type(valor), UNSPECIFIED)
    return None if llego == declarado else (llego if llego != UNSPECIFIED else type(valor).__name__)


def _type_of(annotation: Any) -> str:
    """The contract type of an annotation, or ``UNSPECIFIED`` if it is not one
    of the three. What cannot be described is left unchecked, not guessed."""
    return _TYPES.get(annotation, UNSPECIFIED)


def _inputs_of(func: Callable[..., Any]) -> tuple:
    """Reads the function's signature into ``ParameterSpec``s.

    Returns ``(inputs, wants_ctx)``. ``ctx`` is dropped from the description:
    the executor injects it, so it is not something the sequence can set.

    ``*args``/``**kwargs`` are rejected. A step whose signature is open cannot
    be described, and a catalog that lies about what a step accepts is worse
    than no catalog: it turns a typo into a checked-and-approved typo.
    """
    inputs: List[ParameterSpec] = []
    wants_ctx = False
    for p in inspect.signature(func).parameters.values():
        if p.kind in (p.VAR_POSITIONAL, p.VAR_KEYWORD):
            raise DiscoveryError(
                f"the step '{func.__name__}' ({_where(func)}) takes *args or "
                f"**kwargs: a signature that open cannot be described, and Anvil "
                f"would have to accept any parameter name without checking it"
            )
        if p.name == "ctx":
            wants_ctx = True
            continue
        inputs.append(
            ParameterSpec(
                name=p.name,
                type=_type_of(p.annotation),
                required=p.default is inspect.Parameter.empty,
                default=None if p.default is inspect.Parameter.empty else p.default,
            )
        )
    return inputs, wants_ctx


def _outputs_of(declared: Optional[Dict[str, Any]]) -> List[OutputSpec]:
    """The declared outputs. This is the one thing the signature cannot give:
    a Python return value carries no names."""
    return [OutputSpec(name=n, type=_type_of(t)) for n, t in (declared or {}).items()]


def step(
    func: Optional[Callable[..., Any]] = None,
    *,
    name: Optional[str] = None,
    outputs: Optional[Dict[str, Any]] = None,
    registry: Optional[Registry] = None,
):
    """Registers a function as a step Anvil can call and describe.

    Works bare (``@step``) or with arguments (``@step(outputs={...})``).

    - ``name``: the name a sequence uses. Defaults to the function's name — use
      this only when the two cannot be the same.
    - ``outputs``: ``{"name": type}`` for the values the step returns besides
      its measurement, the ones ``assign`` reads as ``result.outputs.<name>``.
      Declaring them is what lets Anvil check that expression without running
      the sequence (ADR-0020 §3).

    The docstring's first line becomes the step's ``doc`` in the catalog.
    """

    def decora(f: Callable[..., Any]) -> Callable[..., Any]:
        entradas, wants_ctx = _inputs_of(f)
        doc = (inspect.getdoc(f) or "").strip().split("\n")[0]
        spec = StepSpec(
            name=name or f.__name__,
            inputs=entradas,
            outputs=_outputs_of(outputs),
            doc=doc,
        )
        _destino(registry).add(Step(spec=spec, func=f, wants_ctx=wants_ctx))
        # The function is returned untouched: a step is still an ordinary
        # function and its own unit tests call it directly, with no executor
        # and no gRPC in the way.
        return f

    return decora if func is None else decora(func)


def discover(paths: Iterable[str], registry: Optional[Registry] = None) -> List[Path]:
    """Imports every step module under ``paths``, registering what it declares.

    A path is a directory (its top-level ``.py`` files, sorted, plus its
    packages) or a single ``.py`` file. Files starting with ``_`` are skipped,
    so helpers a step imports are not themselves loaded as step modules.

    The directory is put on ``sys.path`` so modules there can import each
    other, which is what makes a folder of steps a project rather than a pile
    of files.

    A path that does not exist is a ``DiscoveryError``: an executor that
    silently serves nothing because of a typo in a path is the emptiest
    possible false green.
    """
    global _ACTUAL
    reg = _destino(registry)
    anterior, _ACTUAL = _ACTUAL, reg
    try:
        return _descubre(paths, reg)
    finally:
        _ACTUAL = anterior


def _descubre(paths: Iterable[str], reg: Registry) -> List[Path]:
    cargados: List[Path] = []
    for bruto in paths:
        p = Path(bruto).expanduser()
        if not p.exists():
            raise DiscoveryError(f"the steps path '{p}' does not exist")
        if p.is_file():
            _importa(p, reg)
            cargados.append(p)
            continue
        dir_str = str(p.resolve())
        if dir_str not in sys.path:
            sys.path.insert(0, dir_str)
        for hijo in sorted(p.iterdir()):
            if hijo.name.startswith(("_", ".")):
                continue
            if hijo.suffix == ".py" or (hijo.is_dir() and (hijo / "__init__.py").exists()):
                destino = hijo / "__init__.py" if hijo.is_dir() else hijo
                _importa(destino, reg)
                cargados.append(hijo)
    return cargados


def _importa(fichero: Path, reg: Registry) -> None:
    """Imports one module by path, under a name that cannot collide.

    The name carries a digest of the full path: two ``instrument.py`` under
    different steps paths are two different modules, and letting the second
    overwrite the first in ``sys.modules`` would serve a catalog nobody asked
    for.
    """
    ruta = fichero.resolve()
    huella = hashlib.sha256(str(ruta).encode("utf-8")).hexdigest()[:8]
    nombre = f"anvil_steps_{ruta.stem}_{huella}"
    spec = importlib.util.spec_from_file_location(nombre, fichero)
    if spec is None or spec.loader is None:
        raise DiscoveryError(f"'{fichero}' cannot be imported as a Python module")
    modulo = importlib.util.module_from_spec(spec)
    sys.modules[nombre] = modulo
    try:
        spec.loader.exec_module(modulo)
    except DiscoveryError:
        raise
    except Exception as e:  # noqa: BLE001 — the file is the user's, not ours
        raise DiscoveryError(f"'{fichero}' failed while importing: {e}") from e
