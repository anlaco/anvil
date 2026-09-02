# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 ANLACO
"""Tests for the Python executor's step SDK (issues #54 and #45).

They use `unittest` from the standard library and **do not import grpc**: the
SDK is the authoring surface, and it has to be testable —by us and by whoever
writes a step— without a gRPC stack, a bench, or a running executor.

    python3 -m unittest discover executors/python
"""

import tempfile
import unittest
from pathlib import Path

from anvil_step import (
    BOOLEAN,
    NUMBER,
    REFERENCE,
    TEXT,
    UNSPECIFIED,
    Context,
    DiscoveryError,
    ObjectStore,
    Reference,
    Registry,
    Result,
    discover,
    step,
)

#: A step is addressed `<module>/<step>` (ADR-0026), and the module is the
#: logical name of the file it lives in. The steps declared in this file
#: therefore register under this file's own name.
HERE = Path(__file__).stem


class SignatureIsTheCatalog(unittest.TestCase):
    """The point of #54: what a step accepts is read off the function itself,
    so the catalog cannot drift from the code that runs."""

    def setUp(self):
        self.reg = Registry()

    def test_names_types_and_defaults_come_from_the_signature(self):
        @step(registry=self.reg, outputs={"serie": str})
        def medir(ctx: Context, canal: float, escala: str = "auto", rapido: bool = False):
            """Mide algo."""
            return Result.measured(1.0)

        spec = self.reg.get(f"{HERE}/medir").spec
        self.assertEqual([p.name for p in spec.inputs], ["canal", "escala", "rapido"])
        self.assertEqual([p.type for p in spec.inputs], [NUMBER, TEXT, BOOLEAN])
        # `ctx` is the executor talking to the step, not something the sequence
        # sets: it must never appear in the catalog.
        self.assertNotIn("ctx", [p.name for p in spec.inputs])
        self.assertTrue(spec.inputs[0].required, "sin default es obligatorio")
        self.assertFalse(spec.inputs[1].required)
        self.assertEqual(spec.inputs[1].default, "auto")
        self.assertEqual(spec.doc, "Mide algo.")
        self.assertEqual([o.name for o in spec.outputs], ["serie"])

    def test_an_unannotated_parameter_is_unchecked_not_a_number(self):
        """ADR-0019, Rule 2 applied to the description: what the executor
        cannot state, it does not claim."""

        @step(registry=self.reg)
        def medir(canal):
            return None

        self.assertEqual(self.reg.get(f"{HERE}/medir").spec.inputs[0].type, UNSPECIFIED)

    def test_a_signature_that_cannot_be_described_is_rejected(self):
        """`**kwargs` would make the catalog accept any name without checking
        it — a typo, checked and approved."""
        with self.assertRaises(DiscoveryError) as e:

            @step(registry=self.reg)
            def medir(**kwargs):
                return None

        self.assertIn("**kwargs", str(e.exception))

    def test_two_steps_with_the_same_name_are_refused(self):
        """The name is what the sequence dispatches on. Serving two would mean
        the bench runs whichever module happened to load last."""

        @step(name="medir", registry=self.reg)
        def uno():
            return None

        with self.assertRaises(DiscoveryError) as e:

            @step(name="medir", registry=self.reg)
            def dos():
                return None

        self.assertIn("medir", str(e.exception))


