// The engine pool: keeping the engine off the interface's thread, and being
// the shape multi-UUT will take.
//
// Exercised with stand-in workers rather than real ones. What is worth testing
// here is the scheduling and the failure handling — that a job is never lost,
// that a dead worker does not leave the interface waiting forever, and that a
// host failure is not reported as a verdict about the sequence.

import { strict as assert } from "node:assert";
import { test } from "node:test";

import { EnginePool, EngineHostError } from "../src/engine-pool.mjs";

/** A worker that answers whatever the script says, asynchronously. */
function fakeWorker({ reply, onPost } = {}) {
  const w = {
    onmessage: null,
    onerror: null,
    terminated: false,
    posted: [],
    postMessage(msg) {
      this.posted.push(msg);
      onPost?.(w, msg);
      if (!reply) return;
      queueMicrotask(() => w.onmessage?.({ data: { id: msg.id, ...reply(msg) } }));
    },
    terminate() {
      this.terminated = true;
    },
  };
  return w;
}

const ok = (result) => () => ({ ok: true, result });

test("a run goes to a worker and comes back", async () => {
  const workers = [];
  const pool = new EnginePool({
    max: 1,
    spawn: () => {
      const w = fakeWorker({ reply: ok({ exitCode: 0, stdout: "", stderr: "valid" }) });
      workers.push(w);
      return w;
    },
  });

  const result = await pool.run({ args: ["seq.yaml", "--validate"], files: { "seq.yaml": "x" } });

  assert.deepEqual(result, { exitCode: 0, stdout: "", stderr: "valid" });
  assert.equal(workers.length, 1);
  assert.deepEqual(workers[0].posted[0].args, ["seq.yaml", "--validate"]);
});

test("a non-zero exit is a result, not an error", async () => {
  // A rejected sequence is an answer. Only the host failing is an exception.
  const pool = new EnginePool({
    max: 1,
    spawn: () => fakeWorker({ reply: ok({ exitCode: 1, stdout: "", stderr: "campo desconocido" }) }),
  });

  const result = await pool.run({ args: [], files: {} });
  assert.equal(result.exitCode, 1);
  assert.match(result.stderr, /campo desconocido/);
});

test("a host failure arrives as EngineHostError", { timeout: 5000 }, async () => {
  const pool = new EnginePool({
    max: 1,
    spawn: () => fakeWorker({ reply: () => ({ ok: false, error: "wasi:sockets is not available" }) }),
  });

  await assert.rejects(
    () => pool.run({ args: [], files: {} }),
    (e) => e instanceof EngineHostError && /wasi:sockets/.test(e.message),
  );
});

test("a worker that dies does not leave the caller waiting", { timeout: 5000 }, async () => {
  // Without this the promise never settles and the interface waits forever on
  // a thread that is gone — a hang with no message, the worst failure mode a
  // UI can have.
  const pool = new EnginePool({
    max: 1,
    spawn: () =>
      fakeWorker({
        onPost: (w) => queueMicrotask(() => w.onerror?.({ message: "worker crashed" })),
      }),
  });

  await assert.rejects(
    () => pool.run({ args: [], files: {} }),
    (e) => e instanceof EngineHostError && /crashed/.test(e.message),
  );
});

test("with one worker, runs queue instead of overlapping", { timeout: 5000 }, async () => {
  // The engine is single-threaded and a component instance is single-use;
  // two runs on one worker at once would interleave on the same thread.
  let inFlight = 0;
  let maxInFlight = 0;
  const pool = new EnginePool({
    max: 1,
    spawn: () =>
      fakeWorker({
        onPost: (w, msg) => {
          inFlight++;
          maxInFlight = Math.max(maxInFlight, inFlight);
          setTimeout(() => {
            inFlight--;
            w.onmessage?.({ data: { id: msg.id, ok: true, result: { exitCode: 0 } } });
          }, 5);
        },
      }),
  });

  await Promise.all([1, 2, 3].map(() => pool.run({ args: [], files: {} })));
  assert.equal(maxInFlight, 1, "one worker must run one sequence at a time");
});

test("a pool of N runs N at once — the shape multi-UUT needs", { timeout: 5000 }, async () => {
  let inFlight = 0;
  let maxInFlight = 0;
  const pool = new EnginePool({
    max: 3,
    spawn: () =>
      fakeWorker({
        onPost: (w, msg) => {
          inFlight++;
          maxInFlight = Math.max(maxInFlight, inFlight);
          setTimeout(() => {
            inFlight--;
            w.onmessage?.({ data: { id: msg.id, ok: true, result: { exitCode: 0 } } });
          }, 5);
        },
      }),
  });

  await Promise.all([1, 2, 3].map(() => pool.run({ args: [], files: {} })));
  assert.equal(maxInFlight, 3, "three workers must be able to run three sequences");
});

test("terminating rejects what was queued rather than dropping it", { timeout: 5000 }, async () => {
  const pool = new EnginePool({
    max: 1,
    spawn: () => fakeWorker({ onPost: () => {} }), // never answers
  });

  const first = pool.run({ args: [], files: {} });
  const queued = pool.run({ args: [], files: {} });
  first.catch(() => {});

  pool.terminateAll();

  await assert.rejects(() => queued, (e) => e instanceof EngineHostError && /stopped/.test(e.message));
});
