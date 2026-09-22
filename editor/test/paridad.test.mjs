// The inventory's own guard (ADR-0043 §5).
//
// `app.mjs` claims the parity inventory "cannot go stale" because it lives in
// the editor rather than in a document nobody reopens. That was a comment, and
// a comment is not a property: nothing stopped a page being added without
// being declared, a `never` being written without a reason, or a debt saying
// only "not implemented" when the whole point is that "not yet, waiting on X",
// "we do it another way" and "not planned, because Y" are three different
// things to tell someone deciding whether to migrate.
//
// This file is what makes the claim true.

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { readFileSync } from "node:fs";

import {
  INSERT_MENU,
  PAGE_DETAILS,
  PROPERTY_PAGES,
  REQUIRED_FIELD,
  TESTSTAND,
  TYPE_LABELS,
  VERDICTS,
  gapTooltip,
  parityEntries,
} from "../src/paridad.mjs";
import { STEP_TYPES } from "../src/document.mjs";
import { OUT, render } from "../scripts/paridad-a-doc.mjs";

const app = readFileSync(new URL("../src/app.mjs", import.meta.url), "utf8");

test("every entry has a verdict, and it is one of the four", () => {
  for (const entry of parityEntries()) {
    assert.ok(entry.state, `${entry.id} has no verdict`);
    assert.ok(
      VERDICTS.includes(entry.state),
      `${entry.id}: "${entry.state}" is not one of ${VERDICTS.join(", ")}`,
    );
  }
});

test("ids are unique", () => {
  const seen = new Set();
  for (const entry of parityEntries()) {
    assert.ok(!seen.has(entry.id), `duplicate id: ${entry.id}`);
    seen.add(entry.id);
  }
});

test("each verdict carries exactly the field it owes", () => {
  for (const entry of parityEntries()) {
    if (entry.state === "built") {
      // What exists explains itself. Prose about TestStand left on a built
      // entry is prose that stopped being checked against the product.
      assert.equal(
        entry.teststand,
        undefined,
        `${entry.id} is built and still carries TestStand prose`,
      );
      for (const owed of Object.values(REQUIRED_FIELD)) {
        assert.equal(entry[owed], undefined, `${entry.id} is built and carries ${owed}`);
      }
      continue;
    }

    assert.ok(entry.teststand, `${entry.id} does not say what TestStand does there`);

    const owed = REQUIRED_FIELD[entry.state];
    assert.ok(
      typeof entry[owed] === "string" && entry[owed].length > 0,
      `${entry.id} is "${entry.state}" and owes a non-empty ${owed}`,
    );
    // Only the field its own verdict owes: a `never` that also carries `needs`
    // is a decision and a debt at once, which is the confusion this replaced.
    for (const [state, field] of Object.entries(REQUIRED_FIELD)) {
      if (state === entry.state) continue;
      assert.equal(
        entry[field],
        undefined,
        `${entry.id} is "${entry.state}" and also carries ${field}`,
      );
    }
  }
});

test("a debt names the engine capability it waits on", () => {
  // The rule that turns the greyed cells into an argument rather than a list:
  // four debts naming the same capability are a priority, and that only works
  // if every debt names one in the same words. "engine: nothing — …" is a
  // legitimate answer; it says the gap is a module someone writes, not a hole
  // in the engine, which is a different queue.
  for (const entry of parityEntries()) {
    if (entry.state !== "todo") continue;
    assert.ok(
      entry.needs.startsWith("engine: "),
      `${entry.id}: "needs" does not name an engine capability`,
    );
    assert.ok(
      entry.needs.length > "engine: ".length + 10,
      `${entry.id}: "needs" names nothing after "engine:"`,
    );
  }
});

test("every built page has a renderer, and every renderer a built page", () => {
  const built = PROPERTY_PAGES.filter((p) => p.state === "built");
  for (const page of built) {
    assert.ok(page.render, `${page.id} is built and names no renderer`);
    assert.match(
      app,
      new RegExp(`function ${page.render}\\b`),
      `${page.id} names ${page.render}, which app.mjs does not define`,
    );
    assert.match(
      app,
      new RegExp(`\\n  ${page.render},`),
      `${page.id} names ${page.render}, which is not wired into PAGE_RENDERERS`,
    );
  }
  for (const page of PROPERTY_PAGES) {
    if (page.state === "built") continue;
    assert.equal(page.render, undefined, `${page.id} is not built and names a renderer`);
  }

  // No renderer wired in that no page asks for: that is a page built and then
  // dropped from the list, which is how a cell silently disappears.
  const wired = app
    .slice(app.indexOf("const PAGE_RENDERERS = {"))
    .split("};")[0]
    .match(/^ {2}(page\w+),$/gm)
    .map((line) => line.trim().replace(",", ""));
  const named = new Set(built.map((p) => p.render));
  for (const fn of wired) {
    assert.ok(named.has(fn), `PAGE_RENDERERS wires ${fn}, which no page names`);
  }
});

test("the insert menu offers exactly the step types the document can make", () => {
  // AP-04: a type the loader does not know is a type the editor must not be
  // able to create. The menu is the one place that could get ahead of it.
  const offered = Object.keys(TYPE_LABELS).sort();
  assert.deepEqual(offered, [...STEP_TYPES].sort());
});

