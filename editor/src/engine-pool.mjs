// Runs sequences on worker threads, off the interface's thread.
//
// Written as a pool from the start even though the editor uses one worker
// today: multi-UUT is one worker per unit (see engine-worker.mjs), and the
// difference between "one worker" and "a pool of one" is where every awkward
// assumption gets baked in — a single implicit worker, an id that is really a
// singleton, a cancel that terminates "the" engine.
//
// The worker factory is injected so this can be tested without a browser.

/** How a run ended, when the host itself failed rather than the sequence. */
export class EngineHostError extends Error {}

export class EnginePool {
  #spawn;
  #max;
  #idle = [];
  #busy = new Set();
  #queue = [];
  #nextId = 1;
  #net = null;
  #bridged = false;
  #engineArgs = [];

  /**
   * `spawn` returns something with the Worker interface: `postMessage`,
   * `onmessage`, `terminate`.
   */
  constructor({ spawn, max = 1 }) {
    this.#spawn = spawn;
    this.#max = max;
  }

  /** Workers currently running something. */
  get running() {
    return this.#busy.size;
  }

  /**
   * Connects this pool's engine worker to a bridge.
   *
   * One worker, one bridge, for now. Multi-UUT needs a channel and a network
   * worker per engine worker — they cannot share, because the channel carries
   * one blocking request at a time by construction — and that is left until
   * there is a second unit to test.
   */
  async attachBridge(url, connect, onLost) {
    if (this.#max !== 1) {
      throw new Error("attaching a bridge to a pool of more than one is not implemented");
    }
    const worker = this.#idle.pop() ?? this.#spawn();
    // `onLost` fires if the socket goes later. The pool forgets the bridge
    // first, so that by the time anyone is told, `bridged` already says no —
    // a caller that repaints on the notice must not paint a Run that cannot run.
    //
    // The network worker itself is kept: it is what answers "not connected to a
    // bridge" to anything the engine asks from here on. Terminating it would
    // leave those requests with nobody to answer them, and the engine parked in
    // `Atomics.wait` until the channel's 120 s last resort (channel.mjs).
    const lost = (reason) => {
      this.#bridged = false;
      this.#engineArgs = [];
      onLost?.(reason);
    };
    const { net, engineArgs } = await connect(worker, url, lost);
    this.#net = net;
    this.#engineArgs = engineArgs ?? [];
    this.#idle.push(worker);
    this.#bridged = true;
  }

  /** Whether a bridge is attached and the engine can reach executors. */
  get bridged() {
    return this.#bridged;
  }

  /**
   * The arguments the bridge says the engine needs — the ephemeral port of the
   * embedded executor and an `--executor` for each declared one.
   *
   * They come from the bridge rather than being guessed here because it is the
   * bridge that reserved those ports, exactly as the native host does before
   * handing them to the guest as argv.
   */
  get engineArgs() {
    return this.#engineArgs;
  }

  /** Runs the engine and resolves with `{ exitCode, stdout, stderr }`. */
  run({ args = [], files = {}, onLine }) {
    return new Promise((resolve, reject) => {
      this.#queue.push({ args, files, onLine, resolve, reject });
      this.#pump();
    });
  }

