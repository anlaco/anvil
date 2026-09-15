// The window shell for the Sequence Editor (ADR-0037). It hosts
// `editor/dist/`, the exact SPA that already runs standalone in a plain
// browser — this process supplies a window, the dialog and file calls
// `editor/src/app.mjs` reaches for through `window.anvil`, and
// `startBridge` below. The engine, the protocol and the bridge relay are
// unchanged: they still run the way ADR-0031 and ADR-0030 describe — this
// just launches the same `anvil --bridge` a person would otherwise have to
// start themselves in a second terminal.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createInterface } from "node:readline";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { app, BrowserWindow, dialog, ipcMain, Menu, protocol, shell } from "electron";
import { writeFile } from "node:fs/promises";
import updater from "electron-updater";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "../..");
const DIST = path.join(HERE, "..", "dist");
const DEV_URL = "http://localhost:5180";
// `ANVIL_EDITOR_SERVE_DIST=1` takes the packaged path — `editor/dist/` over
// the `anvil://` scheme — without building an installer first. It is how the
// cross-origin-isolation requirement (ADR-0037 §b) gets verified on the code
// that actually ships, rather than only on Vite's dev headers.
const isDev = !app.isPackaged && process.env.ANVIL_EDITOR_SERVE_DIST !== "1";

// Cross-origin isolation, which is what makes `SharedArrayBuffer` exist. The
// engine asks for blocking network I/O and the only way to block a thread in
// JavaScript is `Atomics.wait` on a SharedArrayBuffer, which Chromium
// withholds unless the page is cross-origin isolated (ADR-0030, ADR-0031).
// `editor/vite.config.mjs:38-52` sends these same two in dev; this is the
// packaged half of the same requirement.
const ISOLATION = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
  "Cross-Origin-Resource-Policy": "same-origin",
};

// `file://` cannot be cross-origin isolated — it has no origin capable of it
// — so the packaged app serves itself over a scheme of its own instead
// (ADR-0037 §b). It has to be declared privileged before the app is ready.
protocol.registerSchemesAsPrivileged([
  {
    scheme: "anvil",
    privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: true, stream: true },
  },
]);

const TYPES = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".css": "text/css",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff2": "font/woff2",
  ".yaml": "application/yaml",
};

function serveDist() {
  protocol.handle("anvil", async (request) => {
    const url = new URL(request.url);
    const rel = decodeURIComponent(url.pathname);
    const file = path.normalize(path.join(DIST, rel === "/" ? "/index.html" : rel));
    // A shell that will read any file the process can reach is a shell that
    // leaks the machine — the same containment `vite.config.mjs` applies to
    // its `ejemplos/` middleware.
    if (!file.startsWith(DIST)) return new Response("outside dist/", { status: 403 });
    try {
      return new Response(await readFile(file), {
        headers: { "content-type": TYPES[path.extname(file)] ?? "application/octet-stream", ...ISOLATION },
      });
    } catch {
      return new Response("not found", { status: 404 });
    }
  });
}

// ------------------------------------------------------------- the bridge

/** The `anvil --bridge` this window started, if any. One at a time. */
let bridge = null;

function killBridge() {
  if (!bridge) return;
  const child = bridge;
  bridge = null;
  child.kill();
}

/// Starts `anvil <sequencePath> --bridge` and resolves to the `ws://` URL it
/// prints (`packaging/anvil-host/src/main.rs:617-649`) once it appears on
/// stdout, together with which engine it started (`findEngine`).
async function startBridge(sequencePath) {
  killBridge();

  const engine = await findEngine();
  const anvil = engine.path;

  // A sequence opened through the dialog arrives as a real filesystem path;
  // one opened with `?open=` arrives as the dev server's own `/ejemplos/…`,
  // which only means anything relative to the repo.
  const sequence = existsSync(sequencePath)
    ? sequencePath
    : path.join(REPO, sequencePath.replace(/^[/\\]+/, ""));

  const child = spawn(anvil, [sequence, "--bridge"], { stdio: ["ignore", "pipe", "ignore"] });

  return new Promise((resolve, reject) => {
    const done = (fn, value) => {
      clearTimeout(timer);
      lines.close();
      fn(value);
    };
    const timer = setTimeout(() => {
      child.kill();
      done(reject, new Error("the engine did not print a bridge URL within 5s"));
    }, 5000);

    child.on("error", (e) => {
      done(reject, new Error(`could not start ${anvil}: ${e.message}`));
    });

    const lines = createInterface({ input: child.stdout });
    lines.on("line", (line) => {
      // `openBridge()` (editor/src/app.mjs) wants the bare `ws://…` value,
      // the same thing it reads out of `?bridge=` when a person opens the
      // printed link by hand — not the whole printed `http://localhost…`.
      const at = line.indexOf("ws://");
      if (at === -1) return;
      bridge = child;
      done(resolve, { url: line.slice(at).trim(), engine });
    });
    child.on("exit", () => {
      done(reject, new Error("the engine exited before printing a bridge URL"));
    });
  });
}

