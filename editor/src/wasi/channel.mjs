// The channel that lets a synchronous caller wait on an asynchronous answer.
//
// The engine's network calls are synchronous: `blockingRead` returns bytes,
// `Pollable.block()` returns nothing. The browser's networking is not — a
// WebSocket only ever hands you data in a callback. Bridging the two is the
// whole difficulty of the socket shim, and there is exactly one mechanism for
// it in JavaScript: `Atomics.wait` on a `SharedArrayBuffer`, which blocks a
// worker thread outright until another thread wakes it.
//
// So the work is split across two workers. The engine's worker calls into
// here, posts a request and blocks. The network worker owns the WebSocket,
// does the asynchronous half, writes the answer into shared memory and wakes
// the caller. Neither ever runs on the page's own thread — `Atomics.wait` is
// forbidden there, and it would freeze the interface even if it were not.
//
// This requires the page to be cross-origin isolated, which is why the editor
// is served with COOP/COEP (ADR-0030). Without those headers
// `SharedArrayBuffer` does not exist and none of this can work.

/** Slots in the control array. */
const STATE = 0; // 0 = request pending, 1 = answered
const STATUS = 1; // 0 = ok, negative = error, 1 = end of stream
const LENGTH = 2; // bytes written into the data buffer
const CONTROL_SLOTS = 4;

/** Answer statuses. */
export const OK = 0;
export const EOS = 1; // the peer closed: `read` returns nothing, for good
export const ERROR = -1;

/** How long a blocking wait tolerates silence before giving up.
 *
 * Not a network timeout — the bridge and the executors have their own. This is
 * the last resort against a wedged network worker, because a thread parked in
 * `Atomics.wait` with no timeout is a thread that never comes back, and the
 * engine would hang with no message. `wasi-grpc` has no deadlines of its own
 * (crates/ejecutor_pasos/src/main.rs:122-124), so nothing below would notice.
 */
const WAIT_MS = 120_000;

/** How much data one exchange can carry. Reads are chunked to fit. */
export const DATA_BYTES = 1 << 20;

/** Allocates the shared memory the two workers exchange through. */
export function createChannel() {
  return {
    control: new SharedArrayBuffer(CONTROL_SLOTS * Int32Array.BYTES_PER_ELEMENT),
    data: new SharedArrayBuffer(DATA_BYTES),
  };
}

/**
 * The blocking side, used from the engine's worker.
 *
 * One request is in flight at a time, which is what the engine does anyway: it
 * is single-threaded and speaks one gRPC call at a time.
 */
export class BlockingClient {
  #control;
  #data;
  #port;

  constructor({ control, data, port }) {
    this.#control = new Int32Array(control);
    this.#data = new Uint8Array(data);
    this.#port = port;
  }

  /**
   * Posts a request and blocks until the network worker answers.
   *
   * Returns `{ status, bytes }`. `bytes` is a copy, because the shared buffer
   * is reused by the next request and handing out a view would alias it.
   */
  request(message, payload) {
    if (payload && payload.byteLength > 0) {
      if (payload.byteLength > DATA_BYTES) {
        throw new Error(`payload of ${payload.byteLength} exceeds the channel`);
      }
      this.#data.set(payload, 0);
    }

    Atomics.store(this.#control, STATE, 0);
    Atomics.store(this.#control, STATUS, OK);
    Atomics.store(this.#control, LENGTH, 0);

    this.#port.postMessage({ ...message, length: payload ? payload.byteLength : 0 });

    // The wake-up can race the wait: if the answer lands before we park, STATE
    // is already 1 and `Atomics.wait` returns "not-equal" immediately. That is
    // correct, not a missed wake-up, and it is why the state is stored before
    // the message goes out.
    const woke = Atomics.wait(this.#control, STATE, 0, WAIT_MS);
    if (woke === "timed-out") {
      throw new Error(
        "the network worker did not answer in 120s. The bridge may have stopped; " +
          "the engine cannot tell a hung host from a slow instrument, so it stops here " +
          "rather than waiting forever.",
      );
    }

    const status = Atomics.load(this.#control, STATUS);
    const length = Atomics.load(this.#control, LENGTH);
    const bytes = length > 0 ? this.#data.slice(0, length) : new Uint8Array(0);
    return { status, bytes };
  }
}

/**
 * The answering side, used from the network worker.
 *
 * `answer` is called from asynchronous code once the WebSocket has produced
 * something. It writes into shared memory and wakes exactly one waiter.
 */
export class ChannelResponder {
  #control;
  #data;

  constructor({ control, data }) {
    this.#control = new Int32Array(control);
    this.#data = new Uint8Array(data);
  }

  answer(status, bytes) {
    const length = bytes ? Math.min(bytes.byteLength, DATA_BYTES) : 0;
    if (length > 0) this.#data.set(bytes.subarray(0, length), 0);
    Atomics.store(this.#control, LENGTH, length);
    Atomics.store(this.#control, STATUS, status);
    Atomics.store(this.#control, STATE, 1);
    Atomics.notify(this.#control, STATE);
  }

  fail(message) {
    const bytes = new TextEncoder().encode(message);
    this.answer(ERROR, bytes);
  }
}
