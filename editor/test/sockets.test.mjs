// What the socket shim does when the engine exits.
//
// The engine is a `wasi:cli/run` command that states its verdict by exiting,
// and an exit runs no destructors: it never drops its sockets. Natively the
// kernel closes them when the process ends. In the editor there is no process —
// the guest is one instance inside a page that stays up — so unless the shim
// closes them itself, the relay on the far side of the bridge outlives the run
// that opened it.
//
// The cost of that is not a leak, it is a dead Run button. An executor that
// serves one connection at a time — as the one built into anvil did until
// ADR-0041 removed it — is held for good by a connection left behind, and the
// next run's connection waits in the accept queue for ever (#61). Found by driving the editor against
// a real bridge and watching `ss` show two connections to the executor, the
// second with its request sitting unread in the receive queue.

import { strict as assert } from "node:assert";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { MessageChannel, Worker } from "node:worker_threads";

import { runEngine as run } from "../src/engine.mjs";
import { exampleFiles } from "./ejemplos.mjs";
import { DATA_BYTES } from "../src/wasi/channel.mjs";
import { closeOpenSockets, tcpCreateSocket, useBridge } from "../src/wasi/sockets.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));

const load = (name) => readFile(join(HERE, "..", "generated", name));

/**
 * Wires the shim to a worker that answers, and collects every request made.
 *
 * Returns the recorded operations and a `stop`; the array fills as the shim
 * asks, because the worker reports each request before answering it.
 */
function bridgeTo() {
  const control = new SharedArrayBuffer(4 * Int32Array.BYTES_PER_ELEMENT);
  const data = new SharedArrayBuffer(DATA_BYTES);
  const channel = new MessageChannel();

  const worker = new Worker(new URL("./bridge-responder.mjs", import.meta.url), {
    workerData: { control, data, port: channel.port2 },
    transferList: [channel.port2],
  });

  const asked = [];
  worker.on("message", (m) => asked.push(m));
  worker.unref();

  useBridge({ control, data, port: channel.port1 });
  // `stop` closes what is still open before it takes the answering side away.
  // The shim's set of open sockets is module state shared by every test here,
  // so a test that leaves one behind would otherwise have the next test see a
  // close it never asked for — and fail for someone else's reason.
  return {
    asked,
    stop: async () => {
      closeOpenSockets();
      await worker.terminate();
    },
  };
}

/**
 * Waits for the worker's report of the first `count` requests to arrive.
 *
 * The shim's own call is synchronous — that is the point of the channel — but
 * the worker reports what it saw by message, and those land on this thread's
 * event loop. Asserting without waiting reads an empty array.
 */
async function seen(asked, count) {
  for (let i = 0; i < 200 && asked.length < count; i++) {
    await new Promise((r) => setTimeout(r, 5));
  }
  return asked.map((a) => a.op);
}

/** An IPv4 address in the shape `wasi:sockets` hands to the shim. */
const address = (port) => ({ tag: "ipv4", val: { address: [127, 0, 0, 1], port } });

test("a socket the guest left open is closed when it exits", async () => {
  const { asked, stop } = bridgeTo();
  try {
    // Exactly what the engine does and then does not undo: connect, and exit
    // without dropping. `--validate` never opens a socket of its own
    // (crates/motor/src/bin/anvil.rs), so the only one in play is this.
    const socket = tcpCreateSocket.createTcpSocket("ipv4");
    socket.startConnect(null, address(9100));
    assert.deepEqual(
      await seen(asked, 1),
      ["connect"],
      "connecting should have reached the bridge and nothing else",
    );

    const { exitCode } = await run({
      args: ["basica.yseq", "--validate"],
      files: await exampleFiles("basica.yseq"),
      load,
    });
    assert.equal(exitCode, 0);

    // The close is the whole point: without it the bridge keeps relaying for a
    // run that is over, and the executor is never free again.
    assert.deepEqual(
      await seen(asked, 2),
      ["connect", "close"],
      "the guest exited, so its socket should have been closed",
    );
  } finally {
    await stop();
  }
});

test("closing twice asks the bridge once", async () => {
  const { asked, stop } = bridgeTo();
  try {
    const socket = tcpCreateSocket.createTcpSocket("ipv4");
    socket.startConnect(null, address(9100));
    socket.drop();
    // A guest that does drop its socket, and then exits, must not have the
    // shim close an id the bridge has already forgotten — the bridge would
    // answer `no connection`, which reads as a fault and is not one.
    //
    // Two things stop that, and this holds them together rather than either
    // one: `drop` takes the socket out of the open set, and it returns early
    // when already closed. Removing one alone keeps this green; removing both
    // is what turns it red, which is how it was checked.
    closeOpenSockets();

    assert.deepEqual(
      await seen(asked, 2),
      ["connect", "close"],
      "the second close should not have reached the bridge",
    );
  } finally {
    await stop();
  }
});