  #pump() {
    while (this.#queue.length > 0) {
      const worker = this.#take();
      if (!worker) return; // all busy; the next completion pumps again
      const job = this.#queue.shift();
      this.#dispatch(worker, job);
    }
  }

  #take() {
    if (this.#idle.length > 0) return this.#idle.pop();
    if (this.#busy.size < this.#max) return this.#spawn();
    return null;
  }

  #dispatch(worker, job) {
    const id = this.#nextId++;
    this.#busy.add(worker);

    worker.onmessage = ({ data }) => {
      if (data.id !== id) return;
      // A line is progress, not an answer: the job is still running, so the
      // worker is not released and the handler stays installed.
      if (data.line !== undefined) {
        job.onLine?.(data.line);
        return;
      }
      worker.onmessage = null;
      this.#busy.delete(worker);
      this.#idle.push(worker);

      if (data.ok) job.resolve(data.result);
      else job.reject(new EngineHostError(data.error));

      this.#pump();
    };

    // A worker that dies takes its job with it; without this the promise never
    // settles and the interface waits forever on a thread that is gone.
    worker.onerror = (e) => {
      worker.onmessage = null;
      this.#busy.delete(worker);
      job.reject(new EngineHostError(e?.message ?? "the engine worker died"));
      this.#pump();
    };

    worker.postMessage({ id, args: job.args, files: job.files });
  }

  /**
   * Stops everything in flight.
   *
   * Terminating the thread is the only abort available: the engine has no
   * cancellation of its own — it is declared post-MVP together with
   * parallelism (docs/diseno/motor-de-ejecucion.md:137). That is survivable
   * while nothing is being executed, because a validate touches only memory.
   *
   * It will NOT be survivable once Run reaches hardware. Killing the thread
   * mid-sequence leaves whatever the bench was doing exactly as it was, with no
   * `cleanup` run — a power supply still at 30 V. Before Run does anything
   * real, abort has to become something the engine understands, not something
   * done to it from outside.
   */
  terminateAll() {
    for (const w of [...this.#busy, ...this.#idle]) w.terminate();
    this.#net?.terminate();
    this.#net = null;
    this.#bridged = false;
    this.#engineArgs = [];
    this.#busy.clear();
    this.#idle = [];
    for (const job of this.#queue.splice(0)) {
      job.reject(new EngineHostError("the engine was stopped"));
    }
  }
}

/** The pool the editor uses: workers built from engine-worker.mjs. */
export function browserPool({ max = 1 } = {}) {
  return new EnginePool({
    max,
    spawn: () =>
      new Worker(new URL("./engine-worker.mjs", import.meta.url), { type: "module" }),
  });
}

/**
 * Connects an engine worker to a bridge, so the engine can reach executors.
 *
 * Three threads and two channels: the engine's worker blocks on a
 * `SharedArrayBuffer`, a network worker owns the WebSocket, and a
 * `MessageChannel` carries requests directly between them without touching the
 * page's thread — which may be busy painting, and which is not allowed to
 * block anyway.
 *
 * Resolves when the WebSocket is up, and rejects with the reason when it is
 * not. Without this the shim throws on the first socket call, which is exactly
 * what should happen: no bridge means no executor, and saying so beats
 * pretending a connection was refused (ADR-0019, Rule 2).
 */
export async function connectBridge(engineWorker, url, onLost) {
  if (typeof SharedArrayBuffer === "undefined") {
    throw new EngineHostError(
      "SharedArrayBuffer is not available, so the engine cannot block on network " +
        "calls. The page must be cross-origin isolated (COOP/COEP) — see ADR-0030.",
    );
  }

  const control = new SharedArrayBuffer(4 * Int32Array.BYTES_PER_ELEMENT);
  const data = new SharedArrayBuffer(1 << 20);
  const channel = new MessageChannel();

  const net = new Worker(new URL("./net-worker.mjs", import.meta.url), { type: "module" });

  const started = new Promise((resolve, reject) => {
    net.onmessage = ({ data: m }) => {
      if (m.started) resolve(m.engineArgs ?? []);
      else reject(new EngineHostError(m.error ?? "the network worker did not start"));
    };
    net.onerror = (e) =>
      reject(new EngineHostError(e?.message ?? "the network worker died on start"));
  });

  net.postMessage({ op: "start", control, data, url, port: channel.port2 }, [channel.port2]);
  const engineArgs = await started;

  // The start-up handler is done; from here the only thing the network worker
  // has to say is that the socket went. Without this the message lands on a
  // promise that has already settled and nobody hears it.
  net.onmessage = ({ data: m }) => {
    if (m.lost) onLost?.(m.reason);
  };

  const wired = new Promise((resolve) => {
    const previous = engineWorker.onmessage;
    engineWorker.onmessage = (e) => {
      if (e.data?.bridge === "ready") {
        engineWorker.onmessage = previous;
        resolve();
      }
    };
  });
  engineWorker.postMessage({ op: "bridge", control, data, port: channel.port1 }, [channel.port1]);
  await wired;

  return { net, engineArgs };
}