class TheExecutorHonoursItsOwnCatalog(unittest.TestCase):
    """A catalog nobody enforces is a promise nobody keeps: the step would
    measure something else and still say pass."""

    def setUp(self):
        self.reg = Registry()

        @step(registry=self.reg)
        def medir(canal: float, escala: str = "auto"):
            return Result.measured(canal, message=escala)

        # A step that would happily run with the wrong type and **still say
        # pass**. It is the one that shows the check earning its keep: with
        # `medir` above, a bad value blows up inside the function and comes out
        # as `error` anyway, so it cannot tell a checked executor from an
        # unchecked one.
        @step(registry=self.reg)
        def etiquetar(valor: float):
            return Result.passed(message=str(valor))

        self.paso = self.reg.get(f"{HERE}/medir")
        self.laxo = self.reg.get(f"{HERE}/etiquetar")

    def test_an_unknown_input_names_the_ones_the_step_does_take(self):
        """Python would raise a `TypeError` anyway; what the check adds is the
        answer in the same line, because the mistake is nearly always a typo."""
        r = self.paso.invoke(Context(), {"canal": 1.0, "canall": 3.0})
        self.assertEqual(r.status, "error")
        self.assertIn("canall", r.message)
        self.assertIn("canal, escala", r.message)

    def test_a_missing_required_input_is_an_error(self):
        r = self.paso.invoke(Context(), {})
        self.assertEqual(r.status, "error")
        self.assertIn("canal", r.message)

    def test_a_wrong_type_is_an_error_and_never_a_quiet_pass(self):
        r = self.laxo.invoke(Context(), {"valor": "dos"})
        self.assertEqual(r.status, "error", "sin la comprobación diría 'pass'")
        self.assertIn("number", r.message)

    def test_a_bool_does_not_pass_as_a_number(self):
        """In Python `bool` is a subclass of `int`. Letting `True` through
        would measure on channel 1 while the sequence said `true`."""
        r = self.laxo.invoke(Context(), {"valor": True})
        self.assertEqual(r.status, "error")
        self.assertIn("boolean", r.message)

    def test_a_missing_optional_input_is_the_steps_business(self):
        r = self.paso.invoke(Context(), {"canal": 2.0})
        self.assertEqual(r.status, "pass")
        self.assertEqual(r.measured_value, 2.0)
        self.assertEqual(r.message, "auto")


class WhatAStepReturns(unittest.TestCase):
    def setUp(self):
        self.reg = Registry()

    def test_an_exception_is_error_and_never_fail(self):
        """A step that blew up says nothing about the unit under test.
        Recording it as a failed unit is the false red."""

        @step(registry=self.reg)
        def revienta():
            raise ValueError("el instrumento no está")

        r = self.reg.get(f"{HERE}/revienta").invoke(Context(), {})
        self.assertEqual(r.status, "error")
        self.assertIn("ValueError", r.message)

    def test_a_bare_bool_is_pass_fail_and_not_a_measurement_of_zero(self):
        self.assertEqual(Result.of(False).status, "fail")
        self.assertIsNone(Result.of(False).measured_value, "False no es medir 0")
        self.assertEqual(Result.of(True).status, "pass")

    def test_a_bare_number_is_a_measurement(self):
        r = Result.of(4.2)
        self.assertEqual(r.status, "pass")
        self.assertEqual(r.measured_value, 4.2)

    def test_none_is_a_pass_with_no_measurement(self):
        self.assertEqual(Result.of(None).status, "pass")
        self.assertIsNone(Result.of(None).measured_value)

    def test_the_attempt_reaches_the_step_through_ctx(self):
        """RF-09, and the reason `ctx` exists: the attempt number is not
        something the sequence sets, so it is not a parameter."""

        @step(registry=self.reg)
        def transitorio(ctx: Context):
            return ctx.attempt >= 2

        s = self.reg.get(f"{HERE}/transitorio")
        self.assertEqual(s.invoke(Context(attempt=1), {}).status, "fail")
        self.assertEqual(s.invoke(Context(attempt=2), {}).status, "pass")


