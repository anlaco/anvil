# ADR-0043: The editor is laid out as TestStand, and declares what it does not do

- **Status:** Accepted. **Not implemented** by this ADR.
- **Date:** 2026-09-20
- **How it was decided:** in this repo, in a conversation with the person who
  develops here, who writes test sequences in NI TestStand daily and has a
  licence at work. They proposed building the editor alongside TestStand's —
  everything the engine cannot do greyed out, and the engine developed to
  unlock it — and asked for the proposal to be argued against. Four questions
  were put to them and decided: **the scope** (the edit window *and* the
  execution/debug windows), **what a greyed cell says** (three verdicts, not
  one), **where the inventory lives** (a data file feeding both the interface
  and a generated page) and **who sets the pace** (the engine).
  Everything asserted here about **this repo** was verified by reading the
  code, cited with file and line. Everything about **TestStand** is
  **second-hand**: read from NI's online documentation and from twenty-eight
  screenshots of **TestStand 2026Q3** taken at that work machine
  (`Capturas_de_TS/`, git-ignored), not exercised in TestStand by the author of
  this text. The same warning ADR-0040 carries applies here unchanged.

  This ADR was first written against screenshots of **TestStand 2019**, and
  amended the same day, before it was ever committed, when the newer ones
  replaced them. Amended rather than superseded by a second ADR because no
  decision changed — only the evidence under it — and the text had not been
  published. What the re-reading changed in the inventory is listed in
  §Consequences h.
- **Relates to:** ADR-0011, ADR-0016, ADR-0019, ADR-0028, ADR-0029, ADR-0031,
  ADR-0033, ADR-0037, ADR-0040, ADR-0041,
  [principios-del-editor.md](../diseno/principios-del-editor.md),
  [ui-vs-headless.md](../diseno/ui-vs-headless.md),
  [roadmap.md](../roadmap.md)
- **Scope:** decides that the editor's **information architecture is
  TestStand's**, that a gap is **declared in place** with one of three
  verdicts, that the inventory of gaps lives in **one data file** which feeds
  both the greying and a generated documentation page, and that **the engine
  unlocks a cell, never the editor**. It pins the reference version to
  TestStand 2026Q3 and enumerates what is in and out of scope. It **supersedes**
  the guiding rule at the end of `roadmap.md` and the *Editor visual
  (post-MVP)* section of `ui-vs-headless.md`. It does **not** copy TestStand's
  semantics, object model, process model or callbacks; it does **not** change
  the YAML, `paso.proto`, the frozen report (RNF-08) or any SDK; it does
  **not** implement a single engine feature; and it does **not** decide when
  any greyed cell gets built.

## Context

### 1. The code already went there, and the documents say otherwise

`docs/roadmap.md` ends with the project's guiding rule:

> *«La regla rectora: **no replicar TestStand 1:1**; copiar lo bueno […] y
> dejar fuera lo frágil […].»*

and `docs/diseno/ui-vs-headless.md` declares the visual editor *post-MVP*,
*«Sin UI gráfica en v1»*, arguing that a UI coupled to the engine is *«el dolor
de TestStand»*.

Meanwhile commit `2a4a6c8` put TestStand's complete *Step Settings* page list
into `editor/src/app.mjs:475-520`, greying what Anvil has not built and
explaining in a tooltip what TestStand does there, with this reasoning in the
code:

> *«The parity inventory lives in the editor, where it cannot go stale, instead
> of in a document nobody reopens.»*

Both cannot be right. The documents describe a repository that no longer
exists.

### 2. "1:1" is two different claims, and only one of them is wanted

Copying **where an engineer looks for a setting** costs nothing and buys the
migration: the audience is people who use TestStand daily.

Copying **what the setting means** is a different promise. *Looping*,
*Post Actions*, *Switching*, *Synchronization*, *Requirements*,
*Additional Results* and *Property Browser* are not panes — each is a slice of
TestStand's object model (PropertyObject, its type system, NI Switch Executive,
Requirements Gateway). Some are precisely what `docs/vision.md` set out to
leave behind, and `roadmap.md` already lists TestStand's process model,
callbacks and LabVIEW/CVI integration as **out of scope**.

Showing the full list greyed, with one undifferentiated "not yet", quietly
commits Anvil to all of it.

### 3. One verdict is not enough, and the code already knew it

`app.mjs:511` defines a single message:

```js
const NOT_YET = "TestStand has this step type. Anvil does not, yet.";
```

but `app.mjs:807`, for *Property Browser*, already writes something of a
different kind by hand:

> *«In Anvil the raw form of a step is the YAML itself — use the Text view.»*

