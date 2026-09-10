# ADR-0037: The editor's shell is Electron, because Linux is the first platform

- **Status:** Accepted. Supersedes ADR-0036 §a, §b, §c and §e.
- **Date:** 2026-09-10
- **How it was decided:** by this repo, in a conversation with the person who
  develops here, who stated the platform priority this ADR turns on: **Linux
  first, Windows second**. ADR-0036 was written one week earlier and did not
  have that ordering, nor a compiled shell to inspect. Everything asserted
  about this repo is verified in this session by running the commands quoted
  below or by reading the code, and cited with file and line. The claims
  about which WebKitGTK version specific distributions ship are
  **second-hand** and marked where they appear.
- **Relates to:** ADR-0030, ADR-0031, ADR-0034, ADR-0035 (§i), ADR-0036
  (annotates and supersedes), [#67](https://github.com/anlaco/anvil/issues/67)
- **Scope:** decides **which shell wraps the Sequence Editor for
  distribution, on every platform**, and replaces ADR-0036's answer with a
  different one for a reason ADR-0036 could not weigh. It does **not** change
  the engine, the protocol, the bridge, or anything under
  `editor/src/engine*.mjs` — the SPA and its `jco`-transpiled guest are
  untouched by this decision, exactly as ADR-0036 §a already promised of
  Tauri. It does **not** revisit ADR-0035 §b/§d/§g: the operator interface
  stays generated and served by the engine, never wrapped. It does **not**
  decide file-type registration or the single-instance problem
  ([#67](https://github.com/anlaco/anvil/issues/67)), which stay open on the
  same grounds ADR-0036 §c left them open. It does **not** claim Electron is
  the better framework in general — only that it is the right one given the
  platform ordering.

## Context

ADR-0036 chose Tauri and justified it with a reliability promise: §b bundles
the WebView2 bootstrapper so that "the person downloading this does not need
to already have a capable browser configured." That argument is sound. It is
also **specific to Windows**, and ADR-0036 was written for a Windows-shaped
distribution — its `Scope` says so, its bundle targets NSIS, and its Windows
sibling `packaging/package.ps1` describes the editor installer as the
Windows artifact.

The platform ordering is the reverse. **Linux is the first platform Anvil
supports; Windows is the second.** On Linux, Tauri does not carry a runtime —
it borrows the system's. The shell that now exists in the tree makes this
checkable rather than arguable:

```
$ ldd editor/src-tauri/target/debug/anvil-editor | grep -Ei 'webkit|javascriptcore|gtk|soup'
    libwebkit2gtk-4.1.so.0      => /lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0
    libgtk-3.so.0               => /lib/x86_64-linux-gnu/libgtk-3.so.0
    libsoup-3.0.so.0            => /lib/x86_64-linux-gnu/libsoup-3.0.so.0
    libjavascriptcoregtk-4.1.so.0 => /lib/x86_64-linux-gnu/libjavascriptcoregtk-4.1.so.0
```

Four system libraries, dynamically linked, resolved from the host. This
machine has `webkit2gtk-4.1` 2.52.3; a machine that has `webkit2gtk-4.0`, or
no `libsoup-3.0`, does not start this binary at all, and nothing in the
package can fix that from the user's side.

That directly contradicts what this project promises of its other
downloadable artifacts. `README.md:20-26` says of `anvil` and
`anvil-exec-wasm`: "they need no Rust, no cargo, no glibc, **nothing
installed on the system**." On Windows, Tauri honours that promise by
shipping a bootstrapper. On Linux — the first platform — it inverts it: the
shell becomes the one artifact in the package with a hard, versioned system
dependency. **The reliability gap ADR-0036 was written to close is reopened
by ADR-0036 on the platform that matters most.**

Two further facts, both found by building and running the Tauri shell rather
than by reading documentation, sharpen this:

**WebKitGTK ships `SharedArrayBuffer` disabled.** The engine cannot run
without it — it blocks on `Atomics.wait` over shared memory to give the WASM
guest synchronous I/O (ADR-0030, ADR-0031). Cross-origin isolation is not
enough on WebKitGTK: `editor/src-tauri/src/main.rs:126-135` records that the
page was already `crossOriginIsolated: true` and `SharedArrayBuffer` was
*still* undefined until `JSC_useSharedArrayBuffer=1` was set on the process
before the webview started. That workaround is an unstable JavaScriptCore
option, set through an environment variable, on a library the user supplies.
ADR-0036 §e flagged cross-origin isolation as its unverified premise and
expected a Windows CI job to close it; the premise turned out to be false on
Linux, and the fix is outside the artifact's control.

**The Linux webview cannot be inspected from outside itself.**
`editor/src-tauri/src/main.rs:110-118` exists only because of this: a
`log_line` command and a `console` patched in `editor/src/app.mjs:27-45` to
forward through Tauri's IPC, because WebKitGTK exposes no remote inspector
anything can attach to. The comment records the cost in plain terms — three
separate faults hid behind "it does not work" while the shell was built.
Under the ordering in this ADR, that is not a development inconvenience on a
secondary platform; it is the primary platform's production shell being
opaque to every tool this project uses.

## Decision

**a. The Sequence Editor is wrapped by Electron on every platform.** Electron
carries its own Chromium, so the shell resolves to one engine, one version,
chosen by this repo, on Linux and Windows alike. `editor/src/` is unchanged
architecturally: the same SPA, the same `jco`-transpiled engine in a worker
(ADR-0031). This replaces ADR-0036 §a and §b.

**b. Cross-origin isolation is served, not begged for.** The packaged app
serves `editor/dist/` over a registered privileged scheme whose responses
carry `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` — the same two headers
`editor/vite.config.mjs:38-52` already sends in dev. `file://` is not used,
because it yields no origin capable of cross-origin isolation. On Chromium
those two headers are sufficient for `SharedArrayBuffer`, with no
engine-internal flag to set. **Confirmed by running it**, on Linux, on
2026-09-10: serving `editor/dist/` over the `anvil://` scheme, the page
reports `{"url":"anvil://bundle/","crossOriginIsolated":true,
"sharedArrayBuffer":"function"}` and the engine guest validates
`ejemplos/basica.yaml` inside it — with no engine-internal flag set, which is
the whole difference from WebKitGTK. Electron 40.10.6 / Chromium 144. This is
the verification ADR-0036 §e asked for and did not get. It is **not** yet
confirmed on Windows; the `ci-windows` job asserts the same two values
against the packaged build there.

**c. The renderer gets no Node.** `contextIsolation: true`,
`nodeIntegration: false`, `sandbox: true`, and a preload script that exposes
one narrow object (`window.anvil`) through `contextBridge`: open and save
dialogs, read and write a text file, start the local bridge, nothing else.
This is deliberate: it keeps the property that made Tauri attractive on
security grounds — the page cannot reach the filesystem or spawn a process
except through calls this repo wrote and can enumerate — and it is the
replacement for `editor/src-tauri/capabilities/default.json`.

**d. The three-branch file access of ADR-0036 §d is kept, with the native
branch re-pointed.** `editor/src/app.mjs` selects on `window.anvil` where it
selected on `window.__TAURI_INTERNALS__` (`app.mjs:729`); the File System
Access branch and the download fallback are untouched, because the same
`editor/dist/` still has to run standalone in a plain browser.

**e. The console forwarding in `app.mjs:27-45` is deleted, not ported.**
Electron's main process receives the renderer's console through
`webContents.on("console-message")`, and Chromium's remote debugging port
exposes the renderer to any CDP client. The shell needs no help from the page
to be observable, so the page stops carrying shell-specific code.

## Alternatives rejected

- **Keeping Tauri and accepting the Linux system dependency.** This is what
  ADR-0036 decides, and it is defensible under a Windows-first ordering. It
  fails under this one: a first-platform artifact that requires a specific
  `libwebkit2gtk` version, cannot guarantee the engine runs (`SharedArrayBuffer`),
  and cannot be inspected, is worse than the browser-application-mode ADR-0035
  §i chose — which at least failed visibly, on a machine the user could fix.
- **Keeping Tauri on Windows and using Electron on Linux.** Two shells, two
  sets of platform bugs, two packaging pipelines, and every frontend
  capability question asked twice. It preserves the smaller Windows installer
  and nothing else. The point of this decision is to have **one** webview
  whose behaviour is known.
- **Returning to ADR-0035 §i (browser application mode).** Rejected for the
  reason ADR-0036 gave, which this ADR does not dispute: it assumes a
  capable, configured, up-to-date browser on a machine this project does not
  control.
- **Bundling WebKitGTK with the Tauri build (AppImage, Flatpak).** Technically
  possible and it would close the system-dependency hole. It does not close
  the other two — `SharedArrayBuffer` still needs an unstable JSC flag, and
  the webview is still uninspectable — and it re-introduces exactly the
  per-platform runtime weight that was Tauri's argument over Electron, while
  keeping a webview this repo has no influence over.

## Consequences

**a. ADR-0036 §a, §b, §c and §e are superseded, and ADR-0036 is annotated
with a note pointing here.** ADR-0036 §d and §e-the-operator-interface clause
survive: the three-branch file access is kept (§d above), and the operator
interface is still not wrapped by anything.

**b. This repo takes on Chromium's security-patch cadence.** This is the real
price, it is permanent, and it did not exist under ADR-0036 §d's "smaller
maintenance surface." When Chromium publishes a security release that matters,
the editor needs a new build and a new release — not because Anvil changed,
but because its shell did. A project that does not publish on that cadence
ships a known-vulnerable browser to its users. Accepting this ADR is
accepting that obligation.

**c. Every download gets bigger, and the Windows installer loses its
advantage.** ADR-0036 §b's WebView2 bootstrapper produced an installer of a
few megabytes; an Electron one carries Chromium per platform. This was
weighed and explicitly set aside: the person deciding asked which shell is
safer across platforms **setting weight aside**, and answered the platform
ordering question that follows from it.

**d. `editor/src-tauri/` is removed, together with the workarounds it
existed to hold.** `JSC_useSharedArrayBuffer`, the `log_line` command, the
`capabilities/default.json` permission set and the Rust build in the editor
tree all go. `start_bridge` is ported, **including its stopgap**: it still
locates `anvil` by a dev-tree-relative path and still breaks when packaged
([#67](https://github.com/anlaco/anvil/issues/67)). Changing shells does not
fix that, and this ADR does not pretend it does.

**e. The editor becomes drivable by the tools this project already has.**
Playwright speaks to Electron directly, and Chromium's remote debugging port
is reachable on both platforms. This was the observation that started the
conversation, and it is a consequence rather than the justification: it does
not, on its own, warrant replacing a shell.

**f. `packaging/package.ps1`'s and `packaging/package.sh`'s references to a
Tauri installer are wrong until updated**, as are any in `README.md`,
`CHANGELOG.md` and `docs/`. They are corrected in the same change as the code,
per this repo's rule that `docs/` is fixed in the turn the binary contradicts it.