class Discovery(unittest.TestCase):
    """#54: a step of your own is added by dropping a file in, and `server.py`
    is never edited."""

    def escribe(self, dirname, nombre, cuerpo):
        p = Path(dirname) / nombre
        p.write_text(cuerpo, encoding="utf-8")
        return p

    def test_a_module_dropped_in_a_directory_is_served(self):
        reg = Registry()
        with tempfile.TemporaryDirectory() as d:
            self.escribe(
                d,
                "mios.py",
                "from anvil_step import step\n"
                "@step\n"
                "def apretar(par: float):\n"
                "    '''Aprieta.'''\n"
                "    return par > 1.0\n",
            )
            discover([d], registry=reg)
        self.assertEqual([s.qualified for s in reg.catalog()], ["mios/apretar"])

    def test_files_starting_with_underscore_are_not_step_modules(self):
        """So a helper a step imports is not itself loaded as a module."""
        reg = Registry()
        with tempfile.TemporaryDirectory() as d:
            self.escribe(d, "_ayuda.py", "raise AssertionError('no debería cargarse')\n")
            discover([d], registry=reg)
        self.assertEqual(len(reg), 0)

    def test_a_steps_path_that_does_not_exist_is_refused(self):
        """An executor that silently serves nothing because of a typo in a
        path is the emptiest possible false green."""
        with self.assertRaises(DiscoveryError):
            discover(["/no/existe/de/ninguna/manera"], registry=Registry())

    def test_a_module_that_fails_to_import_names_the_file(self):
        reg = Registry()
        with tempfile.TemporaryDirectory() as d:
            self.escribe(d, "roto.py", "import modulo_que_no_existe\n")
            with self.assertRaises(DiscoveryError) as e:
                discover([d], registry=reg)
        self.assertIn("roto.py", str(e.exception))

    def test_the_catalog_is_sorted_so_two_runs_describe_identically(self):
        reg = Registry()
        with tempfile.TemporaryDirectory() as d:
            self.escribe(
                d,
                "varios.py",
                "from anvil_step import step\n"
                "@step\n"
                "def zeta(): pass\n"
                "@step\n"
                "def alfa(): pass\n",
            )
            discover([d], registry=reg)
        self.assertEqual(
            [s.qualified for s in reg.catalog()], ["varios/alfa", "varios/zeta"]
        )


class TestObjectStore(unittest.TestCase):
    """The slots an executor keeps, and the two duties no contract can check
    for it (ADR-0022 §7)."""

    def test_a_reference_names_a_slot_and_not_an_object(self):
        # Mutating what is in the slot does not change the handle: that is what
        # keeps retries working, since the engine re-sends the parameters it
        # evaluated once (ADR-0022 §5).
        store = ObjectStore()
        ref = store.new({"volts": 0})
        store.get(ref)["volts"] = 5
        self.assertEqual(store.get(ref)["volts"], 5)
        # And a by-value language replaces the box in the same slot.
        self.assertEqual(store.replace(ref, {"volts": 9}), ref)
        self.assertEqual(store.get(ref)["volts"], 9)

    def test_a_key_is_never_recycled_after_a_close(self):
        """The duty Anvil cannot check from outside.

        If a closed bench's key came back for the next `open_bench`, an old
        reference would resolve cleanly to a live, **different** object: same
        executor, same lifetime, everything green, measuring against the wrong
        bench (ADR-0022 §7).

        Seen failing by making `new` reuse the lowest free key: the second
        assertion passes, the store hands back `s1`, and the stale reference
        starts resolving.
        """
        store = ObjectStore()
        primero = store.new("banco-A")
        store.close(primero)
        segundo = store.new("banco-B")
        self.assertNotEqual(segundo.payload, primero.payload)
        with self.assertRaises(KeyError) as e:
            store.get(primero)
        self.assertIn("closed", str(e.exception))

    def test_a_reference_of_another_life_is_refused(self):
        """The executor knows this with certainty; Anvil only by comparison
        (ADR-0022 §6)."""
        store = ObjectStore(lifetime="v2")
        ajena = Reference(payload="s1", lifetime="v1")
        with self.assertRaises(KeyError) as e:
            store.get(ajena)
        self.assertIn("v1", str(e.exception))
        self.assertIn("v2", str(e.exception))

    def test_an_unknown_slot_is_refused_and_not_answered_plausibly(self):
        store = ObjectStore()
        inventada = Reference(payload="s99", lifetime=store.lifetime)
        with self.assertRaises(KeyError):
            store.get(inventada)

    def test_a_lifetime_is_never_the_same_twice(self):
        # The other duty (ADR-0022 §7): a process that came back on the same
        # lifetime would make its own restart undetectable, for Anvil and for
        # itself. Two stores standing in for two starts.
        self.assertNotEqual(ObjectStore().lifetime, ObjectStore().lifetime)


