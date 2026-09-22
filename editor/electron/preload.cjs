// The entire surface the page can reach outside itself (ADR-0037 §c).
//
// CommonJS on purpose: a sandboxed preload has no ESM loader, and the
// sandbox is the point — the renderer gets no Node, no `require`, no
// filesystem and no child processes, only the calls below, each of
// which is a message to a handler in `main.mjs` that this repo wrote and can
// enumerate. This is what replaced `src-tauri/capabilities/default.json`.
//
// `window.anvil` is also the feature-detection flag `editor/src/app.mjs`
// selects the native file branch on, the way it used to select on
// `window.__TAURI_INTERNALS__`.
const { contextBridge, ipcRenderer } = require("electron");

// The shell answers the engine calls with `{ value }` or `{ error }` so that an
// engine not being installed is not logged as a crash (`anvil:start-bridge` in
// main.mjs). To the page they are still a promise that resolves or rejects.
const unwrap = ({ value, error, code }) => {
  if (error !== undefined) throw Object.assign(new Error(error), { code });
  return value;
};

contextBridge.exposeInMainWorld("anvil", {
  /** Native open dialog. Resolves to a path, or null if it was dismissed. */
  openDialog: () => ipcRenderer.invoke("anvil:open-dialog"),
  /** Native save dialog. Resolves to a path, or null if it was dismissed. */
  saveDialog: (defaultName) => ipcRenderer.invoke("anvil:save-dialog", defaultName),
  readTextFile: (path) => ipcRenderer.invoke("anvil:read-text", path),
  /** The text, or null if there is no such file. Other failures still throw. */
  readTextFileIfAny: (path) => ipcRenderer.invoke("anvil:read-text-if-any", path),
  /** Whether a file is there. Answers nothing about what is in it. */
  fileExists: (path) => ipcRenderer.invoke("anvil:file-exists", path),
  writeTextFile: (path, text) => ipcRenderer.invoke("anvil:write-text", path, text),
  /**
   * Starts `anvil <path> --bridge`; resolves to `{ url, engine }` — the
   * `ws://` URL it prints and `{ path, version, source, editor }` of the engine used.
   */
  startBridge: (path) => ipcRenderer.invoke("anvil:start-bridge", path).then(unwrap),
  /** `{ editor, packaged }`: this app's version, and whether it is a release. */
  versions: () => ipcRenderer.invoke("anvil:versions"),
  /**
   * Asks for the engine's executable, checks it and remembers it. Resolves to
   * `{ path, version, source, editor }`, or null if the dialog was dismissed.
   */
  locateEngine: () => ipcRenderer.invoke("anvil:locate-engine").then(unwrap),
  /** Stops the bridge `startBridge` started, if there is one. */
  stopBridge: () => ipcRenderer.invoke("anvil:stop-bridge"),
  /**
   * The catalog of the executors a sequence declares (ADR-0044): runs
   * `anvil describe <path>` and resolves to its JSON. Runs no step, and does
   * not need a bridge — which is what lets a step's parameters be drawn with
   * nothing on the bench running.
   */
  describe: (path) => ipcRenderer.invoke("anvil:describe", path).then(unwrap),
  /**
   * Calls `listener(action)` for each item picked from the native menu. The
   * action is the same name the page's own menus carry in `data-action`.
   */
  onMenu: (listener) => ipcRenderer.on("anvil:menu", (_event, action) => listener(action)),
  /**
   * Asks Save / Don't Save / Cancel about unsaved changes to `name`. Resolves
   * to "save", "discard" or "cancel".
   */
  askUnsaved: (name) => ipcRenderer.invoke("anvil:ask-unsaved", name),
  /**
   * Calls `listener(kind)` when a close or reload ("close" | "reload") was
   * held back so the page can save first. Once it has, `leave(kind)` does it.
   */
  onSaveThenLeave: (listener) => ipcRenderer.on("anvil:save-then-leave", (_event, kind) => listener(kind)),
  leave: (kind) => ipcRenderer.send("anvil:leave", kind),
  /**
   * Tells the shell whether a run is in flight and whether there are unsaved
   * changes, so an update never offers to restart on top of either.
   */
  setWorkState: (state) => ipcRenderer.send("anvil:work-state", state),
});
