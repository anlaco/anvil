# ADR-0023: The bridge ships as a file next to `anvil`, not inside it

- **Estado:** Aceptada
- **Fecha:** 2026-08-28
- **Cómo se decidió:** the what and the scope came from direction in issue
  [#57](https://github.com/anlaco/anvil/issues/57) ("the WASM executor is a
  product, not plumbing"); the how was decided here, implementing and
  exercising it. Everything stated about the state of today is **verified by
  reading and running** this repo's code and is cited with file and line.
  The first half of the issue — the executor's placement in `executors/wasm`
  — was already implemented and is not re-litigated here.
- **Relaciona:** ADR-0011, ADR-0012, ADR-0013, ADR-0015, ADR-0020, issue #57,
  [diseno/executores-lenguaje.md](../diseno/executores-lenguaje.md),
  [guia-inicio-rapido.md](../guia-inicio-rapido.md)
- **Alcance:** decides how the bridge binary (`anvil-puente-wasm`) is
  distributed and found. It does **not** grow the WIT to the Python
  executor's features (catalog, ADR-0021; object references, ADR-0022), does
  not implement the remote/Raspberry deployment itself, does not decide
  where the user's `.wasm` lives in the remote case, does not rename the
  binary, does not touch `paso.proto`, and does **not** un-embed the two WASM
  guests — those stay inside the binary.

## Contexto

The engine's default language is WASM (ADR-0001), yet its executor is the
only one that cannot be obtained on its own. The Python executor is a
downloadable module (`executors/python/`, ADR-0012): a file that exists on
its own, that a human can copy and launch. The bridge lives inside the
`anvil` binary:

- `packaging/anvil-host/build.rs` copies it into `OUT_DIR` alongside the two
  guests;
- `packaging/anvil-host/src/main.rs:58` embeds it with `const PUENTE: &[u8]
  = include_bytes!(...)`;
- at startup, `extraer_puente()` (`main.rs:273`) writes it to a temp file
  keyed by a content hash, and `instanciar_wasm()` (`main.rs:305`) spawns it
  per declared `type: wasm`.

That embedding was ADR-0015 §3's deliberate choice to keep the hello world a
single file — and in the same ADR's §Contexto, the user confirmed the
companion decision: the bridge would also be "distribuible suelto para el
caso remoto (Raspberry Pi)". The first half shipped; the second never did.
The executable therefore has two lives — the one Anvil spawns and the one a
human would launch — and only the first is exercised. The second can break
silently for months, and the remote case has no artifact to copy at all.

Two facts make the change smaller than it looks:

1. **The contract handshake the issue asks for already exists.** `paso.proto`
   carries `contract` on `StepRequest` (`crates/modelo/paso.proto`) and the
   echo on `StepResult`; the engine refuses a mismatched echo with a message
   naming **both** numbers ("el ejecutor 'X' entiende el contrato N y este
   paso necesita el M") in `veredicto_del_eco` (`crates/motor/src/lib.rs`).
   This is ADR-0020 §4b, already implemented with tests that were seen red.
   An executor older than the binary fails at the first step, named. No new
   channel is needed.

2. The bridge already has a human-facing CLI: `--wasm <path> [--port <n>]
   [--bind <ip>]` (`executors/wasm/src/main.rs` doc header). Nothing to
   design there — only to keep working and, from now on, to exercise.

## Decisión

1. **The bridge stops being embedded.** `PUENTE` and `extraer_puente()` go
   away. No temp extraction: the human and the host run the same file.

2. **The host finds it next to itself.** The lookup is
   `current_exe()`'s directory + `anvil-puente-wasm`. One mechanism for
   development and distribution alike: `make release` leaves the pair
   together, the release tarball carries the pair, and the dev host does
   exactly what the published one will.

3. **The host's `build.rs` places it there.** It copies the compiled bridge
   from `executors/wasm/target/<profile>/` to the directory where cargo
   leaves the host binary — same profile preference and fallback-with-
   warning the guests already get. A missing bridge fails the host build
   with the recipe to run, exactly like a missing guest does.

4. **A missing file fails with a named path.** When a `type: wasm` executor
   has to come up and the file is not next to the `anvil` binary, the error
   names the path that was looked at and how to get it there (`make
   release`, or copy from `executors/wasm/target/`). It never says "your
   executor is old" — that wording belongs to the contract mismatch, which
   already names both versions.

5. **The second life gets exercised.** The bridge's own workspace tests
   launch its binary by hand (CLI parsing, the EOF exit, the
   not-a-component diagnostic) and the host's integration test runs the
   `demo_wasm` sequence both ways: with the file present and with it absent.

## Por qué esta forma (y lo que se descartó)

- **Keep embedding it and *also* ship the file.** Two copies of the same
  binary in one distribution is a desync waiting to happen: the embedded one
  is always current, the loose one can age unexercised — exactly the
  "breaks silently for months" failure the issue names. Issue #57 says
  "rather than inside it" on purpose.
- **A second lookup path as a dev fallback** (e.g. also searching
  `executors/wasm/target/`). Two search mechanisms means the published
  binary exercises a path the developer never runs. The pair-next-to-binary
  rule is the same on both sides of the release.
- **ADR-0012's discarded alternative** ("moving the WASM executor to
  `executors/` breaks zero-install") does not govern this, for three
  reasons: it spoke of `crates/ejecutor_pasos` — the default executor, which
  stays in `crates/` and stays embedded; that ADR is superseded by ADR-0013
  in its loading mechanism; and the issue #57 placement (first half) already
  moved the *bridge* into `executors/`. What is genuinely paid here is the
  single-file hello world: the tarball carries two files. It is accepted
  because the alternative — a bridge nobody can copy or launch — was the
  promise ADR-0015 made and never kept.

## Consecuencias

- `anvil` no longer runs alone if a sequence declares `type: wasm`: it needs
  the bridge file next to it, and says so with the path it looked at.
  Sequences without `type: wasm` executors never notice.
- The release changes shape: two binaries instead of one, built musl
  together. The packaging step (manual) puts both in the tarball;
  `verifica-release.yml` is the check that the pair actually works from a
  clean download.
- The temp-extraction path disappears — one less moving part and one less
  temp file per content version.
- ADR-0015 §3 (embed + extract to temp) is amended by this ADR; its CLI and
  `--bind` decision carries over untouched.
- The host's user-facing messages stay in Spanish for now: the beta
  regression suite (`docs/qa/regresion/run.sh`) and `exit_codes.rs` assert
  them verbatim, and changing surface in passing is this repo's idea of a
  broken change. Their translation is its own work.
- The `include_bytes!` of the two guests (`main.rs:56-57`) is untouched:
  they are core, not product (ADR-0011, ADR-0012).