class TestReferenceParameters(unittest.TestCase):
    """A `Reference` annotation is described, and enforced, like any other."""

    def test_a_reference_parameter_is_described_as_one(self):
        reg = Registry()

        @step(registry=reg, outputs={"bench": Reference})
        def abrir(ctx, address: str) -> Result:
            return Result.passed(outputs={"bench": ctx.objects.new(address)})

        spec = reg.get(f"{HERE}/abrir").spec
        self.assertEqual([p.type for p in spec.inputs], [TEXT])
        self.assertEqual([(o.name, o.type) for o in spec.outputs], [("bench", REFERENCE)])

    def test_a_number_where_a_reference_goes_is_an_error_and_not_a_measurement(self):
        """The executor enforces its own catalog: a mismatch is `error`, never
        `fail` and never a default."""
        reg = Registry()

        @step(registry=reg)
        def medir(ctx, bench: Reference) -> Result:
            return Result.measured(1.0)

        r = reg.get(f"{HERE}/medir").invoke(Context(), {"bench": 4.2})
        self.assertEqual(r.status, "error")
        self.assertIsNone(r.measured_value)
        self.assertIn("reference", r.message)

    def test_a_step_carries_the_bench_across_invocations(self):
        """The whole pattern, end to end and without a wire: one step mints,
        another uses, a third closes."""
        reg = Registry()
        store = ObjectStore()
        ctx = Context(objects=store)

        @step(registry=reg, outputs={"bench": Reference})
        def abrir(ctx) -> Result:
            return Result.passed(outputs={"bench": ctx.objects.new({"volts": 0})})

        @step(registry=reg, outputs={"bench": Reference})
        def configurar(ctx, bench: Reference, volts: float) -> Result:
            ctx.objects.get(bench)["volts"] = volts
            return Result.passed(outputs={"bench": bench})

        @step(registry=reg)
        def leer(ctx, bench: Reference) -> Result:
            return Result.measured(ctx.objects.get(bench)["volts"])

        ref = reg.get(f"{HERE}/abrir").invoke(ctx, {}).outputs["bench"]
        vuelta = reg.get(f"{HERE}/configurar").invoke(ctx, {"bench": ref, "volts": 5.0}).outputs["bench"]
        self.assertEqual(vuelta, ref, "configurar cambia el banco, no cuál es el banco")
        self.assertEqual(reg.get(f"{HERE}/leer").invoke(ctx, {"bench": ref}).measured_value, 5.0)


