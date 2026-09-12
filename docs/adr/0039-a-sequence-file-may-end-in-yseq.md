# ADR-0039: A sequence file may end in `.yseq`, and stays YAML

- **Status:** Accepted
- **Date:** 2026-09-13
- **How it was decided:** in this repo, in a conversation with the person who
  develops here, who asked whether a sequence, being plain YAML, could carry an
  extension of its own "so it is easy to see and to run directly with anvil".
  Before deciding, both halves of today's behaviour were **run**, not read: a
  sequence renamed to `.yseq` executes, and a `sequence_call` naming a `.yseq`
  file without a slash fails. Every claim below about this repo is verified in
  that session and cited with file and line.
- **Relates to:** ADR-0002, ADR-0010, ADR-0037,
  [#67](https://github.com/anlaco/anvil/issues/67),
  [formato-de-secuencia.md](../diseno/formato-de-secuencia.md)
- **Scope:** decides that **`.yseq` is recognised as a sequence file wherever
  the extension is looked at** — the loader's path-or-name rule and the
  editor's file dialogs — **alongside** `.yaml` and `.yml`, and that a sequence
  the editor creates from nothing is offered as `.yseq`. It does **not** change
  the content: a `.yseq` file is YAML, parsed by the same loader with the same
  schema. It does **not** rename any existing sequence, in `ejemplos/`,
  `process_models/` or anywhere else, and it does **not** deprecate `.yaml` or
  `.yml`. It does **not** register the extension with any operating system:
  that is #67, which ADR-0037 leaves open, and this ADR only removes one reason
  it could not be done. It does **not** give the limits sidecar or the process
  model an extension of their own — both are named explicitly on the command
  line and neither looks at one. It does **not** decide a MIME type.

## Context

A sequence is data (ADR-0002) and the data is YAML. That was never in question
here, and is not now: YAML is what people review by hand, what a diff shows,
and what any editor highlights.

The question is what the **file** says about itself. `sequence.yaml` says "this
is YAML", which is true of a Kubernetes manifest, a CI workflow and a Compose
file. On a disk full of them, a sequence cannot be told apart without opening
it. And there is a second cost, less visible: #67 asks for a sequence to open
in the editor on double click, and ADR-0037 leaves that open. No application
can claim `.yaml` on someone's machine without stealing every other YAML file
from whatever opened it before. An extension of its own is the precondition
for ever doing it.

What the repo does with extensions today, verified:

1. **Running a sequence does not look at the extension.**
   `cargar_de_archivo` reads the file and parses the text
   (`crates/cargador/src/lib.rs:1261-1264`). A copy of a working sequence
   renamed to `hola.yseq` runs and passes, unchanged.
2. **Telling a path from a name does.** A `sequence_call` names either an
   inline subsequence or a file, and `es_path` decides which: a slash, or an
   ending in `.yaml`/`.yml`, means a file
   (`crates/cargador/src/lib.rs:1270-1273`, the convention of ADR-0010, also
   written in `docs/diseno/formato-de-secuencia.md:141-143`). So
   `sequence: hija.yseq` is read as the name of an inline subsequence and the
   load fails — *"referencia la subsecuencia inline 'hija.yseq' que no
   existe"* — while `sequence: ./hija.yseq` works. Both were run.
3. **The editor's dialogs do.** The browser picker accepts `.yaml` and `.yml`
   (`editor/src/app.mjs:692`), the fallback input the same
   (`editor/src/app.mjs:766`), and the Electron shell's open and save dialogs
   filter on the same two (`editor/electron/main.mjs:162` and `:171`). A
   `.yseq` file would not even be offered.
4. **Nothing else does.** The limits sidecar arrives by `--limits <path>`
   (`crates/motor/src/bin/anvil.rs:137`), the editor's `?open=` passes a path
   through without inspecting it (`editor/src/app.mjs:885-900`), and the host's
   only extension check is the one refusing a `.wasm` given where a sequence was
   expected (`packaging/anvil-host/src/main.rs:323`).

So the half that matters most already works, and the half that does not fails
loudly, at load, before any step runs. That is why this is cheap.

## Decision

### 1 — `.yseq` is a sequence file, in addition to `.yaml` and `.yml`

Wherever the extension decides something, `.yseq` counts the same as the other
two. In `es_path`, a destination ending in `.yseq` is a path. In every file
dialog, `.yseq` is offered.

**Additive, not a replacement.** Sequences in `.yaml` exist, in this repo and
out of it, and renaming them buys nothing; nor would refusing a `.yaml` a user
already has. Three accepted endings is a small price for not breaking anyone.

### 2 — The content does not change, and that is said out loud

A `.yseq` file is YAML, loaded by the same loader against the same schema. The
extension marks *what the file is for*, not a new format. Anyone who opens it
with a YAML-aware editor, linter or diff tool must keep getting what they got
with `.yaml` — at worst by telling their tool once that `*.yseq` is YAML.

A format that looked like YAML and was not would be worse than either: people
would read it as YAML and be wrong.

### 3 — A sequence the editor creates is offered as `.yseq`

When a document has no filename yet, the editor proposes `sequence.yseq` in its
save dialogs and download. A file that already has a name keeps it: opening
`basica.yaml` and saving it does not rename it.

This is the one place the new ending is *preferred* rather than merely
accepted, and it is deliberate: if nothing ever proposes it, the extension
exists only for those who already know about it, and the reason in §Context
never arrives.

## Alternatives rejected

**Leave it as is and tell people to write `./`.** Running already works, so
this would be free. But a `sequence_call` that fails for `hija.yseq` and works
for `hija.yaml` is a rule nobody would guess, and a file dialog that hides your
own sequences is not something a user works around — they conclude the editor
cannot open them.

**Make `.yseq` the only extension.** Breaks every existing sequence to gain a
tidier listing. In 0.x the surface may change between minors
(`CHANGELOG.md:7-9`), which makes it allowed, not free.

**Detect a sequence by its content instead of its name.** Reading every YAML
file to see whether it has `main:` would make the loader's path-or-name rule
depend on the filesystem, and still leave the operating system with no way to
associate the file with the editor, which is half the reason for this ADR.

**A different extension** (`.anvil`, `.seq`, `.aseq`). `.seq` is TestStand's own sequence
file (`docs/diseno/formato-de-secuencia.md:268`), the product Anvil competes
with, and borrowing it would invite exactly the confusion this ADR is for;
`.anvil` hides that the file
is YAML, which §2 wants visible. `.yseq` keeps both halves of what the file is
— **Y**AML, and a **seq**uence — in four letters. The choice was made by the
person who develops here, and nothing found contradicts it.

## Consequences

- `es_path` accepts `.yseq`, and a test in `crates/cargador` covers the
  `sequence_call` case that was run and failed.
- The editor's browser picker, fallback input and Electron dialogs offer
  `.yseq` first, then `.yaml` and `.yml`; a new document is proposed as
  `sequence.yseq`.
- `docs/diseno/formato-de-secuencia.md` states the rule with three endings.
- **No sequence in the repo is renamed.** `ejemplos/`, `process_models/` and
  the test fixtures stay `.yaml`; converting them is not needed and would make
  the history of every one of them harder to follow.
- **#67 stays open.** Registering `.yseq` with Linux, Windows or macOS is still
  to be decided and built; this ADR makes it possible, not done.
- A `sequence_call` whose destination ends in `.yseq` is now looked up as a
  file instead of as an inline subsequence. If some sequence named an inline
  subsequence `something.yseq`, it changes meaning and fails to load, loudly.
  **Not verified** that the loader forbids a dot in an inline subsequence's
  name; no sequence in this repo uses one.
