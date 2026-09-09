// The answering half of the channel, for tests: stands in for the network
// worker, records what the socket shim asked for and says OK to all of it.
//
// It is a real worker on a real SharedArrayBuffer because that is the only way
// to exercise `BlockingClient`: it parks the calling thread in `Atomics.wait`,
// so whatever wakes it cannot be on that thread.

import { parentPort, workerData } from "node:worker_threads";

import { ChannelResponder, OK } from "../src/wasi/channel.mjs";

const responder = new ChannelResponder({
  control: workerData.control,
  data: workerData.data,
});

workerData.port.on("message", (message) => {
  parentPort.postMessage({ op: message.op, id: message.id });
  responder.answer(OK, null);
});