class ModuleAddressing(unittest.TestCase):
    """A step is addressed `<module>/<step>` (ADR-0026).

    The module is the logical name of the `.py` it lives in, derived from the
    file and never declared, so that this executor can serve several modules
    the way the WASM one serves several components.
    """

    def write(self, dirname, name, body):
        p = Path(dirname) / name
        p.write_text(body, encoding="utf-8")
        return p

    SOURCE = (
        "from anvil_step import step\n"
        "@step\n"
        "def medir_voltaje():\n"
        "    '''Mide.'''\n"
        "    return 4.2\n"
    )

    def test_the_module_is_the_file_stem(self):
        reg = Registry()
        with tempfile.TemporaryDirectory() as d:
            self.write(d, "multimetro.py", self.SOURCE)
            discover([d], registry=reg)
        self.assertEqual([s.qualified for s in reg.catalog()], ["multimetro/medir_voltaje"])
        self.assertEqual(reg.catalog()[0].name, "medir_voltaje", "the local name survives")

    def test_the_same_step_name_in_two_modules_no_longer_collides(self):
        """What qualifying bought, and it used to refuse to start.

        Seen to fail by keeping the registry keyed on `spec.name`: the second
        module raises DiscoveryError, exactly as it did before this change.
        """
        reg = Registry()
        with tempfile.TemporaryDirectory() as d:
            self.write(d, "multimetro.py", self.SOURCE)
            self.write(d, "plc.py", self.SOURCE)
            discover([d], registry=reg)
        self.assertEqual(
            [s.qualified for s in reg.catalog()],
            ["multimetro/medir_voltaje", "plc/medir_voltaje"],
        )
        self.assertIsNotNone(reg.get("plc/medir_voltaje"))
        self.assertIsNotNone(reg.get("multimetro/medir_voltaje"))

    def test_a_bare_name_no_longer_resolves(self):
        """Serving several modules, guessing which one is meant would make a
        sequence's meaning depend on the folder's contents."""
        reg = Registry()
        with tempfile.TemporaryDirectory() as d:
            self.write(d, "multimetro.py", self.SOURCE)
            discover([d], registry=reg)
        self.assertIsNone(reg.get("medir_voltaje"))

    def test_a_bare_name_is_answered_with_the_qualified_one(self):
        """The mistake this change makes easy deserves the fix, not a list."""
        reg = Registry()
        with tempfile.TemporaryDirectory() as d:
            self.write(d, "multimetro.py", self.SOURCE)
            self.write(d, "plc.py", self.SOURCE)
            discover([d], registry=reg)
        self.assertEqual(
            reg.suggest("medir_voltaje"),
            ["multimetro/medir_voltaje", "plc/medir_voltaje"],
        )
        self.assertEqual(reg.suggest("plc/medir_voltaje"), [], "already qualified")

    def test_two_modules_with_the_same_logical_name_are_refused(self):
        """Two `instrument.py` under different steps paths would make a step
        name ambiguous, and serving the wrong one is worse than not starting.

        Seen to fail by dropping the check in `add_module`: discovery succeeds
        and the second module's steps silently answer for the first's.
        """
        reg = Registry()
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            self.write(a, "multimetro.py", self.SOURCE)
            self.write(b, "multimetro.py", self.SOURCE)
            with self.assertRaises(DiscoveryError) as e:
                discover([a, b], registry=reg)
        self.assertIn("two modules are both called 'multimetro'", str(e.exception))

    def test_a_module_clash_is_caught_before_importing_anything(self):
        """The clash is decided by the file name, so it must be found before
        anybody's module-level code runs — an import can touch an instrument.
        """
        reg = Registry()
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            self.write(a, "multimetro.py", self.SOURCE)
            # If this one is ever imported, it blows up loudly.
            self.write(b, "multimetro.py", "raise AssertionError('imported')\n")
            with self.assertRaises(DiscoveryError) as e:
                discover([a, b], registry=reg)
        self.assertIn("two modules are both called", str(e.exception))

    def test_a_package_takes_its_directory_name(self):
        """Nobody writes a sequence against a module called `__init__`."""
        reg = Registry()
        with tempfile.TemporaryDirectory() as d:
            pkg = Path(d) / "fuentes"
            pkg.mkdir()
            (pkg / "__init__.py").write_text(self.SOURCE, encoding="utf-8")
            discover([d], registry=reg)
        self.assertEqual([s.qualified for s in reg.catalog()], ["fuentes/medir_voltaje"])

    def test_each_module_carries_the_hash_of_its_file(self):
        """Which artifact answered is part of the answer (ADR-0025 §6)."""
        reg = Registry()
        with tempfile.TemporaryDirectory() as d:
            self.write(d, "multimetro.py", self.SOURCE)
            discover([d], registry=reg)
        info = reg.modules["multimetro"]
        self.assertEqual(len(info.sha256), 64)
        self.assertTrue(str(info.file).endswith("multimetro.py"))


if __name__ == "__main__":
    unittest.main()
