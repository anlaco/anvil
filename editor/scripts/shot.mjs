// Screenshots the running editor, and optionally evaluates an expression in
// it, over Chromium's remote debugging protocol (ADR-0037 §e).
//
// This is the thing the previous shell could not offer at all: WebKitGTK
// exposes no inspector anything can attach to, so a fault in the window was
// invisible to every tool in this repo — which is how three of them hid
// behind "it does not work" while that shell was built.
//
// Start the editor with a debug port, then run this in another terminal:
//
//   ANVIL_EDITOR_DEBUG_PORT=9222 npm run app
//   node scripts/shot.mjs out.png
//   node scripts/shot.mjs out.png 'document.querySelectorAll(".step").length'
//
// No dependency: `fetch` and `WebSocket` are both in Node.
import { writeFile } from "node:fs/promises";

const [out = "shot.png", expression] = process.argv.slice(2);
const port = process.env.ANVIL_EDITOR_DEBUG_PORT ?? "9222";

const targets = await fetch(`http://127.0.0.1:${port}/json`).then((r) => r.json());
// Every window is a target; the editor's page is the one that is not a
// worker or a devtools surface of its own.
const page = targets.find((t) => t.type === "page" && !t.url.startsWith("devtools://"));
if (!page) {
  console.error(`no page on :${port} — is the editor running with ANVIL_EDITOR_DEBUG_PORT set?`);
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve, { once: true });
  ws.addEventListener("error", reject, { once: true });
});

let next = 0;
const pending = new Map();
ws.addEventListener("message", (event) => {
  const msg = JSON.parse(event.data);
  const settle = pending.get(msg.id);
  if (!settle) return;
  pending.delete(msg.id);
  msg.error ? settle.reject(new Error(msg.error.message)) : settle.resolve(msg.result);
});

const send = (method, params = {}) =>
  new Promise((resolve, reject) => {
    const id = ++next;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params }));
  });

if (expression) {
  const { result } = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  console.log(JSON.stringify(result.value ?? result.description ?? null));
}

const { data } = await send("Page.captureScreenshot", { format: "png" });
await writeFile(out, Buffer.from(data, "base64"));
console.log(`${out} (${page.title})`);
ws.close();