That is not a debt. It is a design answer. And *Switching* —*«Anvil has no
switching»*— is neither: it is a decision. Three kinds of statement are being
written into one field.

### 4. The vocabulary that governs the editor was never written down

`AP-02`, `AP-03`, `AP-04`, `AP-05`, `AP-07` and `AP-12` are cited **24 times**
across `editor/src/app.mjs`, `editor/src/document.mjs`, `editor/src/style.css`,
`editor/index.html`, `editor/README.md` and four files in `editor/test/`.
`grep` finds no definition anywhere in the repository.

AP-04 in particular — *the editor cannot build a sequence the loader refuses* —
is the load-bearing rule of the editor and the reason this ADR can state a
direction at all. A decision about the editor that leans on an undefined
principle is not a decision.

### 5. The inventory's own promise is unverified

*«…where it cannot go stale»* is true of the tooltips and false of everything
else: the inventory is prose inside a render function, it cannot be read
outside the editor, it cannot be published to someone deciding whether to
migrate, and nothing fails when a pane is added without declaring it.

## Decision

### 1. The information architecture is TestStand's; the semantics are not

The editor's panes, menus, page lists, orders and names are TestStand's.
What a thing **means** is Anvil's, and where the two differ the difference is
documented and kept: `comparison: none` yields `done` where TestStand yields
*Passed* (ADR-0040 §8), a `statement` yields `done` and not `pass`, and a
step's signature is **asked for** with `Describe` rather than inspected
(ADR-0028) because asking works the same for WASM, for Python and for a box in
another room.

This is AP-02, now written down.

### 2. The reference version is TestStand 2026Q3

Pinned, because without a version "1:1" names nothing and moves under us. It is
the version in `Capturas_de_TS/`, and it is the one on the licence this project
is built beside — which is the point: parity with a TestStand nobody in this
project can open is parity with a document, not with a product.

It is carried in **one place**, `TESTSTAND` in `editor/src/paridad.mjs`, so the
editor, the test and the published page cannot disagree about which TestStand
this is. When the reference moves again, it moves in a new ADR — this one was
amended in place only because it had not yet been committed.

### 3. Scope

**In:** the Sequence Editor's **edit window** (menu bar, toolbars, Insertion
Palette, Templates, the Sequences pane, the step list with its columns and its
Setup/Main/Cleanup groups, Step Settings, the Variables pane, the status bar)
and its **execution and debug windows** (an execution view, Call Stack, Watch,
Breakpoints, and the Step Settings / Output / Analysis Results tabs).

**Out:** Operator Interfaces (post-MVP, and a different product — RF-41), the
Type Editor, the Deployment Utility, Source Control integration, and the
Sequence Analyzer. Out of scope is a scope decision, not a gap: these are
`never` under §4 until an ADR says otherwise.

### 4. A gap is declared in place, with one of three verdicts

A thing TestStand has is never simply omitted from Anvil's interface. It
appears where TestStand puts it, greyed, saying what TestStand does there and
which of three situations Anvil is in:

| Verdict | Means | Must also carry |
|---|---|---|
| `todo` | Anvil intends to have this and does not yet. | **`needs`**: the engine capability that unlocks it. |
| `elsewhere` | Anvil solves this, differently. | **`anvil`**: how, and where to go. |
| `never` | Deliberately out of scope. | **`why`**: the reason, or the ADR that decided it. |

A fourth state, `built`, marks what exists, so the inventory is a complete
census rather than a list of holes.

`todo` **must name the engine capability**. A debt that cannot say what it is
waiting for is a wish, and four debts naming the same capability are a
priority — which is the main thing this mechanism is for.

### 5. The inventory is data, in one place, and a test keeps it honest

It lives in `editor/src/paridad.mjs`: a pure data module, no DOM and no
imports, in the same spirit as `run-state.mjs` being kept out of the DOM.
`app.mjs` reads it; render functions stay in `app.mjs`, keyed by id.

A documentation page, `docs/paridad-teststand.md`, is **generated** from it,
and a `--check` mode in CI fails when the committed page and the generator
disagree. The same pattern `docs/book/check.sh` already uses for the book.

A test asserts that every id the editor paints is declared, that every `built`
id has a render, and that each verdict carries exactly the fields it owes.
Without it, "cannot go stale" stays a comment rather than a property.

### 6. The engine unlocks the cell — never the editor

A capability is implemented and **verified headless** (CLI and test) and only
then does the editor stop greying it. Never the reverse.

This is AP-04 seen from the other side. If the editor could get ahead of the
engine, it could offer what the loader refuses; and it is what keeps
`editor/README.md`'s rule true — *«Anything that would require changing the
engine to suit the editor is a design mistake.»*