// ------------------------------------------------------------- the engine

// The editor does not carry the engine: it uses the `anvil` installed on the
// machine, the way a code editor uses the compiler that is installed rather
// than one of its own (#80). ADR-0035 keeps the IDE and the engine separate
// artefacts, and the engine is what a bench gets on its own.

const ENGINE_EXE = process.platform === "win32" ? "anvil.exe" : "anvil";

/** Where a located engine is remembered, per user. */
const enginePrefs = () => path.join(app.getPath("userData"), "engine.json");

/// The engine to run, and where it was found, in this order:
///
/// 1. `ANVIL_EDITOR_ENGINE`, a path — for scripts and tests. If it is set and
///    wrong, that is an error, not a reason to quietly try something else.
/// 2. The one located through File ▸ Locate Anvil Engine…, remembered.
/// 3. The dev tree's release build, only when running unpackaged, so that
///    `npm run app` uses the engine built from this checkout rather than an
///    older one that happens to be on PATH.
/// 4. `anvil` on PATH. On Windows, a PATH set in one terminal is not the PATH
///    of an editor started from the Start menu, which is why 2 exists.
///
/// Each candidate is checked by asking it for `--version`: a file with the
/// right name that is not the engine fails here, with its path in the
/// message, rather than as a bridge that never prints its URL.
async function findEngine() {
  const forced = process.env.ANVIL_EDITOR_ENGINE;
  if (forced) return checkEngine(forced, "ANVIL_EDITOR_ENGINE");

  const remembered = await rememberedEngine();
  if (remembered && existsSync(remembered)) return checkEngine(remembered, "located");

  if (!app.isPackaged) {
    const dev = path.join(REPO, "packaging/anvil-host/target/release", ENGINE_EXE);
    if (existsSync(dev)) return checkEngine(dev, "dev tree");
  }

  for (const dir of (process.env.PATH ?? "").split(path.delimiter)) {
    if (!dir) continue;
    const candidate = path.join(dir, ENGINE_EXE);
    if (existsSync(candidate)) return checkEngine(candidate, "PATH");
  }

  throw engineError(
    `the Anvil engine (${ENGINE_EXE}) was not found on PATH` +
      (remembered ? ` nor at ${remembered}, where it was located before` : "") +
      ` — install it from the release page, then File ▸ Locate Anvil Engine…`,
  );
}

async function rememberedEngine() {
  try {
    return JSON.parse(await readFile(enginePrefs(), "utf8")).path ?? null;
  } catch {
    return null;
  }
}

/// Resolves to `{ path, version, source, editor }` if `file` answers `--version` as
/// the engine does. `anvil --version` writes to stderr (#75), so both
/// streams are read.
function checkEngine(file, source) {
  return new Promise((resolve, reject) => {
    let out = "";
    let child;
    try {
      child = spawn(file, ["--version"], { stdio: ["ignore", "pipe", "pipe"] });
    } catch (e) {
      reject(engineError(`${file} (${source}) could not be started: ${e.message}`));
      return;
    }
    const timer = setTimeout(() => child.kill(), 5000);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (out += d));
    child.on("error", (e) => {
      clearTimeout(timer);
      reject(engineError(`${file} (${source}) could not be started: ${e.message}`));
    });
    child.on("close", () => {
      clearTimeout(timer);
      const version = /^anvil (\S+)/m.exec(out)?.[1];
      // `editor` is this app's own version when it is a release, so the page
      // can say when the two differ; a dev run has none worth comparing.
      const editor = app.isPackaged ? app.getVersion() : null;
      if (version) resolve({ path: file, version, source, editor });
      else
        reject(
          engineError(
            `${file} (${source}) is not the Anvil engine: \`--version\` did not answer "anvil <version>"`,
          ),
        );
    });
  });
}

