// The install folder: a logical `runtime` name becoming a command line, and
// the failures that must name what was looked for (ADR-0046 §4, §g).

import { strict as assert } from "node:assert";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

import { parse } from "yaml";

import {
  devCommand,
  executorsRoot,
  installedRuntimes,
  notLocal,
  readRuntime,
} from "../electron/runtimes.mjs";

/** An install folder with the runtimes given, each with its executable. */
function install(runtimes) {
  const root = mkdtempSync(path.join(tmpdir(), "anvil-executors-"));
  for (const [name, manifest] of Object.entries(runtimes)) {
    const dir = path.join(root, name);
    mkdirSync(dir, { recursive: true });
    writeFileSync(path.join(dir, "executor.json"), JSON.stringify(manifest));
    if (manifest.exec) {
      const exe = path.join(dir, manifest.exec);
      writeFileSync(exe, "#!/bin/sh\nexit 0\n");
      chmodSync(exe, 0o755);
    }
  }
  return root;
}

const WASM = { runtime: "wasm", exec: "anvil-exec-wasm", code: "--modules", port: "--port", eof: "--exit-on-eof" };
const PYTHON = { runtime: "python", exec: "anvil-exec-python", code: "--steps", port: "--port" };

test("ANVIL_HOME moves the whole folder; otherwise it is ~/.anvil", () => {
  assert.equal(
    executorsRoot({ ANVIL_HOME: "/opt/anvil" }, "/home/x"),
    path.join("/opt/anvil", "executors"),
  );
  assert.equal(executorsRoot({}, "/home/x"), path.join("/home/x", ".anvil", "executors"));
});

