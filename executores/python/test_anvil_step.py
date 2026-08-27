"""Tests for the Python executor's step SDK (issues #54 and #45).

They use `unittest` from the standard library and **do not import grpc**: the
SDK is the authoring surface, and it has to be testable —by us and by whoever
writes a step— without a gRPC stack, a bench, or a running executor.

    python3 -m unittest discover executores/python
"""

import tempfile
import unittest
from pathlib import Path

from anvil_step import (
    BOOLEAN,
    NUMBER,
    TEXT,
    UNSPECIFIED,
    Context,
    DiscoveryError,
    Registry,
    Result,
    discover,
    step,
)


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

        spec = self.reg.get("medir").spec
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

        self.assertEqual(self.reg.get("medir").spec.inputs[0].type, UNSPECIFIED)

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

        self.paso = self.reg.get("medir")
        self.laxo = self.reg.get("etiquetar")

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

        r = self.reg.get("revienta").invoke(Context(), {})
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

        s = self.reg.get("transitorio")
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
        self.assertEqual([s.name for s in reg.catalog()], ["apretar"])

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
        self.assertEqual([s.name for s in reg.catalog()], ["alfa", "zeta"])


if __name__ == "__main__":
    unittest.main()