/// File ▸ Locate Anvil Engine…: picks the executable, checks it, and
/// remembers it. Resolves to the engine, or null if the dialog was dismissed.
async function locateEngine(window) {
  const { canceled, filePaths } = await dialog.showOpenDialog(window, {
    title: "Locate the Anvil engine",
    properties: ["openFile"],
    filters: process.platform === "win32" ? [{ name: "anvil.exe", extensions: ["exe"] }] : [],
  });
  if (canceled) return null;
  const engine = await checkEngine(filePaths[0], "located");
  await writeFile(enginePrefs(), JSON.stringify({ path: engine.path }, null, 2), "utf8");
  return engine;
}

/** Resolves to `{ value }` or `{ error }`; see `anvil:start-bridge`. */
const answer = (promise) =>
  promise.then(
    (value) => ({ value }),
    (e) => ({ error: e.message, code: e.code }),
  );

/** An error meaning "there is no usable engine", as opposed to a bridge that failed. */
const engineError = (message) => Object.assign(new Error(message), { code: "no-engine" });

// ---------------------------------------------------------------- the IPC

// Everything the page can reach, and nothing else (ADR-0037 §c). The
// renderer has no Node: these handlers are the whole surface, which is
// what `editor/src-tauri/capabilities/default.json` used to enumerate.
function wireIpc() {
  ipcMain.handle("anvil:open-dialog", async (event) => {
    const { canceled, filePaths } = await dialog.showOpenDialog(BrowserWindow.fromWebContents(event.sender), {
      filters: [{ name: "Anvil sequence", extensions: ["yseq", "yaml", "yml"] }],
      properties: ["openFile"],
    });
    return canceled ? null : filePaths[0];
  });

  ipcMain.handle("anvil:save-dialog", async (event, defaultName) => {
    const { canceled, filePath } = await dialog.showSaveDialog(BrowserWindow.fromWebContents(event.sender), {
      defaultPath: defaultName ?? "sequence.yseq",
      filters: [{ name: "Anvil sequence", extensions: ["yseq", "yaml", "yml"] }],
    });
    return canceled ? null : filePath;
  });

  ipcMain.handle("anvil:read-text", (_event, file) => readFile(file, "utf8"));

  // Separate from the one above because "there is no such file" is an answer
  // here, not a failure: `?open=` is handed either a real path or the dev
  // server's `/ejemplos/…`, and it has no way to tell which until it looks.
  // Rejecting for the expected case made Electron print a handler error on
  // every single start, in the log this shell exists to keep readable.
  // Anything else — a permission, a bad encoding — still throws.
  ipcMain.handle("anvil:read-text-if-any", async (_event, file) =>
    existsSync(file) ? readFile(file, "utf8") : null,
  );
  ipcMain.handle("anvil:write-text", (_event, file, text) => writeFile(file, text, "utf8"));
  // An engine that is not installed, or not where it was, is an expected
  // answer here, not a fault: it comes back as `{ error }` for the page to
  // show, rather than as a rejection Electron logs with a stack trace on every
  // file opened — the same reasoning as `anvil:read-text-if-any` below.
  ipcMain.handle("anvil:start-bridge", (_event, sequencePath) => answer(startBridge(sequencePath)));
  ipcMain.handle("anvil:stop-bridge", () => killBridge());
  ipcMain.handle("anvil:locate-engine", (event) =>
    answer(locateEngine(BrowserWindow.fromWebContents(event.sender))),
  );
  ipcMain.handle("anvil:ask-unsaved", (event, name) =>
    askUnsaved(BrowserWindow.fromWebContents(event.sender), name),
  );
  // What the page asks for once a "Save" answered at close or reload has
  // actually saved: the leaving it had to hold back, done now.
  ipcMain.on("anvil:leave", (event, kind) => {
    const window = BrowserWindow.fromWebContents(event.sender);
    if (kind === "close") window?.close();
    else window?.webContents.reload();
  });
  ipcMain.handle("anvil:versions", () => ({
    editor: app.getVersion(),
    packaged: app.isPackaged,
  }));
  ipcMain.on("anvil:work-state", (_event, state) => {
    work = {
      running: Boolean(state?.running),
      dirty: Boolean(state?.dirty),
      name: typeof state?.name === "string" ? state.name : null,
    };
    offerRestart();
  });
}

