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
import { app, BrowserWindow, dialog, ipcMain, protocol, shell } from "electron";
import { writeFile } from "node:fs/promises";

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
/// stdout.
///
/// **Stopgap, not the real answer**, carried over from the Tauri shell
/// unchanged (ADR-0037 §d): it finds `anvil` by a path relative to this file
/// that only exists in the dev tree — it breaks the moment this ships
/// packaged, because there is no bundling story for the engine yet (#67).
/// Good enough to develop against today; revisit before this goes further
/// than a developer's own checkout.
async function startBridge(sequencePath) {
  killBridge();

  const anvil = path.join(
    REPO,
    process.platform === "win32"
      ? "packaging/anvil-host/target/release/anvil.exe"
      : "packaging/anvil-host/target/release/anvil",
  );

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
      done(resolve, line.slice(at).trim());
    });
    child.on("exit", () => {
      done(reject, new Error("the engine exited before printing a bridge URL"));
    });
  });
}

// ---------------------------------------------------------------- the IPC

// Everything the page can reach, and nothing else (ADR-0037 §c). The
// renderer has no Node: these five handlers are the whole surface, which is
// what `editor/src-tauri/capabilities/default.json` used to enumerate.
function wireIpc() {
  ipcMain.handle("anvil:open-dialog", async (event) => {
    const { canceled, filePaths } = await dialog.showOpenDialog(BrowserWindow.fromWebContents(event.sender), {
      filters: [{ name: "Anvil sequence", extensions: ["yaml", "yml"] }],
      properties: ["openFile"],
    });
    return canceled ? null : filePaths[0];
  });

  ipcMain.handle("anvil:save-dialog", async (event, defaultName) => {
    const { canceled, filePath } = await dialog.showSaveDialog(BrowserWindow.fromWebContents(event.sender), {
      defaultPath: defaultName ?? "sequence.yaml",
      filters: [{ name: "Anvil sequence", extensions: ["yaml", "yml"] }],
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
  ipcMain.handle("anvil:start-bridge", (_event, sequencePath) => startBridge(sequencePath));
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

  // The bridge is this window's child, not a background service: it must not
  // outlive the window a person can see it from.
  window.on("close", killBridge);
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
  createWindow();
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  killBridge();
  if (process.platform !== "darwin") app.quit();
});

app.on("before-quit", killBridge);
