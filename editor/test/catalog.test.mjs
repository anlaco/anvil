// What the editor does with a catalog (ADR-0044).
//
// The Module tab used to read a parameter's type off the literal in the YAML
// with `typeof`, which says what someone typed and not what the step takes: a
// `canal` written `"1"` looked like a text parameter because it was written as
// one. Now the rows are what the executor declares.
//
// The functions under test are the pure half — the ones that decide **which
// rows exist and what state each is in**. That is where a wrong answer is
// expensive: a required input silently missing is a step that will not run,
// and an input the executor does not declare is one it would drop while
// measuring something else. The engine calls the second a finding and never a
// warning (`EntradaDesconocida` in crates/motor/src/catalogo.rs), and the
// editor must not be gentler about it.

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { readFileSync } from "node:fs";

const app = readFileSync(new URL("../src/app.mjs", import.meta.url), "utf8");

/**
 * `filasDeParametros` and its two helpers are not exported: `app.mjs` is the
 * page, and it reaches for the DOM the moment it is imported. The function is
 * pure, so it is lifted out by source and evaluated — which also fails loudly
 * if it is ever renamed or moved, rather than silently testing nothing.
 */
function lift(name) {
  const at = app.indexOf(`function ${name}(`);
  assert.notEqual(at, -1, `app.mjs no longer defines ${name}`);
  const end = app.indexOf("\n}\n", at) + 2;
  return app.slice(at, end);
}

const filasDeParametros = new Function(
  `${lift("filasDeParametros")}; return filasDeParametros;`,
)();

/** A `StepSpec` shaped as `anvil describe` prints it. */
const spec = (inputs) => ({ name: "multimetro/medir", doc: "", inputs, outputs: [] });
const param = (name, extra = {}) => ({
  name,
  type: "number",
  required: false,
  doc: "",
  has_default: false,
  default: null,
  ...extra,
});

test("the rows are what the executor declares, in its own order", () => {
  // The executor's order is the order of the function's signature — the one
  // whoever wrote the step chose. The YAML's key order is an accident of
  // typing.
  const filas = filasDeParametros(spec([param("canal"), param("offset")]), { offset: 2 });
  assert.deepEqual(
    filas.map((f) => f.name),
    ["canal", "offset"],
  );
  assert.equal(filas.every((f) => f.estado === "declared"), true);
});

test("a declared input with no value is shown, not dropped", () => {
  // The whole point of drawing from the catalog: a parameter nobody has filled
  // in still has a row, which is how someone discovers it exists.
  const [canal] = filasDeParametros(spec([param("canal")]), {});
  assert.equal(canal.tiene, false);
  assert.equal(canal.value, undefined);
});

test("the type comes from the executor, not from what was typed", () => {
  // `"1"` in the YAML is a string. The executor says `canal` is a number, and
  // the executor is the one that will read it.
  const [canal] = filasDeParametros(spec([param("canal", { type: "number" })]), { canal: "1" });
  assert.equal(canal.type, "number");
  assert.notEqual(canal.type, typeof "1");
});

test("an input the executor does not declare is marked, not hidden", () => {
  // It would be dropped and the step would measure something else. Hiding it
  // would leave someone reading a sequence that does not do what it says.
  const filas = filasDeParametros(spec([param("canal")]), { canal: 1, canall: 2 });
  const sobra = filas.find((f) => f.name === "canall");
  assert.ok(sobra, "the undeclared input has a row");
  assert.equal(sobra.estado, "not-served");
  // And it comes after the declared ones: the signature first, the surprises
  // after it.
  assert.equal(filas.at(-1).name, "canall");
});

test("required and optional stay distinguishable", () => {
  const filas = filasDeParametros(
    spec([param("canal", { required: true }), param("offset", { required: false })]),
    {},
  );
  assert.equal(filas[0].required, true);
  assert.equal(filas[1].required, false);
});

test("a declared default is carried, and 'no default' is not the same as null", () => {
  // ADR-0021 §5: informative, and the step applies it — the engine sends
  // nothing. The editor shows it as a placeholder, so it has to know which
  // parameters have one.
  const filas = filasDeParametros(
    spec([
      param("a", { has_default: true, default: 0 }),
      param("b", { has_default: false, default: null }),
    ]),
    {},
  );
  assert.equal(filas[0].hasDefault, true);
  assert.equal(filas[0].default, 0);
  assert.equal(filas[1].hasDefault, false);
});

test("with no catalog it falls back to the YAML and says so", () => {
  // Showing nothing would be worse: a step that has inputs would look like one
  // that has none. The state is what stops the fallback being mistaken for an
  // answer — `unknown-catalog` is not editable and not marked.
  const filas = filasDeParametros(null, { canal: 1 });
  assert.equal(filas.length, 1);
  assert.equal(filas[0].estado, "unknown-catalog");
  assert.equal(filas[0].type, "number", "falls back to typeof, which is all it has");
});