// ------------------------------------------------------------ the updates

/**
 * What the page says it is in the middle of. `offerRestart` reads it, and the
 * unsaved-changes question takes the file's name from it.
 */
let work = { running: false, dirty: false, name: null };
/** The version downloaded and waiting for a restart, if any. */
let downloaded = null;
/** Whether a check or a download is under way, so a second one waits. */
let busy = false;

// Updates come from the published GitHub Releases — never a draft, so a
// release under review reaches nobody. Nothing happens behind anyone's back:
// at start the editor asks whether to update (Update / Later / Never), it
// downloads only after "Update", and it installs only when the person says so,
// never while a run is in flight — a restart mid-run leaves a bench wherever
// the run had it, with no `cleanup`. "Never" turns the check at start off;
// Help ▸ Check for Updates… still checks by hand, and turns it back on.
//
// Only the NSIS installer and the AppImage update themselves. A `.deb` belongs
// to the system's package manager, and a dev tree to git.
const updatable = () =>
  app.isPackaged && (process.platform === "win32" || Boolean(process.env.APPIMAGE));

const updatePrefs = () => path.join(app.getPath("userData"), "updates.json");

async function checksAtStart() {
  try {
    return JSON.parse(await readFile(updatePrefs(), "utf8")).checkAtStart !== false;
  } catch {
    return true;
  }
}

async function setChecksAtStart(on) {
  await writeFile(updatePrefs(), JSON.stringify({ checkAtStart: on }, null, 2), "utf8");
  log(`checking for updates at start: ${on ? "on" : "off"}`);
}

function log(message) {
  console.log(`[updater] ${message}`);
}

/// The answers to the update questions, without showing them:
/// `ANVIL_EDITOR_UPDATE_ANSWERS=update,close` answers the first question
/// "update" and the second "close", in order. Native dialogs cannot be clicked
/// unattended, so without this none of these paths could be exercised.
const scripted = (process.env.ANVIL_EDITOR_UPDATE_ANSWERS ?? "").split(",").filter(Boolean);

async function ask(window, options, answers) {
  if (scripted.length > 0) {
    const answer = scripted.shift();
    log(`"${options.message}" answered "${answer}" (ANVIL_EDITOR_UPDATE_ANSWERS)`);
    return { response: answers.indexOf(answer), checkboxChecked: options.checkboxChecked };
  }
  const { response, checkboxChecked } = await dialog.showMessageBox(window, options);
  return { response, checkboxChecked };
}

function startUpdates() {
  if (!updatable()) return;
  const { autoUpdater } = updater;
  autoUpdater.autoDownload = false;
  // Nothing to install until the person has said "Update" (`checkForUpdates`).
  autoUpdater.autoInstallOnAppQuit = false;
  // Its own trail on this process's stdout, like the renderer's (ADR-0037 §e).
  autoUpdater.logger = { info: log, warn: log, error: log, debug: () => {} };

  // Where to look instead of the Release page. It is how the whole path —
  // check, download, install over the running copy — gets exercised against
  // a local server without publishing anything.
  if (process.env.ANVIL_EDITOR_UPDATE_URL) {
    autoUpdater.setFeedURL({ provider: "generic", url: process.env.ANVIL_EDITOR_UPDATE_URL });
  }

  autoUpdater.on("update-downloaded", (info) => {
    downloaded = info.version;
    offerRestart();
  });

  checksAtStart().then((on) => {
    if (on) checkForUpdates({ manual: false });
    else log("not checking at start (turned off with Never)");
  });
}

