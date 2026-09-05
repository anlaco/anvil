// One engine, on its own thread.
//
// The engine is a synchronous WASM component: while it runs, whatever thread it
// is on does nothing else. On the page's main thread that means a frozen
// interface — dead buttons, no way to abort — for as long as a sequence takes.
// Fine for the 80 ms `ejemplos/basica.yaml` takes to validate; not fine for a
// real run.
//
// A worker is a real operating-system thread, so this is also the shape
// multi-UUT takes later: one worker per unit under test, each with its own
// engine instance and no shared memory between them. That isolation is the
// property `docs/vision.md` wants and that TestStand's shared-memory threading
// does not give — so it is worth keeping even once one worker would do.
//
// What this does NOT do is make the engine itself parallel. The engine is
// single-threaded (`crates/motor/src` has no `thread` or `spawn`) and
// in-sequence parallelism is post-MVP by decision
// (docs/diseno/motor-de-ejecucion.md:137). N workers are N sequences, not one
// sequence going faster — and a front end may not invent what the engine does
// not do (ADR-0031).

import { runEngine } from "./engine.mjs";
import { useBridge } from "./wasi/sockets.mjs";

// Fetched here rather than passed in: the bytes are large and the worker can
// read them itself.
const load = async (name) => {
  const res = await fetch(`/generated/${name}`);
  if (!res.ok) {
    throw new Error(`could not load ${name} (${res.status}). Run 'npm run transpile' first.`);
  }
  return new Uint8Array(await res.arrayBuffer());
};

self.onmessage = async ({ data }) => {
  // Wiring the bridge is a message of its own, sent once before any run: the
  // socket shim needs the shared channel and the port to the network worker
  // before the engine can reach anything (ADR-0030).
  if (data.op === "bridge") {
    useBridge({ control: data.control, data: data.data, port: data.port });
    self.postMessage({ bridge: "ready" });
    return;
  }

  const { id, args, files } = data;
  try {
    const result = await runEngine({ args, files, load });
    self.postMessage({ id, ok: true, result });
  } catch (e) {
    // A host failure is not a verdict about the sequence and must not be
    // reported as one (ADR-0019, Rule 2). It travels as `ok: false`, which the
    // caller renders differently from a rejected sequence.
    self.postMessage({ id, ok: false, error: e?.message ?? String(e) });
  }
};