test("a runtime is a name, and the manifest says which flag takes the code", () => {
  const root = install({ wasm: WASM, python: PYTHON });
  try {
    assert.deepEqual(installedRuntimes(root), ["python", "wasm"]);

    const wasm = readRuntime("wasm", root, "linux");
    assert.equal(wasm.code, "--modules");
    assert.equal(wasm.port, "--port");
    assert.equal(wasm.exe, path.join(root, "wasm", "anvil-exec-wasm"));

    // The point of the indirection: the same caller, a different flag.
    assert.equal(readRuntime("python", root, "linux").code, "--steps");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a runtime that is not installed names what was looked for and what is", () => {
  const root = install({ wasm: WASM });
  try {
    assert.throws(
      () => readRuntime("python", root, "linux"),
      (e) => {
        assert.match(e.message, /no runtime 'python'/);
        assert.ok(e.message.includes(path.join(root, "python", "executor.json")), "names the file");
        assert.match(e.message, /installed there: wasm/);
        return true;
      },
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("an empty install folder says so rather than listing nothing", () => {
  const root = install({});
  try {
    assert.throws(() => readRuntime("wasm", root, "linux"), /nothing is installed there/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a folder renamed under another name is refused, not answered to twice", () => {
  const root = install({ "wasm-old": { ...WASM } });
  try {
    assert.throws(
      () => readRuntime("wasm-old", root, "linux"),
      /says it is the runtime 'wasm', but it is installed as 'wasm-old'/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a manifest missing a field says which one", () => {
  const root = install({ wasm: { runtime: "wasm", exec: "anvil-exec-wasm", port: "--port" } });
  try {
    assert.throws(() => readRuntime("wasm", root, "linux"), /has no 'code'/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("on Windows the extensions Windows would have tried are tried, and named", () => {
  const root = install({ wasm: { ...WASM, exec: "anvil-exec-wasm" } });
  try {
    // The fixture wrote the bare name, which on Windows is what a `.exe` is
    // found *beside*; here it stands in for it and resolves first.
    assert.equal(readRuntime("wasm", root, "win32").exe, path.join(root, "wasm", "anvil-exec-wasm"));

    const bare = install({ wasm: { runtime: "wasm", exec: "gone", code: "--modules", port: "--port" } });
    rmSync(path.join(bare, "wasm", "gone"));
    try {
      assert.throws(
        () => readRuntime("wasm", bare, "win32"),
        (e) => {
          assert.match(e.message, /'gone', 'gone\.exe', 'gone\.cmd', 'gone\.bat'/);
          return true;
        },
      );
    } finally {
      rmSync(bare, { recursive: true, force: true });
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the command takes its code relative to the sequence and its port from the declaration", () => {
  const root = install({ wasm: WASM });
  const project = mkdtempSync(path.join(tmpdir(), "anvil-seq-"));
  mkdirSync(path.join(project, "departamento", "dist"), { recursive: true });
  try {
    const cmd = devCommand(
      { runtime: "wasm", code: "departamento/dist" },
      { sequenceDir: project, port: 9101, root, platform: "linux" },
    );
    assert.equal(cmd.exe, path.join(root, "wasm", "anvil-exec-wasm"));
    assert.deepEqual(cmd.args, [
      "--modules",
      path.join(project, "departamento", "dist"),
      "--port",
      "9101",
      "--exit-on-eof",
    ]);
    assert.equal(cmd.cwd, project);
    assert.equal(cmd.stopsOnEof, true, "so the launcher knows to give it a stdin");
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(project, { recursive: true, force: true });
  }
});

test("code that is not there is caught before anything is spawned", () => {
  const root = install({ wasm: WASM });
  const project = mkdtempSync(path.join(tmpdir(), "anvil-seq-"));
  try {
    assert.throws(
      () => devCommand({ runtime: "wasm", code: "pasos" }, { sequenceDir: project, port: 9101, root }),
      (e) => {
        assert.match(e.message, /'pasos' is not there/);
        assert.ok(e.message.includes(path.join(project, "pasos")), "names where it looked");
        return true;
      },
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(project, { recursive: true, force: true });
  }
});

test("an executor on another machine is not this editor's to start", () => {
  assert.equal(notLocal("127.0.0.1"), null);
  assert.equal(notLocal("localhost"), null);
  assert.equal(notLocal("::1"), null);
  assert.match(notLocal("192.168.1.50"), /is not this machine/);
});

// ---------------------------------------------------------------------------
// The manifests this repo ships, and the `dev:` blocks the examples carry.
// Neither is checked by anything else: a typo in either is only noticed by
// whoever presses the button.
// ---------------------------------------------------------------------------

test("every manifest in executors/ is one this reader accepts", () => {
  const repo = path.resolve(import.meta.dirname, "..", "..");
  const shipped = ["wasm", "python"];
  const root = mkdtempSync(path.join(tmpdir(), "anvil-shipped-"));
  try {
    for (const name of shipped) {
      const manifest = JSON.parse(
        readFileSync(path.join(repo, "executors", name, "executor.json"), "utf8"),
      );
      // Copied into the shape the install folder has, since in the source tree
      // the executable is under `target/` and not beside its manifest.
      const dir = path.join(root, name);
      mkdirSync(dir, { recursive: true });
      writeFileSync(path.join(dir, "executor.json"), JSON.stringify(manifest));
      writeFileSync(path.join(dir, manifest.exec), "");
      const read = readRuntime(name, root, "linux");
      assert.equal(read.name, name);
      assert.ok(read.code.startsWith("--"), `${name}: 'code' is a flag`);
      assert.ok(read.port.startsWith("--"), `${name}: 'port' is a flag`);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the examples' dev: blocks name a runtime this repo ships, and code that is there", () => {
  const repo = path.resolve(import.meta.dirname, "..", "..");
  const ejemplos = path.join(repo, "ejemplos");
  const root = install({ wasm: WASM, python: PYTHON });
  try {
    let seen = 0;
    for (const file of readdirSync(ejemplos).filter((f) => f.endsWith(".yseq"))) {
      const doc = parse(readFileSync(path.join(ejemplos, file), "utf8"));
      for (const [name, dev] of Object.entries(doc?.dev ?? {})) {
        seen += 1;
        // Throws if the runtime is not installed or `code` is not there, and
        // the message names which sequence it came from.
        assert.doesNotThrow(
          () => devCommand(dev, { sequenceDir: ejemplos, port: 9101, root, platform: "linux" }),
          `${file}: dev entry '${name}'`,
        );
      }
    }
    assert.ok(seen > 0, "the examples carry dev: blocks");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a runtime with no 'eof' is launched without one, and says so", () => {
  const root = install({ python: PYTHON });
  const project = mkdtempSync(path.join(tmpdir(), "anvil-seq-"));
  mkdirSync(path.join(project, "pasos"), { recursive: true });
  try {
    const cmd = devCommand(
      { runtime: "python", code: "pasos" },
      { sequenceDir: project, port: 9200, root, platform: "linux" },
    );
    assert.deepEqual(cmd.args, ["--steps", path.join(project, "pasos"), "--port", "9200"]);
    // Not an oversight to paper over: such a runtime is reaped on an orderly
    // close and not on a crash, and the launcher has to be able to tell.
    assert.equal(cmd.stopsOnEof, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
    rmSync(project, { recursive: true, force: true });
  }
});

test("an 'eof' that is not a flag is refused, not ignored", () => {
  const root = install({ wasm: { ...WASM, eof: "" } });
  try {
    assert.throws(() => readRuntime("wasm", root, "linux"), /'eof' is there but is not a flag/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