/// Checks the latest published release and offers it.
///
/// At start (`manual: false`) a failure or "nothing new" says nothing: no
/// network is not something to interrupt anyone for. By hand, every outcome
/// gets an answer, and the dialog carries the "check at start" switch so Never
/// can be undone where it is noticed.
async function checkForUpdates({ manual }) {
  const window = BrowserWindow.getAllWindows()[0];
  if (!updatable()) {
    if (manual) {
      await dialog.showMessageBox(window, {
        type: "info",
        title: "Check for Updates",
        message: "This copy of the editor does not update itself.",
        detail: app.isPackaged
          ? "Installed from a .deb, it is updated by the system's package manager."
          : "It is running from a source checkout.",
      });
    }
    return;
  }
  if (downloaded) return offerRestart({ again: true });
  if (busy) return;
  busy = true;
  try {
    const result = await updater.autoUpdater.checkForUpdates();
    const current = app.getVersion();
    const atStart = await checksAtStart();

    if (!result?.isUpdateAvailable) {
      if (manual) {
        const { checkboxChecked } = await ask(
          window,
          {
            type: "info",
            title: "Check for Updates",
            message: `Anvil Sequence Editor ${current} is the latest version.`,
            buttons: ["OK"],
            checkboxLabel: "Check for updates when the editor starts",
            checkboxChecked: atStart,
          },
          ["ok"],
        );
        if (checkboxChecked !== atStart) await setChecksAtStart(checkboxChecked);
      }
      return;
    }

    const version = result.updateInfo.version;
    const buttons = manual ? ["Update", "Later"] : ["Update", "Later", "Never"];
    const { response, checkboxChecked } = await ask(
      window,
      {
        type: "info",
        title: "Update available",
        message: `Anvil Sequence Editor ${version} is available.`,
        detail:
          `You have ${current}. Update downloads it now and asks before restarting.` +
          (manual ? "" : " Never stops checking at start; Help ▸ Check for Updates… still works."),
        buttons,
        defaultId: 0,
        cancelId: 1,
        noLink: true,
        ...(manual
          ? { checkboxLabel: "Check for updates when the editor starts", checkboxChecked: atStart }
          : {}),
      },
      ["update", "later", "never"],
    );
    if (manual && checkboxChecked !== atStart) await setChecksAtStart(checkboxChecked);

    if (response === 2) {
      await setChecksAtStart(false);
      return;
    }
    if (response !== 0) {
      log(`${version} offered; asking again next time`);
      return;
    }
    log(`downloading ${version}`);
    // Before the download, not after the restart question: electron-updater
    // registers its install-on-quit handler the moment a download finishes,
    // and only if this is already true then
    // (`node_modules/electron-updater/out/BaseUpdater.js:31-33`, 6.8.9). Set
    // once the restart question is answered, it came too late and closing the
    // editor installed nothing. Both answers to that question install — now,
    // or on close — so from "Update" on this is simply true.
    updater.autoUpdater.autoInstallOnAppQuit = true;
    await updater.autoUpdater.downloadUpdate();
  } catch (e) {
    log(`update check failed: ${e?.message ?? e}`);
    if (manual) {
      await dialog.showMessageBox(window, {
        type: "error",
        title: "Check for Updates",
        message: "Could not check for updates.",
        detail: String(e?.message ?? e),
      });
    }
  } finally {
    busy = false;
  }
}

let restartAsked = false;

/// Once a download is in: restart now, or install when the editor closes. Held
/// back while a run is in flight (`anvil:work-state` calls this again when it
/// ends).
async function offerRestart({ again = false } = {}) {
  if (!downloaded || work.running || (restartAsked && !again)) return;
  const window = BrowserWindow.getAllWindows()[0];
  if (!window) return;
  restartAsked = true;
  log(`${downloaded} downloaded; asking whether to restart now`);

  const { response } = await ask(
    window,
    {
      type: "info",
      title: "Update ready",
      message: `Anvil Sequence Editor ${downloaded} is ready to install.`,
      detail: work.dirty
        ? "There are unsaved changes: restarting asks whether to save them first."
        : "Restart the editor now to use it, or install it when you close the editor.",
      buttons: ["Restart now", "When I close the editor"],
      defaultId: work.dirty ? 1 : 0,
      cancelId: 1,
      noLink: true,
    },
    ["restart", "close"],
  );
  // A run may have started while the question was on screen.
  // No `killBridge` here: the unsaved-changes question can still cancel the
  // quit, and `will-quit` stops the bridge once it is really happening.
  if (response === 0 && !work.running) {
    updater.autoUpdater.quitAndInstall();
  }
}

