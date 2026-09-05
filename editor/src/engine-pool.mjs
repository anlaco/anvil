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

  /** Runs the engine and resolves with `{ exitCode, stdout, stderr }`. */
  run({ args = [], files = {} }) {
    return new Promise((resolve, reject) => {
      this.#queue.push({ args, files, resolve, reject });
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