test("a module that takes nothing is not the same as a module nobody asked about", () => {
  // Two empty tables that mean opposite things: "this takes no inputs" is a
  // fact, "nobody has been asked" is a question. The states keep them apart so
  // the panel can say different words (ADR-0019, Rule 2).
  assert.deepEqual(filasDeParametros(spec([]), {}), []);
  assert.deepEqual(filasDeParametros(null, {}), []);
});

// ---------------------------------------------------------------------------
// The seam between the two halves.
//
// `fixtures-describe.json` is the **engine's real output**, captured from
//
//     anvil describe ejemplos/demo_departamento.yseq --quiet
//
// trimmed to two steps. The tests above prove the editor does the right thing
// with a catalog; this one proves it is reading the catalog the engine
// actually prints. They are written in two languages, in two crates, by two
// people at different times, and a key renamed on either side — `has_default`,
// `required`, `type` — would break nothing that either half's own tests can
// see. This is where it breaks instead.
// ---------------------------------------------------------------------------

const REAL = JSON.parse(
  readFileSync(new URL("./fixtures-describe.json", import.meta.url), "utf8"),
);

const catalogOf = new Function(
  "state",
  `${lift("catalogOf")}; return catalogOf;`,
);
const specOf = new Function(
  "state",
  `${lift("catalogOf")}; ${lift("specOf")}; return specOf;`,
);

test("the editor reads the document the engine really prints", () => {
  const state = { catalog: REAL };
  const cat = catalogOf(state)("instrumentos");
  assert.ok(cat, "the executor is found and describes itself");
  assert.equal(
    cat.steps.length,
    2,
    "steps is where the editor looks for them",
  );

  const spec = specOf(state)("instrumentos", "multimetro/medir_voltaje");
  assert.ok(spec, "a module is found by the qualified name a sequence writes");
  assert.equal(spec.doc, "Measures DC voltage on a channel.");
  assert.deepEqual(
    spec.outputs.map((o) => o.name),
    ["canal_usado"],
    "outputs are what the connector pane draws on the right",
  );

  // And through the table, which is the whole point: the row exists, typed,
  // with nothing written in the YAML for it.
  const filas = filasDeParametros(spec, {});
  assert.deepEqual(filas.map((f) => [f.name, f.type, f.required, f.hasDefault]), [
    ["canal", "number", false, false],
  ]);
});

test("an executor's own step is addressed by its qualified name", () => {
  // `multimetro/medir_voltaje` and `demo/measure_voltage` are two modules of
  // one department (ADR-0025 §2). The prefix is part of the name a sequence
  // writes, so looking a module up without it must not find one.
  const state = { catalog: REAL };
  assert.ok(specOf(state)("instrumentos", "demo/measure_voltage"));
  assert.equal(specOf(state)("instrumentos", "medir_voltaje"), null);
});

// ---------------------------------------------------------------------------
// The Output tab, and what does not belong on it.
//
// Found by running the editor against a real Python executor rather than by
// reading it: the tab was most NDJSON. `--events` writes one JSON object per
// line to stderr, and stderr is what the tab shows — so the event stream
// buried the lines someone actually needs, a warning from an executor or a
// connection that was retried.
// ---------------------------------------------------------------------------

const sinEventos = new Function(`${lift("sinEventos")}; return sinEventos;`)();

test("the event stream is taken off the Output tab, and nothing else is", () => {
  const stderr = [
    "secuencia 'prueba_fuente' cargada (2 pasos en main)",
    '{"event":"sequence_start","sequence":"prueba_fuente","seq":0}',
    "connected to the step executors (banco)",
    '{"event":"step_start","name":"autotest","seq":1}',
    "aviso: el ejecutor 'banco' no publica una vida en su catálogo",
  ].join("\n");

  assert.deepEqual(sinEventos(stderr).split("\n"), [
    "secuencia 'prueba_fuente' cargada (2 pasos en main)",
    "connected to the step executors (banco)",
    "aviso: el ejecutor 'banco' no publica una vida en su catálogo",
  ]);
});

test("a line that only looks like JSON stays", () => {
  // fd 2 is shared with the executors, and this filter must not eat what one
  // of them printed. The test is `applyEvent`'s, inverted: an object with an
  // `event` is an event, and everything else is someone talking.
  const stderr = [
    '{"nivel":"warn","texto":"el instrumento tardó 3 s"}',
    "{ esto no es json",
    '{"event":"step_end","seq":2}',
  ].join("\n");

  assert.deepEqual(sinEventos(stderr).split("\n"), [
    '{"nivel":"warn","texto":"el instrumento tardó 3 s"}',
    "{ esto no es json",
  ]);
});