// ------------------------------------------------------ unsaved changes

const UNSAVED_ANSWERS = ["save", "discard", "cancel"];

/// The Save / Don't Save / Cancel question, asked before anything throws the
/// open document away (#84). Resolves to "save", "discard" or "cancel".
///
/// `ANVIL_EDITOR_UNSAVED_ANSWER` answers it without showing it, for the same
/// reason `ANVIL_EDITOR_OPEN` exists: a native dialog cannot be clicked by
/// anything but a hand, so without it no path through here can be exercised
/// unattended.
async function askUnsaved(window, name) {
  const { response } = forced(name) ?? (await dialog.showMessageBox(window, unsavedQuestion(name)));
  return UNSAVED_ANSWERS[response] ?? "cancel";
}

function forced(name) {
  const answer = process.env.ANVIL_EDITOR_UNSAVED_ANSWER;
  if (!UNSAVED_ANSWERS.includes(answer)) return null;
  console.log(`[unsaved] ${name}: answered "${answer}" (ANVIL_EDITOR_UNSAVED_ANSWER)`);
  return { response: UNSAVED_ANSWERS.indexOf(answer) };
}

function unsavedQuestion(name) {
  return {
    type: "warning",
    title: "Unsaved changes",
    message: `Save the changes to ${name}?`,
    detail: "If you don't save them, they are lost.",
    buttons: ["Save", "Don't Save", "Cancel"],
    defaultId: 0,
    cancelId: 2,
    noLink: true,
  };
}

// Closing the window, quitting and reloading all unload the page, and the page
// refuses to unload while it has unsaved changes (`beforeunload` in app.mjs).
// Electron then asks here instead of showing Chromium's own prompt, which has
// no Save.
//
// It has to answer synchronously — letting the unload go on is
// `preventDefault()` on this very event — so "Save" cannot save first and then
// continue. It keeps the unload cancelled, has the page save, and the page
// asks for the close or the reload again once the save went through
// (`anvil:leave`). A save that is itself cancelled leaves the window as it was.
function guardUnsaved(window) {
  // `close` is emitted before `beforeunload`, so by the time the question is
  // asked this says whether it was a close. Anything else that unloads the
  // page is a reload.
  let leaving = "reload";
  window.on("close", () => {
    leaving = "close";
  });

  window.webContents.on("will-prevent-unload", (event) => {
    const kind = leaving;
    leaving = "reload";
    const name = work.name ?? "this sequence";
    const { response } = forced(name) ?? {
      response: dialog.showMessageBoxSync(window, unsavedQuestion(name)),
    };
    const answer = UNSAVED_ANSWERS[response] ?? "cancel";
    if (answer === "discard") event.preventDefault();
    else if (answer === "save") window.webContents.send("anvil:save-then-leave", kind);
  });
}

// --------------------------------------------------------------- the menu

