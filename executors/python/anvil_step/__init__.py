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

**Objects that stay here.** A bench, an instrument session, a connection: a
thing with open sockets that cannot travel and must not be reopened per step.
Keep it in ``ctx.objects`` and hand the sequence a ``Reference`` to it
(ADR-0022)::

    @step(outputs={"rack": Reference})
    def open_bench(ctx, address: str) -> Result:
        # Opens the bench and hands back a handle to it.
        return Result.passed(outputs={"rack": ctx.objects.new(Bench(address))})

    @step
    def set_voltage(ctx, rack: Reference, volts: float) -> Result:
        ctx.objects.get(rack).set_voltage(volts)
        return Result.passed()

The reference names a **slot**, so ``set_voltage`` answers no new handle: it
changed the bench, not which bench. See `ObjectStore` for the two duties an
executor owes here that no contract can check for it.
"""

from __future__ import annotations

import hashlib
import importlib.util
import inspect
import sys
from dataclasses import dataclass, field
from pathlib import Path
import itertools
import threading
import uuid
from typing import Any, Callable, Dict, Iterable, List, Optional

__all__ = [
    "step",
    "Result",
    "Context",
    "ParameterSpec",
    "OutputSpec",
    "StepSpec",
    "ModuleInfo",
    "MODULE_SEP",
    "Registry",
    "REGISTRY",
    "discover",
    "DiscoveryError",
    "Reference",
    "ObjectStore",
    "LIFETIME",
]

# The four types of the contract, and no more: a parameter that needs
# structure is a badly cut step (ADR-0020 §2). `UNSPECIFIED` is what an
# unannotated parameter gets — it means "unchecked", never "number".
UNSPECIFIED = "unspecified"
NUMBER = "number"
TEXT = "text"
BOOLEAN = "boolean"
REFERENCE = "reference"

#: This process's **life** (ADR-0022 §6). Minted once, at import, and published
#: in the catalog so Anvil can find out that the process holding its references
#: died and was born again — which is not a question a type system can answer.
#:
#: The executor owes two things the contract cannot verify (ADR-0022 §7): a
#: different lifetime on every start, which this is, and never recycling a
#: payload within one, which `ObjectStore` is.
LIFETIME = uuid.uuid4().hex

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
class Reference:
    """A handle to an object this executor keeps for itself (ADR-0022).

    The object never crosses the wire — it holds open sockets and vendor
    driver locks — so what travels is this, and Anvil never looks inside it.

    **It names a slot, not an object.** Mutating the state behind a reference
    does not change its identity: a step that reconfigures the bench answers
    with the very reference it was given. Mint a new one only when another
    object was really born — deriving one configuration from another,
    duplicating (ADR-0022 §5). Getting this wrong is not cosmetic: retries
    re-send the *same* parameters on every attempt, so an attempt that handed
    back a new handle would leave the next attempt holding one the executor
    already considers spent.

    You rarely build one by hand: ``ctx.objects.new(obj)`` mints it and
    ``ctx.objects.get(ref)`` gives the object back.
    """

    #: Minted by this executor, and opaque to Anvil.
    payload: str
    #: The life this reference was born under. Defaults to this process's.
    lifetime: str = ""
    #: Stamped by **Anvil** on the way out, because the executor does not know
    #: what the sequence called it (ADR-0022 §4). Steps can read it; nothing in
    #: the executor should decide anything on it.
    executor: str = ""


class ObjectStore:
    """The slots this executor keeps, and the two duties it owes (ADR-0022 §7).

    A step opens a bench, hands back a reference, and later steps look the
    bench up again by that reference::

        @step(outputs={"rack": Reference})
        def open_bench(ctx, address: str) -> Result:
            return Result.passed(outputs={"rack": ctx.objects.new(Bench(address))})

        @step
        def set_voltage(ctx, rack: Reference, volts: float) -> Result:
            ctx.objects.get(rack).set_voltage(volts)
            return Result.passed()      # the same slot: no new reference

    **Keys are never recycled**, not even after a slot is closed, and that is
    the duty Anvil cannot check from outside: if a closed bench's key came back
    for the next `open_bench`, an old reference would resolve cleanly to a
    live, *different* object — same executor, same lifetime, everything green,
    measuring against the wrong bench. A monotonic counter is what makes that
    impossible, and it is why this is a counter and not a free list.

    The store is thread-safe because ``server.py`` serves on a thread pool.
    """

    def __init__(self, lifetime: Optional[str] = None) -> None:
        #: Without one, a fresh identity: two stores are never the same life,
        #: which is what keeps a test store from being mistaken for the
        #: process's own. `server.py` passes this process's `LIFETIME`.
        self.lifetime = lifetime or uuid.uuid4().hex
        self._slots: Dict[str, Any] = {}
        self._closed: set = set()
        self._next = itertools.count(1)
        self._lock = threading.Lock()

    def new(self, obj: Any) -> Reference:
        """Puts an object in a **fresh** slot and returns its reference."""
        with self._lock:
            key = f"s{next(self._next)}"
            self._slots[key] = obj
            return Reference(payload=key, lifetime=self.lifetime)

    def get(self, ref: Reference) -> Any:
        """The object in that slot.

        Raises ``KeyError`` for a reference of another life, a closed slot or
        one that never existed. The executor is the one that knows this with
        certainty — Anvil only knows by comparison — so it says so rather than
        returning something plausible.
        """
        self._check(ref)
        with self._lock:
            return self._slots[ref.payload]

    def replace(self, ref: Reference, obj: Any) -> Reference:
        """Puts a different object **in the same slot**, and answers the same
        reference.

        This is what a by-value language needs. In LabVIEW a class is a value:
        the VI receives a box with the bench and returns a different box. The
        executor drops the new box in the same slot and answers the same
        reference, so LabVIEW's by-value nature stays inside the executor,
        which is where it does no harm (ADR-0022 §5).
        """
        self._check(ref)
        with self._lock:
            self._slots[ref.payload] = obj
        return ref

    def close(self, ref: Reference) -> Any:
        """Empties the slot and returns what was in it. The key stays spent."""
        self._check(ref)
        with self._lock:
            obj = self._slots.pop(ref.payload)
            self._closed.add(ref.payload)
            return obj

    def _check(self, ref: Reference) -> None:
        if not isinstance(ref, Reference):
            raise KeyError(f"expected an object reference, got a {type(ref).__name__}")
        # A reference of another life is rejected here, with certainty, and not
        # only by Anvil's comparison (ADR-0022 §6). An executor that does not
        # publish a lifetime leaves the check to nobody, so a reference that
        # carries none is not waved through either.
        if ref.lifetime != self.lifetime:
            raise KeyError(
                f"the reference '{ref.payload}' is from the life '{ref.lifetime}' and "
                f"this executor is on '{self.lifetime}': it was minted by a process "
                f"that no longer exists"
            )
        with self._lock:
            if ref.payload in self._closed:
                raise KeyError(f"the reference '{ref.payload}' names a slot already closed")
            if ref.payload not in self._slots:
                raise KeyError(f"the reference '{ref.payload}' names no slot of this executor")

    def __len__(self) -> int:
        with self._lock:
            return len(self._slots)


#: The store `server.py` hands to every step through `ctx.objects`. One per
#: process, like the registry: slots and their lifetime are process state.
OBJECTS = ObjectStore(LIFETIME)

# `bool` before `int`: in Python `bool` is a subclass of `int`, and a step
# declaring `flag: bool` must not be described as taking a number.
_TYPES = {bool: BOOLEAN, int: NUMBER, float: NUMBER, str: TEXT, Reference: REFERENCE}


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
class ModuleInfo:
    """A module this executor serves: where it came from, and what it was.

    The hash is read off the file and identifies the artifact that answered.
    ADR-0025 §6 wants it in every run's report; there is no field in
    `paso.proto` to carry it yet, so for now it is logged and shown by
    ``--list``.
    """

    file: Path
    sha256: str


def _sha256(file: Path) -> str:
    try:
        return hashlib.sha256(file.read_bytes()).hexdigest()
    except OSError:
        # A module that was imported and then vanished is not worth failing a
        # bench over; the empty hash says "unknown", never a wrong one.
        return ""


#: What separates a module's logical name from a step's, in the name a
#: sequence writes and Anvil dispatches on (ADR-0026 §1). The same separator
#: the WASM bridge uses: the two departments speak one language.
MODULE_SEP = "/"


@dataclass(frozen=True)
class StepSpec:
    """A step's signature: what Anvil can check without running it."""

    name: str
    inputs: List[ParameterSpec] = field(default_factory=list)
    outputs: List[OutputSpec] = field(default_factory=list)
    doc: str = ""
    #: The module this step lives in — the logical name of its ``.py``
    #: (ADR-0026 §2). It is **derived from the file**, never declared: the
    #: author of a step writes a function and nothing else, and a module that
    #: is renamed or moved does not need its steps edited.
    module: str = ""

    @property
    def qualified(self) -> str:
        """The name a sequence writes: ``<module>/<step>``.

        Two modules can now each serve a ``medir_voltaje`` — which is the whole
        point — so the module is part of the address and not decoration.
        """
        return f"{self.module}{MODULE_SEP}{self.name}" if self.module else self.name


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
    - ``objects``: this executor's slots (ADR-0022). ``objects.new(obj)`` mints
      a reference the sequence can carry from step to step; ``objects.get(ref)``
      gives the object back. It is here and not a parameter because the store
      belongs to the **process**, not to the measurement: a step that could be
      handed a different store would be a step that could be handed a different
      bench without the sequence saying so.
    """

    attempt: int = 1
    options: Dict[str, str] = field(default_factory=dict)
    step_name: str = ""
    objects: ObjectStore = field(default_factory=ObjectStore)


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
        for name in inputs:
            if not any(p.name == name for p in self.spec.inputs):
                conocidos = ", ".join(p.name for p in self.spec.inputs) or "none"
                return (
                    f"the step '{self.spec.name}' does not take a parameter "
                    f"called '{name}' (it takes: {conocidos})"
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
    """The steps this executor serves, keyed by the name a sequence uses.

    That key is the **qualified** name, ``<module>/<step>`` (ADR-0026): this
    executor is a department that serves several modules, and two of them may
    each have a ``medir_voltaje`` without either shadowing the other.
    """

    def __init__(self) -> None:
        self._steps: Dict[str, Step] = {}
        #: Logical module name → (file, sha256). Filled by ``discover``; what
        #: answers "what do you serve, and from which artifact".
        self.modules: Dict[str, "ModuleInfo"] = {}

    def add(self, s: Step) -> None:
        key = s.spec.qualified
        earlier = self._steps.get(key)
        if earlier is not None:
            # Same name in the same module: still an error, and the same one as
            # before. Two modules sharing a step name is no longer a clash —
            # that is what qualifying bought.
            raise DiscoveryError(
                f"two steps are called '{key}': "
                f"{_where(earlier.func)} and {_where(s.func)}. "
                f"The name is what a sequence dispatches on, so it must be unique"
            )
        self._steps[key] = s

    def add_module(self, name: str, file: Path) -> None:
        """Records a module and its artifact hash, refusing a name clash.

        Two files with the same stem under different steps paths would make a
        step name ambiguous, and serving the wrong one is worse than not
        starting: the run would measure with a module nobody asked for and the
        report would not say so.
        """
        earlier = self.modules.get(name)
        if earlier is not None:
            raise DiscoveryError(
                f"two modules are both called '{name}': {earlier.file} and {file}. "
                f"A module's logical name is its file stem, so rename one of the two"
            )
        self.modules[name] = ModuleInfo(file=file, sha256=_sha256(file))

    def get(self, name: str) -> Optional[Step]:
        return self._steps.get(name)

    def suggest(self, name: str) -> List[str]:
        """Qualified names whose step half is ``name``.

        For the message when a sequence sends a bare name — the mistake this
        change makes easy, and one worth answering with the fix rather than
        with a list of everything served.
        """
        if MODULE_SEP in name:
            return []
        return sorted(k for k, s in self._steps.items() if s.spec.name == name)

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
_CURRENT: Optional[Registry] = None


def _target_registry(registry: Optional[Registry]) -> Registry:
    """Where a `@step` goes: the one named, the one being discovered, or the
    process-wide one.

    Written with `is not None` and never with `or`: an empty `Registry` is
    falsy —it has a `__len__`— so `registry or REGISTRY` would quietly send
    every step of a fresh registry to the global one. That bug is invisible
    until two tests collide.
    """
    if registry is not None:
        return registry
    return _CURRENT if _CURRENT is not None else REGISTRY


def _logical_name(file: Path) -> str:
    """A module's logical name: the stem of its ``.py`` (ADR-0026 §2).

    ``instrument.py`` is ``instrument``. A package's ``__init__.py`` takes the
    package's own directory name instead — nobody writes a sequence against a
    module called ``__init__``.
    """
    return file.parent.name if file.stem == "__init__" else file.stem


def _module_of(func: Callable[..., Any]) -> str:
    """The logical module of the file a step is written in.

    **Derived from the file, never declared.** The author of a step writes a
    function and nothing else, so a module that is renamed or moved needs no
    edit inside it — the same rule the WASM bridge follows for a `.wasm`.

    A function with no readable file —built at runtime, typed into a REPL—
    gets no module and registers under its bare name. That is a real case
    (``exec``, a generated step), and giving it a made-up module would be
    worse than leaving it unqualified.
    """
    try:
        file = Path(func.__code__.co_filename)
    except AttributeError:
        return ""
    if file.suffix != ".py":
        return ""
    return _logical_name(file)


def _where(func: Callable[..., Any]) -> str:
    """``file:line`` of a step function, for the ambiguity message."""
    try:
        code = func.__code__
        return f"{code.co_filename}:{code.co_firstlineno}"
    except AttributeError:
        return getattr(func, "__name__", "<unknown>")


def _type_mismatch(declared: str, value: Any) -> Optional[str]:
    """The name of the type that arrived when it is not the declared one.

    ``UNSPECIFIED`` accepts anything: a type the step never claimed cannot be
    contradicted. The lookup is by exact ``type`` and not by ``isinstance``,
    which is what keeps ``True`` out of a number parameter — in Python ``bool``
    is a subclass of ``int``, so an ``isinstance`` test would let a boolean
    through and the step would measure on channel 1 while the sequence said
    ``true``.
    """
    if declared == UNSPECIFIED:
        return None
    arrived = _TYPES.get(type(value), UNSPECIFIED)
    return None if arrived == declared else (arrived if arrived != UNSPECIFIED else type(value).__name__)


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

    def decorate(f: Callable[..., Any]) -> Callable[..., Any]:
        inputs, wants_ctx = _inputs_of(f)
        doc = (inspect.getdoc(f) or "").strip().split("\n")[0]
        spec = StepSpec(
            name=name or f.__name__,
            inputs=inputs,
            outputs=_outputs_of(outputs),
            doc=doc,
            module=_module_of(f),
        )
        _target_registry(registry).add(Step(spec=spec, func=f, wants_ctx=wants_ctx))
        # The function is returned untouched: a step is still an ordinary
        # function and its own unit tests call it directly, with no executor
        # and no gRPC in the way.
        return f

    return decorate if func is None else decorate(func)


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
    global _CURRENT
    reg = _target_registry(registry)
    previous, _CURRENT = _CURRENT, reg
    try:
        return _discover_into(paths, reg)
    finally:
        _CURRENT = previous


def _discover_into(paths: Iterable[str], reg: Registry) -> List[Path]:
    loaded: List[Path] = []
    for raw in paths:
        p = Path(raw).expanduser()
        if not p.exists():
            raise DiscoveryError(f"the steps path '{p}' does not exist")
        if p.is_file():
            # The module is recorded **before** importing: a name clash between
            # two steps paths is decided by the file name, and finding it out
            # before running anybody's module-level code is the whole point.
            reg.add_module(_logical_name(p), p.resolve())
            _import_one(p, reg)
            loaded.append(p)
            continue
        dir_str = str(p.resolve())
        if dir_str not in sys.path:
            sys.path.insert(0, dir_str)
        for child in sorted(p.iterdir()):
            if child.name.startswith(("_", ".")):
                continue
            if child.suffix == ".py" or (child.is_dir() and (child / "__init__.py").exists()):
                target = child / "__init__.py" if child.is_dir() else child
                reg.add_module(_logical_name(target), target.resolve())
                _import_one(target, reg)
                loaded.append(child)
    return loaded


def _import_one(file: Path, reg: Registry) -> None:
    """Imports one module by path, under a name that cannot collide.

    The name carries a digest of the full path: two ``instrument.py`` under
    different steps paths are two different modules, and letting the second
    overwrite the first in ``sys.modules`` would serve a catalog nobody asked
    for.
    """
    path = file.resolve()
    digest = hashlib.sha256(str(path).encode("utf-8")).hexdigest()[:8]
    name = f"anvil_steps_{path.stem}_{digest}"
    spec = importlib.util.spec_from_file_location(name, file)
    if spec is None or spec.loader is None:
        raise DiscoveryError(f"'{file}' cannot be imported as a Python module")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    try:
        spec.loader.exec_module(module)
    except DiscoveryError:
        raise
    except Exception as e:  # noqa: BLE001 — the file is the user's, not ours
        raise DiscoveryError(f"'{file}' failed while importing: {e}") from e
