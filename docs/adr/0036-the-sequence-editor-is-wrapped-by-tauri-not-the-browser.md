# ADR-0036: The Sequence Editor is wrapped by Tauri, not the browser

- **Status:** Accepted. **Not implemented** by this ADR. `editor/` has no
  `src-tauri/` directory, no Tauri dependency in `editor/package.json`, and
  `packaging/package.sh` still produces a single Linux musl tarball
  (`packaging/package.sh:15`) — nothing Windows-shaped exists yet.
- **Date:** 2026-09-10
- **How it was decided:** by this repo, in a `/pensar` conversation with the
  person who develops here, followed by a plan. Everything asserted about
  **this repo** is verified by reading the code in this session and cited
  with file and line. The claim about Tauri's header configuration is
  **second-hand**, from web research, **not exercised in this repo yet** —
  marked where it appears.
- **Relates to:** ADR-0031, ADR-0034, ADR-0035 (annotates §i and its rejection
  of Tauri), [#67](https://github.com/anlaco/anvil/issues/67)
- **Scope:** decides **what wraps the Sequence Editor for distribution on
  Windows, and why it is not the browser's application mode that ADR-0035 §i
  chose**. It does **not** decide file-type registration or the
  single-instance problem ([#67](https://github.com/anlaco/anvil/issues/67),
  ADR-0035 §d) — Tauri makes that configurable, but it stays off here for the
  same reason ADR-0035 left it open: no design exists yet for "open this file
  in the engine that is already running." It does **not** change the engine,
  the protocol, or anything under `editor/src/engine*.mjs`. It does **not**
  touch the operator interface, which stays generated and served by the
  engine itself (ADR-0035 §b, §d, §g), never wrapped by Tauri.

## Context

> **Superseded in part by [ADR-0037](0037-the-editors-shell-is-electron-because-linux-is-the-first-platform.md) (2026-09-10):** §a, §b, §c and §e
> below are replaced — the shell is **Electron**, on every platform. The
> reason is not a flaw in the reasoning here but a fact this ADR did not
> have: the platform ordering is **Linux first, Windows second**, and §b's
> reliability argument (bundle the runtime so the user needs nothing
> installed) is Windows-only. On Linux Tauri does the opposite — the compiled
> shell links against the system's `libwebkit2gtk-4.1`, `libgtk-3`,
> `libsoup-3.0` and `libjavascriptcoregtk-4.1`. What survives: §d (the
> three-branch file access, with its native branch re-pointed) and §e's
> clause that the operator interface is not wrapped by anything.

ADR-0035 §i decided the Sequence Editor would be "wrapped by the browser, not
a framework" — the engine serves the page, the browser opens it in
application mode. That reasoning assumes a browser worth installing as an app
is already on the machine. It holds for a developer's own daily-driver
computer; it stops holding the moment someone else downloads the editor onto
a machine you do not control, which is exactly the distribution model this
project already promises for the engine itself: `README.md:25` says the
Linux binary needs "no Rust, no cargo, no glibc, nothing installed on the
system." A Windows build of the editor that silently requires a modern,
already-configured browser breaks that promise for the one artifact most
people will actually open first.

The engine's in-browser run mode has a hard technical dependency that any
wrapper has to preserve: it blocks on `Atomics.wait` over a
`SharedArrayBuffer` to give the WASM guest synchronous I/O
(`editor/src/engine-worker.mjs:1-9`, comment on why a worker thread exists at
all), and `SharedArrayBuffer` only exists in a cross-origin-isolated page —
which is why `editor/vite.config.mjs:38-52` already sends
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` in dev. Whatever wraps the
editor for Windows has to keep sending those two headers, or the engine
cannot run inside it at all.

## Decision

**a. The Windows distributable of the Sequence Editor wraps the existing SPA
in Tauri**, using WebView2 (Edge's rendering engine) rather than whatever
browser the target machine happens to have. Nothing under `editor/src/`
changes architecturally: it is the same SPA running the same
`jco`-transpiled engine in a worker (ADR-0031); Tauri supplies the window,
not a different engine host.

**b. The installer bundles WebView2's bootstrapper.** If the target machine
does not have a working WebView2 runtime, installing Anvil's editor installs
it — the person downloading this does not need to already have a capable
browser configured, which is the reliability gap that made ADR-0035 §i's
browser-application-mode insufficient for this distribution model.

**c. Cross-origin isolation is carried into the Tauri build.** Tauri 2
supports setting response headers, including COOP/COEP, through
`tauri.conf.json` for production builds (**second-hand, not yet exercised
here** — to be confirmed by actually running the built app and checking
`SharedArrayBuffer` exists, not assumed from documentation). The dev flow
(`tauri dev`, which wraps Vite) keeps getting them from the existing
`vite.config.mjs` headers unchanged.

> **Contradicted by [ADR-0037](0037-the-editors-shell-is-electron-because-linux-is-the-first-platform.md) (2026-09-10):** the premise §e correctly
> flags as unverified turned out to be **false on Linux**, and it was found
> by building this shell, not by reading more documentation. WebKitGTK ships
> `SharedArrayBuffer` disabled outright: the page was already
> `crossOriginIsolated: true` and `SharedArrayBuffer` was still `undefined`
> until `JSC_useSharedArrayBuffer=1` was set on the process before the
> webview started (`editor/src-tauri/src/main.rs:126-135`). Cross-origin
> isolation alone is sufficient on Chromium, which is what ADR-0037 §b
> relies on; on WebKitGTK it is not, and the workaround is an unstable
> JavaScriptCore option applied to a library the user supplies.

**d. Where the SPA depends on a browser-only capability not guaranteed
inside a webview — the File System Access API used for opening and saving a
sequence (`editor/src/app.mjs:675-681,716-720`) — a Tauri-native path is
added as a third branch, selected by feature-detecting `window.__TAURI__`,
alongside the two that already exist (File System Access, and the download
fallback for browsers without it). This is additive: neither existing branch
is removed, since the same `editor/dist/` build still has to run standalone
in a plain browser for the demo and for Linux/macOS use before this ADR's
Windows-specific packaging exists for those platforms too.

**e. The operator interface is not affected.** It continues to be generated
and served by the engine's own binary (ADR-0035 §b, §d, §g); this decision
concerns only how the Sequence Editor — the developer's tool, never installed
on a bench — is packaged.

## Alternatives rejected

- **Electron.** Same weight and Chromium-patch-maintenance argument ADR-0035
  already made against it, unchanged by anything in this ADR. Tauri answers
  the reliability gap that motivated revisiting this decision at a fraction
  of Electron's per-platform size, without taking on an independent browser
  security-patch cadence.

  > **Reversed by [ADR-0037](0037-the-editors-shell-is-electron-because-linux-is-the-first-platform.md) (2026-09-10):** both halves of this still
  > hold — Electron is heavier and does take on the Chromium-patch cadence,
  > and ADR-0037 accepts that cost explicitly in its Consequences §b. What
  > changed is the other side of the scale: under Linux-first, Tauri does
  > **not** answer the reliability gap at all, because on Linux it ships no
  > runtime to answer it with.
- **Keeping ADR-0035 §i's browser-application-mode.** Rejected specifically
  because it assumes a capable, already-installed, up-to-date browser on a
  machine this project does not control — true of a developer's own
  computer, not true of "someone downloaded the editor like they would
  FreeCAD." The reasoning that made Tauri lose to browser-application-mode in
  ADR-0035 (an artefact to build, sign and update per platform, three
  webviews to support) still applies and is accepted as a real, ongoing cost
  — it is outweighed here by the reliability requirement.

## Consequences

**a. ADR-0035 §i, and its "Tauri" line under Alternatives rejected, are
annotated with a note pointing here.**

**b. Packaging gains a second, independent Windows artifact.** The engine's
own binary (`anvil.exe`/`anvil-exec-wasm.exe`, packaged by
`packaging/package.ps1`, mirroring `packaging/package.sh`) and the editor's
Tauri installer are two separate downloads with two separate purposes — one
runs a sequence headless (CI, a bench, scripting), the other is the
development tool. Documentation has to keep them visibly distinct so nobody
installs the wrong one expecting the other's behavior.

**c. File-type registration and the single-instance problem stay open.**
Tauri exposes file-association configuration, but turning it on requires the
same unresolved design ADR-0035 §d already flagged — opening a second
sequence must reach the engine that is already running, not start a second
one — so it is not enabled by this ADR.

**d. A new, smaller maintenance surface is accepted.** This repo now owns
tracking Tauri's own release cadence and verifying WebView2 bootstrap
behavior on real Windows machines — lighter than Electron's Chromium-patch
treadmill, but not zero, and it did not exist before this decision.

**e. The cross-origin-isolation claim in Decision §c is unverified until
exercised.** The CI job that builds and smoke-tests the Tauri app on a
`windows-latest` runner is what is supposed to close this gap; until it runs
and confirms `SharedArrayBuffer` is actually available inside the packaged
app, this ADR's technical premise rests on documentation, not on having run
the code — which is exactly the distinction this repo's own rules ask not to
blur.

> **Closed by [ADR-0037](0037-the-editors-shell-is-electron-because-linux-is-the-first-platform.md) (2026-09-10):** the gap §e names was closed, in
> the opposite direction from the one expected. It was not a `windows-latest`
> CI job that closed it but building the shell on Linux, where the premise
> fails. ADR-0037 §b inherits the same obligation — it must be confirmed by
> running the packaged app and reading `crossOriginIsolated` and
> `typeof SharedArrayBuffer` inside it, not by trusting Chromium's
> documentation — and it is not to be treated as settled until that is
> recorded there.