// The native menu bar, and the only one inside the shell: the page's own
// File/View menus exist for a plain browser, which has no menu of its own to
// put them in, and `app.mjs` hides them when `window.anvil` is there. Two bars
// saying the same thing was how it looked on Windows before this.
//
// Each item only names an action; what the action does stays in `app.mjs`,
// next to the page's own menus, so the two cannot come to mean different
// things.
function buildMenu() {
  const send = (action) => (_item, window) => window?.webContents.send("anvil:menu", action);
  const item = (label, action, accelerator) => ({ label, accelerator, click: send(action) });

  Menu.setApplicationMenu(
    Menu.buildFromTemplate([
      ...(process.platform === "darwin" ? [{ role: "appMenu" }] : []),
      {
        label: "&File",
        submenu: [
          item("&New", "new", "CmdOrCtrl+N"),
          item("&Open…", "open", "CmdOrCtrl+O"),
          { type: "separator" },
          item("&Save", "save", "CmdOrCtrl+S"),
          item("Save &As…", "save-as", "CmdOrCtrl+Shift+S"),
          { type: "separator" },
          item("Locate Anvil &Engine…", "locate-engine"),
          { type: "separator" },
          process.platform === "darwin" ? { role: "close" } : { role: "quit" },
        ],
      },
      // Undo, cut, copy and paste in the text view and in every field of the
      // step settings: without the role, those shortcuts stop working on
      // Windows and Linux the moment a custom menu replaces the default one.
      { role: "editMenu" },
      {
        label: "&View",
        submenu: [
          item("S&teps", "view-steps"),
          item("Te&xt", "view-text"),
          { type: "separator" },
          { role: "reload" },
          { role: "toggleDevTools" },
          { type: "separator" },
          { role: "resetZoom" },
          { role: "zoomIn" },
          { role: "zoomOut" },
          { type: "separator" },
          { role: "togglefullscreen" },
        ],
      },
      { role: "windowMenu" },
      {
        label: "&Help",
        submenu: [
          {
            id: "check-for-updates",
            label: "Check for &Updates…",
            click: () => checkForUpdates({ manual: true }),
          },
        ],
      },
    ]),
  );
}

// -------------------------------------------------------------- the window

function createWindow() {
  const window = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 800,
    minHeight: 600,
    title: "Anvil Sequence Editor",
    show: false,
    webPreferences: {
      preload: path.join(HERE, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  window.once("ready-to-show", () => window.show());

  // The renderer's console, on this process's stdout. Unlike the Tauri
  // shell, the page needs no code of its own for this (ADR-0037 §e) — and
  // without it a frontend error is invisible to everything except the eyes
  // in front of the window, which is how three separate faults hid behind
  // "it does not work" while the previous shell was built.
  window.webContents.on("console-message", (event) => {
    const level = ["debug", "info", "warning", "error"][event.level] ?? "info";
    console.log(`[renderer:${level}] ${event.message} (${event.sourceId}:${event.lineNumber})`);
  });

  // A link to somewhere else is somewhere else: it opens in the person's
  // browser, never as a second window of this app pointed off-origin.
  window.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url);
    return { action: "deny" };
  });

  // `ANVIL_EDITOR_OPEN=<path>` opens that sequence at startup instead of
  // waiting for File ▸ Open. The native file dialog cannot be driven by a
  // test or by anything but a hand on a mouse, so without this there is no
  // way to exercise the open-and-run path unattended.
  const open = process.env.ANVIL_EDITOR_OPEN;
  const query = open ? `?open=${encodeURIComponent(open)}` : "";
  window.loadURL(isDev ? `${DEV_URL}/${query}` : `anvil://bundle/${query}`);

  guardUnsaved(window);

  // The bridge is this window's child, not a background service: it must not
  // outlive the window a person can see it from. `closed`, not `close`: a
  // close can still be cancelled by the unsaved-changes question, and a
  // window left open without its bridge has lost Run for nothing.
  window.on("closed", killBridge);
  return window;
}

// Chromium's remote debugging port, off unless asked for. This is what makes
// the shell inspectable from outside itself on every platform (ADR-0037 §e)
// — the thing the Linux WebKitGTK webview could not offer at all.
//
// Known wart: Chromium opens this listener without `FD_CLOEXEC`, so the
// engine's bridge — spawned by `startBridge` below — inherits the descriptor.
// A bridge that outlives its window keeps the port bound, and the next editor
// silently starts with no debug port at all. If a session claims it cannot
// reach the port, look for an orphaned `anvil --bridge` before believing
// anything else. Development-only, so it is documented rather than worked
// around.
if (process.env.ANVIL_EDITOR_DEBUG_PORT) {
  app.commandLine.appendSwitch("remote-debugging-port", process.env.ANVIL_EDITOR_DEBUG_PORT);
}

app.whenReady().then(() => {
  if (!isDev) serveDist();
  wireIpc();
  buildMenu();
  createWindow();
  startUpdates();
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  killBridge();
  if (process.platform !== "darwin") app.quit();
});

// `will-quit`, not `before-quit`, for the same reason as `closed` above: it is
// emitted only once every window has actually closed.
app.on("will-quit", killBridge);
