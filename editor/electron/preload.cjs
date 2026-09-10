// The entire surface the page can reach outside itself (ADR-0037 §c).
//
// CommonJS on purpose: a sandboxed preload has no ESM loader, and the
// sandbox is the point — the renderer gets no Node, no `require`, no
// filesystem and no child processes, only the five calls below, each of
// which is a message to a handler in `main.mjs` that this repo wrote and can
// enumerate. This is what replaced `src-tauri/capabilities/default.json`.
//
// `window.anvil` is also the feature-detection flag `editor/src/app.mjs`
// selects the native file branch on, the way it used to select on
// `window.__TAURI_INTERNALS__`.
const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("anvil", {
  /** Native open dialog. Resolves to a path, or null if it was dismissed. */
  openDialog: () => ipcRenderer.invoke("anvil:open-dialog"),
  /** Native save dialog. Resolves to a path, or null if it was dismissed. */
  saveDialog: (defaultName) => ipcRenderer.invoke("anvil:save-dialog", defaultName),
  readTextFile: (path) => ipcRenderer.invoke("anvil:read-text", path),
  writeTextFile: (path, text) => ipcRenderer.invoke("anvil:write-text", path, text),
  /** Starts `anvil <path> --bridge`; resolves to the `ws://` URL it prints. */
  startBridge: (path) => ipcRenderer.invoke("anvil:start-bridge", path),
});