It also fixes the failure mode this ADR is most exposed to: a UI-led cadence
implements what is cheap to draw and defers what is expensive and matters.

### 7. `roadmap.md`'s guiding rule is replaced

It becomes: **the semantics are not replicated 1:1; the editor's information
architecture is, and it declares its gaps.** What the old rule protected — no
monolithic process model, no callbacks that break existing sequences — is
unchanged and is now carried by §3 and §4 `never`.

### 8. The AP principles are written down

In [`docs/diseno/principios-del-editor.md`](../diseno/principios-del-editor.md),
reconstructed from their citations. The unreferenced numbers are **reserved,
not renumbered**: AP-12 exists, so the original series had at least twelve, and
renumbering the six survivors would break every citation in the code to tidy a
list. This ADR's rule enters as **AP-13**.

## Consequences

a. **The editor becomes the migration document.** `docs/paridad-teststand.md`
   answers, for someone holding a TestStand licence, exactly what Anvil does
   and does not do — generated, so it cannot drift from the product.

b. **What is missing gets loud, starting with the worst of it.** Of the six
   debug controls in §3's execution window, four wait on one engine
   capability: stop, look, resume. And *Terminate* waits on cancellation with
   guaranteed `cleanup`, which `editor/README.md` already calls out and which
   is a risk **today**, with no parity involved: *«killing the thread
   mid-sequence leaves the bench exactly as it was, with no `cleanup` run.»*
   After this ADR that sentence has a cell on screen instead of a paragraph in
   a README.

c. **Some greys will be uncomfortable, and that is the point.** An editor whose
   execution window is mostly greyed is an honest editor. The alternative —
   omitting what we lack — is the false green of ADR-0019 Rule 2 applied to a
   product: silence read as "there is nothing to report".

d. **A `never` is a commitment.** It is cheaper to write than a `todo` and
   harder to undo, because it appears in the published parity page. A `never`
   that turns out to be wrong is changed by an ADR, like any other decision
   here.

e. **This does not make the editor lead.** §6 is what keeps the MVP's order
   intact; the engine's queue is still the engine's queue. What changes is that
   the queue is now visible in the place where its absence is felt.

f. **`comment` is the shape of what comes next.** It was added on the General
   page because TestStand has it there; it moved the minor because it is a new
   YAML field, and the engine never reads it. Most `todo` cells will not be
   that cheap, and §6 is what stops that difference from being discovered late.

g. **Not decided here:** the order in which any cell gets built, whether the
   parity page is published to the website, and how `AP-01`, `AP-06` and
   `AP-08`–`AP-11` are recovered.

h. **What moving the reference from 2019 to 2026Q3 changed.** Most of the
   inventory held: the same ten menus, the same eleven Properties pages in the
   same order, the same step types in the palette, the same fourteen Flow
   Control entries, the same step-list columns, the same five status-bar
   fields. What moved:

   - The step's tab strip **reversed**: `Properties | Data Source | Limits |
     Module` became **`Module | Limits | Data Source | Properties`**.
   - **Sequences left the sequence file window** and became a tab beside
     Variables, with columns Sequence, Comment and Requirement.
   - **DataLogger left the palette**; **IO Configuration** arrived, and
     **IO Configurations** joined Templates in the docked pane.
   - Variables gained a **Filter by name** box; a phase became **collapsible**,
     closed by `<End Group>`, and an empty one reads `<Insert Steps Here>`.
   - The palette gained an **adapter selector** at its top — which Anvil
     answers with `elsewhere`, because its adapter is gRPC and there is only
     one (ADR-0003); what varies is the executor, and that is a field on the
     step.
   - The Insert Step **separators are gone**: 2019 had a menu, 2026Q3 has a
     tree.

   The lesson is the one this ADR is built on. Every one of those was found by
   opening the new screenshots against the inventory, which took an afternoon
   because the inventory is a **list of claims in one file** rather than
   behaviour spread through a rendering function. Had it still been prose
   inside `app.mjs`, most of these would have been found by a user.

## References

- NI, *TestStand Help* — Step Settings, Sequence File Window, Execution Window,
  Insertion Palette: <https://www.ni.com/docs/en-US/bundle/teststand/>
- `Capturas_de_TS/` — twenty-eight screenshots of TestStand 2026Q3, kept
  locally and git-ignored (`.gitignore:47-51`).
- [`docs/investigacion/TestStand-y-competencia.md`](../investigacion/TestStand-y-competencia.md)
  — the single source for everything this project asserts about the competition.