test("a menu entry is either a step type Anvil makes or a declared gap", () => {
  const walk = (entries) => {
    for (const entry of entries) {
      if (entry.sep) continue;
      assert.ok(entry.id, `a menu entry has no id: ${entry.label}`);
      if (entry.items) {
        walk(entry.items);
        // A heading that opens onto something needs no verdict of its own; one
        // that opens onto nothing is itself a gap and must say so.
        if (entry.items.length === 0) {
          assert.ok(entry.state, `${entry.id} opens onto nothing and declares no verdict`);
        }
        continue;
      }
      if (entry.type) {
        assert.equal(entry.state, "built", `${entry.id} makes a step and is not built`);
      } else {
        assert.ok(entry.state, `${entry.id} is not a step type and declares no verdict`);
        assert.notEqual(entry.state, "built", `${entry.id} is built and makes no step`);
      }
    }
  };
  walk(INSERT_MENU);
});

test("every in-page gap hangs under a page that exists and is built", () => {
  const pages = new Map(PROPERTY_PAGES.map((p) => [p.id, p]));
  for (const detail of PAGE_DETAILS) {
    const page = pages.get(detail.under);
    assert.ok(page, `${detail.id} hangs under ${detail.under}, which is not a page`);
    // A gap inside a page that is not built would be greyed twice over, and
    // nobody would ever see it.
    assert.equal(
      page.state,
      "built",
      `${detail.id} hangs under ${detail.under}, which is "${page.state}"`,
    );
    assert.ok(
      app.includes(`"${detail.id}"`),
      `${detail.id} is declared and never shown: app.mjs does not name it`,
    );
  }
});

test("app.mjs shows no gap it has not declared", () => {
  // The other direction of the one above: a `parityNote`, `parityRow` or
  // `parityById` naming an id that is not in the inventory throws at render
  // time, in front of someone using the editor. Here it fails in the test.
  const declared = new Set(parityEntries().map((e) => e.id));
  let checked = 0;
  for (const m of app.matchAll(/parity(?:Note|Row|ById)\(([^)]*)\)/g)) {
    for (const id of m[1].matchAll(/"([\w.-]+)"/g)) {
      checked += 1;
      assert.ok(declared.has(id[1]), `app.mjs shows "${id[1]}", which is not declared`);
    }
  }
  // A regex that silently stopped matching would make this test pass by
  // looking at nothing, which is the failure mode of every scan like it.
  assert.ok(checked >= 10, `only ${checked} ids were checked; the scan is not finding them`);
});

test("the execution toolbar and the greyed tabs are declared too", () => {
  // These are named in `app.mjs` as plain lists rather than at a call site, so
  // the scan above does not reach them.
  const declared = new Set(parityEntries().map((e) => e.id));
  const toolbar = app.slice(app.indexOf("const EXEC_TOOLBAR = ["));
  for (const m of toolbar.slice(0, toolbar.indexOf("];")).matchAll(/"([\w.-]+)"/g)) {
    assert.ok(declared.has(m[1]), `the execution toolbar shows "${m[1]}", which is not declared`);
  }
  const tabs = app.slice(app.indexOf("function markGreyedTabs("));
  for (const m of tabs.slice(0, tabs.indexOf("\n}")).matchAll(/"(execution\.[\w.-]+)"/g)) {
    assert.ok(declared.has(m[1]), `a greyed tab shows "${m[1]}", which is not declared`);
  }
});

test("a gap's tooltip says what TestStand does and where Anvil stands", () => {
  for (const entry of parityEntries()) {
    const tip = gapTooltip(entry);
    if (entry.state === "built") {
      assert.equal(tip, null, `${entry.id} is built and still has a gap tooltip`);
      continue;
    }
    assert.ok(tip.startsWith("TestStand: "), `${entry.id}: tooltip does not open with TestStand`);
    const anvil = tip.split("\n")[1];
    assert.ok(anvil?.startsWith("Anvil: "), `${entry.id}: tooltip does not say where Anvil stands`);
    // The three verdicts must not read alike. This is the whole taxonomy.
    const expected = {
      todo: "Anvil: not yet. Waiting on ",
      elsewhere: "Anvil: ",
      never: "Anvil: not planned. ",
    }[entry.state];
    assert.ok(anvil.startsWith(expected), `${entry.id}: tooltip does not read as "${entry.state}"`);
  }
});

test("a never names a decision written down somewhere", () => {
  // `never` is cheaper to write than `todo` and much harder to undo: it goes
  // into the published parity page. So it may not be invented while filling
  // the table in — it has to point at a document that already decided it.
  for (const entry of parityEntries()) {
    if (entry.state !== "never") continue;
    assert.match(
      entry.why,
      /ADR-\d{4}|roadmap\.md|vision\.md|requisitos\.md/,
      `${entry.id}: "never" cites no decision`,
    );
  }
});

test("the reference version of TestStand is pinned", () => {
  // Without it "1:1" names nothing and moves under us (ADR-0043 §2).
  //
  // Both shapes NI has used: a year (2019) and the quarterly naming it moved
  // to (2026Q3). Anything else is a placeholder someone left behind.
  assert.match(TESTSTAND, /^\d{4}(Q[1-4])?$/);
});

test("the published parity page matches the inventory", () => {
  // The same check CI runs with `--check`, here so it fails in a second rather
  // than in a pipeline. The page is the migration document someone reads
  // before deciding whether to try Anvil; a page that quietly stopped matching
  // the product is worse than no page.
  //
  // Line endings normalised away for the same reason `--check` does it: the
  // question is whether the page still says what the inventory says.
  assert.equal(
    readFileSync(OUT, "utf8").replace(/\r\n/g, "\n"),
    render(),
    "docs/paridad-teststand.md is stale — run: node editor/scripts/paridad-a-doc.mjs",
  );
});
