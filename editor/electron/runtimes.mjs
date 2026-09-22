// The install folder: how a logical `runtime` name becomes a command line
// (ADR-0046 §4).
//
// `dev:` says `runtime: wasm`, never a path, because a path is the one thing
// in a committed file that differs per machine. So the name has to be resolved
// somewhere, and that somewhere is a folder of installed executors:
//
//     ~/.anvil/executors/
//       wasm/    executor.json  anvil-exec-wasm
//       python/  executor.json  anvil-exec-python
//
// The manifest is what lets a front end launch a runtime it knows nothing
// about. Every executor takes its code from a flag and its address from
// another, but they do not agree on the names — `--modules` for the WASM
// bridge, `--steps` for Python — and that single indirection is the only thing
// the install folder has to standardise (ADR-0046 §4).
//
// This module is deliberately free of Electron: it is `node:fs` and strings, so
// `editor/test/runtimes.test.mjs` can run it under plain `node --test`.
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";

/** What each runtime folder must carry. */
export const MANIFEST = "executor.json";

/**
 * Where installed executors live.
 *
 * `ANVIL_HOME` moves the whole thing — one variable, so a test, a CI job and a
 * bench that keeps its tools on another volume all have the same lever. There
 * is no search *path*: several locations is how a name starts meaning
 * different code on two machines, which is the failure ADR-0046 refused a
 * station file over.
 */
export function executorsRoot(env = process.env, home = homedir()) {
  const base = env.ANVIL_HOME || path.join(home, ".anvil");
  return path.join(base, "executors");
}

/** The runtimes installed under `root`, by name. Empty when there is no folder. */
export function installedRuntimes(root) {
  let entries;
  try {
    entries = readdirSync(root, { withFileTypes: true });
  } catch {
    return [];
  }
  return entries
    .filter((e) => e.isDirectory() && existsSync(path.join(root, e.name, MANIFEST)))
    .map((e) => e.name)
    .sort();
}

/**
 * The executable of a runtime folder.
 *
 * On Windows the name in the manifest is not the file: `anvil-exec-wasm` is
 * `anvil-exec-wasm.exe`, and a Python launcher arrives as a `.cmd`. Rather than
 * put a per-platform name in every manifest, the extensions Windows itself
 * would have tried are tried here — and named in the failure, because "not
 * found" without the list is the report that sends someone looking in the
 * wrong folder (ADR-0019, Rule 2).
 */
function resolveExe(dir, exec, platform) {
  const names = platform === "win32" ? [exec, `${exec}.exe`, `${exec}.cmd`, `${exec}.bat`] : [exec];
  for (const name of names) {
    const file = path.join(dir, name);
    if (existsSync(file) && statSync(file).isFile()) return file;
  }
  const tried = names.map((n) => `'${n}'`).join(", ");
  throw new Error(`the runtime '${path.basename(dir)}' is installed but its executable is missing: looked for ${tried} in ${dir}`);
}

function optionalString(manifest, field, file) {
  const value = manifest[field];
  if (value === undefined || value === null) return null;
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`${file}: '${field}' is there but is not a flag`);
  }
  return value;
}

function requireString(manifest, field, file) {
  const value = manifest[field];
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`${file} has no '${field}': a runtime manifest needs 'runtime', 'exec', 'code' and 'port'`);
  }
  return value;
}

/**
 * One installed runtime, read from its manifest:
 * `{ name, dir, exe, code, port, eof }` — where `code`, `port` and `eof` are
 * flag names, not values.
 *
 * Throws when the name is not installed, and the message names what was looked
 * for, where, and what is there instead. This is the exact point where an
 * install folder becomes a search-path problem if it stays quiet (ADR-0046 §g).
 */
export function readRuntime(name, root, platform = process.platform) {
  const dir = path.join(root, name);
  const file = path.join(dir, MANIFEST);
  if (!existsSync(file)) {
    const have = installedRuntimes(root);
    const instead = have.length > 0 ? `installed there: ${have.join(", ")}` : "nothing is installed there";
    throw new Error(`no runtime '${name}': looked for ${file} — ${instead}`);
  }

  let manifest;
  try {
    manifest = JSON.parse(readFileSync(file, "utf8"));
  } catch (e) {
    throw new Error(`${file} is not readable JSON: ${e.message}`);
  }
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) {
    throw new Error(`${file} is not a manifest object`);
  }

  // The folder is the name a sequence writes; the manifest says it too, and
  // they have to agree. A folder copied under another name and left saying the
  // old one is a runtime that answers to two names, which is how the same
  // `dev:` block starts meaning different code on two machines.
  const declared = requireString(manifest, "runtime", file);
  if (declared !== name) {
    throw new Error(`${file} says it is the runtime '${declared}', but it is installed as '${name}'`);
  }

  return {
    name,
    dir,
    exe: resolveExe(dir, requireString(manifest, "exec", file), platform),
    code: requireString(manifest, "code", file),
    port: requireString(manifest, "port", file),
    // Optional, and the only thing in here that is about the *launcher* rather
    // than the executor: the flag that makes it stop when its stdin closes.
    // A runtime that has one can be reaped by the operating system when
    // whoever started it dies without getting to run any code — a crash, a
    // SIGKILL, a machine that goes down. One that has none is reaped only on
    // an orderly close, and that is worth knowing rather than assuming.
    eof: optionalString(manifest, "eof", file),
  };
}

/**
 * The command that brings one executor up here: `{ exe, args, cwd, runtime }`.
 *
 * `dev.code` is relative to the sequence, like everything else a sequence
 * names (ADR-0046 §2), and it is checked before anything is spawned: a
 * directory that is not there produces a process that exits with its own
 * message on a stream nobody is reading yet.
 *
 * `port` is the executor's declared port. The editor starts it **where the
 * sequence says it listens** — starting it somewhere else would mean a run
 * that works only from the editor, which is the shape ADR-0046 removed.
 */
export function devCommand(dev, { sequenceDir, port, root, platform = process.platform }) {
  const runtime = readRuntime(dev.runtime, root, platform);
  const code = path.resolve(sequenceDir, dev.code ?? ".");
  if (!existsSync(code)) {
    throw new Error(`'${dev.code}' is not there: ${code}, resolved against the sequence`);
  }
  const args = [runtime.code, code, runtime.port, String(port)];
  if (runtime.eof) args.push(runtime.eof);
  return {
    exe: runtime.exe,
    args,
    cwd: sequenceDir,
    // Whether the caller should give it a stdin it never writes to. The pipe
    // is the mechanism: it closes when the launching process dies, however it
    // dies, and the executor goes with it.
    stopsOnEof: Boolean(runtime.eof),
    runtime,
  };
}

/**
 * Why this executor cannot be started from here, or null.
 *
 * An address that is not this machine's is the whole reason `dev:` is optional
 * and the engine ignores it: the bench at 192.168.1.50 was started by whoever
 * runs that bench, and the editor cannot and must not reach over to do it.
 */
export function notLocal(host) {
  const h = String(host ?? "").trim();
  if (h === "127.0.0.1" || h === "::1" || h === "localhost" || h === "0.0.0.0") return null;
  return `'${h}' is not this machine: start that executor where it runs, or point the sequence at a local one`;
}
